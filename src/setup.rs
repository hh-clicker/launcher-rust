//! Explicit preparation actions, downloads and installation.
#[cfg(test)]
use crate::preflight::{local_endpoint, parse_version};
use crate::{
    config::{self, Layout},
    preflight::{
        boot_identity, browser_directory, chrome, desktop_version, docker_cli, docker_desktop,
        node_runtime, virtualization, wsl_default, wsl_version, NODE_VERSION,
    },
    process,
};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        bail!("Ожидание подготовки отменено");
    }
    Ok(())
}
fn wait_for_engine(
    cancel: &AtomicBool,
    timeout: Duration,
    mut ready: impl FnMut() -> bool,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        check_cancelled(cancel)?;
        if ready() {
            return Ok(());
        }
        for _ in 0..30 {
            check_cancelled(cancel)?;
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    bail!("Docker Engine не подтвердил готовность за 5 минут. Проверьте окно Docker Desktop")
}

fn complete_installation(
    layout: &Layout,
    package: &Path,
    verified: Result<()>,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    verified?; // Never discard a package after a failed or cancelled installation.
    let downloads = layout.data.join("downloads");
    if package.parent() != Some(downloads.as_path()) {
        bail!("Очистка разрешена только для установщика этой portable-копии");
    }
    let removed = (|| -> Result<()> {
        let metadata = match fs::symlink_metadata(package) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("Необычный тип файла установщика");
        }
        fs::remove_file(package)?;
        Ok(())
    })();
    if let Err(error) = removed {
        // Cleanup failure must not turn a verified installation into a failed step.
        progress("Компонент установлен; не удалось удалить скачанный установщик. Подробности в logs/setup.log");
        let _ = fs::create_dir_all(layout.data.join("logs"));
        if let Ok(mut log) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(layout.data.join("logs/setup.log"))
        {
            let _ = writeln!(log, "Download cleanup {}: {error:#}", package.display());
        }
    }
    Ok(())
}
fn download_progress(label: &str, bytes: u64, total: Option<u64>) -> String {
    match total.filter(|n| *n > 0) {
        Some(total) => format!(
            "{label}: {} из {} МБ ({}%)",
            bytes / (1024 * 1024),
            total / (1024 * 1024),
            (u128::from(bytes.min(total)) * 100 / u128::from(total))
        ),
        None => format!("{label}: загружено {} МБ", bytes / (1024 * 1024)),
    }
}
fn prepare_docker_start_settings(path: &Path) -> Result<()> {
    let mut settings = if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1_048_576 {
            bail!("Неизвестный формат настроек Docker Desktop");
        }
        serde_json::from_slice::<serde_json::Value>(&fs::read(path)?)?
    } else {
        serde_json::json!({})
    };
    let object = settings
        .as_object_mut()
        .context("Неизвестный формат настроек Docker Desktop")?;
    if object.get("DisplayedOnboarding") == Some(&serde_json::json!(true))
        && object.get("OpenUIOnStartupDisabled") == Some(&serde_json::json!(true))
    {
        return Ok(());
    }
    object.insert("DisplayedOnboarding".into(), serde_json::json!(true));
    object.insert("OpenUIOnStartupDisabled".into(), serde_json::json!(true));
    fs::create_dir_all(path.parent().context("Docker settings directory")?)?;
    config::atomic_write(path, &serde_json::to_vec_pretty(&settings)?)
}
fn start_docker_desktop(
    cli: &Path,
    desktop: Option<&Path>,
    cancel: &AtomicBool,
    mut run: impl FnMut(std::process::Command, Duration) -> Result<String>,
    mut launch: impl FnMut(std::process::Command) -> Result<()>,
) -> Result<()> {
    check_cancelled(cancel)?;
    let mut start = process::docker_command(cli);
    start.args(["desktop", "start", "--detach"]);
    if let Some(directory) = desktop.and_then(|p| p.parent()) {
        start.current_dir(directory);
    }
    if run(start, Duration::from_secs(120)).is_err() {
        check_cancelled(cancel)?;
        let desktop =
            desktop.context("CLI не запустил Docker Desktop, а Docker Desktop.exe не найден")?;
        let mut start = process::command(desktop);
        if let Some(directory) = desktop.parent() {
            start.current_dir(directory);
        }
        launch(start).context("Не удалось запустить Docker Desktop.exe")?;
    }
    Ok(())
}
fn docker_user_scope(path: &Path) -> Result<bool> {
    for machine in [false, true] {
        if let Some(location) = crate::platform::registry_string(
            machine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Docker Desktop",
            "InstallLocation",
        ) {
            if path == PathBuf::from(location.trim_matches('"')).join("Docker Desktop.exe") {
                return Ok(!machine);
            }
        }
    }
    if std::env::var_os("LOCALAPPDATA").is_some_and(|base| path.starts_with(PathBuf::from(base))) {
        return Ok(true);
    }
    if std::env::var_os("ProgramFiles").is_some_and(|base| path.starts_with(PathBuf::from(base))) {
        return Ok(false);
    }
    bail!("Не удалось доказать область установки Docker. Выполните обновление вручную через Docker Desktop")
}

fn node_with_npm(layout: &Layout) -> Option<PathBuf> {
    node_runtime(layout)
        .into_iter()
        .chain(Some(layout.data.join(format!(
            "runtimes/node-v{NODE_VERSION}-win-x64/node.exe"
        ))))
        .find(|p| {
            p.is_file()
                && p.parent()
                    .unwrap()
                    .join("node_modules/npm/bin/npm-cli.js")
                    .is_file()
                && process::query(p, &["--version"]).is_ok_and(|v| config::node_compatible(&v))
        })
}

pub fn download(
    url: &str,
    path: &Path,
    max: u64,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    // Do not negotiate gzip for installer binaries: automatic decompression strips
    // Content-Length and prevents the same progress reporting used by the Kotlin launcher.
    let response = crate::auth::agent(1800)
        .get(url)
        .set("Accept-Encoding", "identity")
        .call()?;
    let total = response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());
    if total.is_some_and(|n| n > max) {
        bail!("Файл превышает допустимый размер");
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let partial = path.with_extension("partial");
    let mut file = fs::File::create(&partial)?;
    let mut reader = response.into_reader();
    let mut buf = [0; 65536];
    let mut done = 0;
    let mut last = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > max {
            bail!("Превышен допустимый размер загрузки");
        }
        file.write_all(&buf[..n])?;
        if last.elapsed() > Duration::from_millis(200) {
            progress(done, total);
            last = std::time::Instant::now();
        }
    }
    progress(done, total);
    if total.is_some_and(|n| n != done) {
        bail!("Загрузка неполная");
    }
    file.sync_all()?;
    drop(file);
    crate::platform::replace_file(&partial, path)
}
pub fn extract_zip(file: &Path, dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(file)?)?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let relative = entry
            .enclosed_name()
            .context("Небезопасный путь внутри ZIP")?;
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("Символические ссылки в ZIP запрещены");
        }
        total = total.checked_add(entry.size()).context("ZIP overflow")?;
        if total > 1_500_000_000 {
            bail!("Слишком большой распакованный архив");
        }
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(target)?;
        } else {
            fs::create_dir_all(target.parent().unwrap())?;
            std::io::copy(&mut entry, &mut fs::File::create(target)?)?;
        }
    }
    Ok(())
}
pub fn prepare_node(layout: &Layout, mut progress: impl FnMut(&str)) -> Result<()> {
    if node_with_npm(layout).is_some() {
        progress("Совместимый Node.js и npm уже доступны");
        return Ok(());
    }
    let filename = format!("node-v{NODE_VERSION}-win-x64.zip");
    let base = format!("https://nodejs.org/dist/v{NODE_VERSION}");
    let sums = crate::auth::agent(30)
        .get(&format!("{base}/SHASUMS256.txt"))
        .call()?
        .into_string()?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let mut p = line.split_whitespace();
            let hash = p.next()?;
            if p.next()? == filename {
                Some(hash.to_owned())
            } else {
                None
            }
        })
        .context("Нет SHA-256 для Node.js Windows x64")?;
    let archive = layout.data.join("downloads").join(&filename);
    progress("Скачиваем официальный Node.js portable…");
    download(
        &format!("{base}/{filename}"),
        &archive,
        150_000_000,
        |done, total| progress(&download_progress("Node.js", done, total)),
    )?;
    let digest = format!("{:x}", Sha256::digest(fs::read(&archive)?));
    if digest != expected {
        bail!("SHA-256 Node.js не совпадает");
    }
    extract_zip(&archive, &layout.data.join("runtimes"))?;
    let portable = layout
        .data
        .join(format!("runtimes/node-v{NODE_VERSION}-win-x64"));
    let verified =
        process::query(portable.join("node.exe").as_os_str(), &["--version"]).and_then(|version| {
            if version.trim() != format!("v{NODE_VERSION}")
                || !portable.join("node_modules/npm/bin/npm-cli.js").is_file()
            {
                bail!("Проверка portable Node.js/npm не пройдена");
            }
            Ok(())
        });
    complete_installation(layout, &archive, verified, &mut progress)?;
    Ok(())
}
pub fn prepare_browser(layout: &Layout, mut progress: impl FnMut(&str)) -> Result<()> {
    if browser_directory(layout).is_some() {
        progress("Browser Agent и его зависимости уже готовы");
        return Ok(());
    }
    let node = node_with_npm(layout).context("Сначала подготовьте Node.js с npm")?;
    layout.verify_payload()?;
    let dest = layout.browser_directory()?;
    let parent = dest.parent().context("Каталог Browser Agent")?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".prepare-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<()> {
        // Copy only known application inputs. Never clone profiles, env files or snapshots.
        copy_tree(
            &layout.payload.join("browser-agent/dist"),
            &staging.join("dist"),
        )?;
        copy_tree(
            &layout.payload.join("browser-agent/resources"),
            &staging.join("resources"),
        )?;
        for name in ["package.json", "package-lock.json"] {
            fs::copy(
                layout.payload.join("browser-agent").join(name),
                staging.join(name),
            )?;
        }
        let npm = node
            .parent()
            .unwrap()
            .join("node_modules/npm/bin/npm-cli.js");
        if !npm.is_file() {
            bail!("npm рядом с Node.js не найден. Подготовьте официальный portable Node.js");
        }
        progress("Скачиваем зависимости Browser Agent (npm ci --omit=dev)…");
        let npm_config = staging.join("empty.npmrc");
        fs::write(&npm_config, b"")?;
        let npm_global_config = staging.join("global.npmrc");
        fs::write(&npm_global_config, b"")?;
        let mut c = process::command(&node);
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("npm_config_")
            {
                c.env_remove(key);
            }
        }
        c.arg(npm)
            .args([
                "ci",
                "--omit=dev",
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
            ])
            .current_dir(&staging)
            .env("npm_config_cache", layout.data.join("npm-cache"))
            .env("npm_config_userconfig", &npm_config)
            .env("npm_config_globalconfig", &npm_global_config)
            .env("npm_config_registry", "https://registry.npmjs.org/")
            .env("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD", "1");
        c.env("HH_RUST_COMMAND_LOG", layout.data.join("logs/setup.log"));
        process::run(c, Duration::from_secs(600))?;
        progress("Проверяем зависимости и ресурсы Browser Agent…");
        let mut c = process::command(&node);
        c.args(["--input-type=module", "--eval", "await import('playwright'); const {loadHhSelectors}=await import('./dist/hh/selector-registry.js'); await loadHhSelectors();"])
        .current_dir(&staging)
        .env("HH_CLICKER_BROWSER_PROFILE_DIR", layout.data.join("browser-profiles"));
        c.env("HH_RUST_COMMAND_LOG", layout.data.join("logs/setup.log"));
        process::run(c, Duration::from_secs(180))?;
        if !staging.join("dist/main.js").is_file() || !config::browser_resources_present(&staging) {
            bail!("Browser Agent не подготовлен");
        }
        fs::write(
            staging.join(".dependencies-ready"),
            b"locked-production-v1\n",
        )?;
        if dest.exists() {
            fs::rename(
                &dest,
                parent.join(format!("incomplete-{}", uuid::Uuid::new_v4())),
            )?;
        }
        fs::rename(&staging, &dest)?;
        Ok(())
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    progress("Browser Agent готов");
    Ok(())
}
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("Ссылки в payload запрещены");
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
fn publisher_matches(subject: &str, publisher: &str) -> bool {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in subject.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if c == ',' && !quoted {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    let fields: Vec<_> = parts
        .iter()
        .filter_map(|p| {
            p.split_once('=').map(|(k, v)| {
                (
                    k.trim().to_uppercase(),
                    v.trim().trim_end_matches('.').to_uppercase(),
                )
            })
        })
        .collect();
    fields
        .iter()
        .find(|(k, _)| k == "O")
        .or_else(|| fields.iter().find(|(k, _)| k == "CN"))
        .is_some_and(|(_, v)| v == &publisher.to_uppercase())
}
fn verify_signature(path: &Path, publisher: &str) -> Result<()> {
    let out=process::powershell("$ErrorActionPreference='Stop'; $s=Get-AuthenticodeSignature -LiteralPath $env:HH_RUST_PACKAGE; if($s.Status -ne 'Valid'){throw 'Invalid Authenticode signature'}; $s.SignerCertificate.Subject",&[("HH_RUST_PACKAGE",path.as_os_str())],Duration::from_secs(60))?;
    if !publisher_matches(&out, publisher) {
        bail!("Неожиданный издатель установщика: нужен {publisher}");
    }
    Ok(())
}
#[derive(Clone, Copy, Debug)]
pub enum Preparation {
    Wsl,
    WslUpdate,
    WslDefault,
    DockerInstall,
    DockerStart,
    Chrome,
    Node,
    Browser,
}
impl Preparation {
    pub fn description(self) -> &'static str {
        match self {
        Self::Wsl=>"Установить WSL 2 без дистрибутива Linux. Windows может запросить права администратора и перезагрузку.",
        Self::WslUpdate=>"Обновить системный WSL. Windows может запросить права администратора и перезагрузку.",
        Self::WslDefault=>"Выбрать WSL 2 версией по умолчанию для текущего пользователя.",
        Self::DockerInstall=>"Скачать, установить и запустить официальный Docker Desktop, дождаться готовности Linux Engine. Новая установка — для текущего пользователя с WSL 2; обновление сохраняет область установки и backend. Нажатие «Принимаю и установить» означает принятие Docker Subscription Service Agreement.",
        Self::DockerStart=>"Запустить установленный Docker Desktop. Это изменит состояние системной зависимости.",
        Self::Chrome=>"Скачать официальный Google Chrome x64 MSI и установить в Windows. Нажатие «Принимаю и установить» означает принятие условий Google Chrome. Существующий Chrome не изменяется.",
        Self::Node=>"Скачать официальный Node.js 24.20.0 в data/runtimes и проверить SHA-256. Системный PATH не изменяется.",
        Self::Browser=>"Подготовить Browser Agent в data и скачать зависимости npm из lock-файла. Нужен интернет при первой подготовке. Сервисы и браузер не запускаются.",
    }
    }
}
pub fn prepare(layout: &Layout, action: Preparation, progress: impl FnMut(&str)) -> Result<()> {
    prepare_cancellable(layout, action, &AtomicBool::new(false), progress)
}
pub fn prepare_cancellable(
    layout: &Layout,
    action: Preparation,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    check_cancelled(cancel)?;
    match action {
        Preparation::Node => prepare_node(layout, &mut progress),
        Preparation::Browser => prepare_browser(layout, &mut progress),
        Preparation::Wsl | Preparation::WslUpdate => {
            virtualization()?;
            if wsl_version().is_ok() {
                progress("Совместимый WSL уже установлен; системных изменений нет");
                return Ok(());
            }
            let boot = boot_identity()?;
            progress("Ожидаем системную подготовку WSL…");
            let args = if matches!(action, Preparation::Wsl) {
                "--install --no-distribution --web-download"
            } else {
                "--update --web-download"
            };
            // Fixed arguments, no shell interpolation of paths. The UI is the explicit authorization.
            let script=format!("$ErrorActionPreference='Stop'; $p=Start-Process -FilePath wsl.exe -ArgumentList '{args}' -Verb RunAs -WindowStyle Normal -Wait -PassThru; if($p.ExitCode -notin @(0,3010)){{throw ('WSL exit '+$p.ExitCode)}}");
            process::powershell(&script, &[], Duration::from_secs(1800))?;
            config::atomic_write(
                &layout.data.join("reboot-required.json"),
                &serde_json::to_vec(&serde_json::json!({"boot":boot.trim()}))?,
            )?;
            progress("WSL подготовлен. Перезагрузите Windows, затем запустите лаунчер снова");
            Ok(())
        }
        Preparation::WslDefault => {
            if wsl_default().is_ok() {
                progress("WSL 2 уже выбран по умолчанию; изменений нет");
                return Ok(());
            }
            process::query("wsl.exe", &["--set-default-version", "2"])?;
            Ok(())
        }
        Preparation::DockerStart => {
            let desktop = docker_desktop();
            let cli = docker_cli().context("Docker CLI не найден")?;
            let engine_ready = || -> bool {
                let mut command = process::docker_command(&cli);
                command.args(["info", "--format", "{{.OSType}}"]);
                command.env(
                    "HH_RUST_COMMAND_LOG",
                    layout.data.join("logs/docker-start.log"),
                );
                process::run_cancellable(command, Duration::from_secs(12), cancel)
                    .is_ok_and(|v| v.trim() == "linux")
            };
            if engine_ready() {
                progress("Docker Engine уже работает; существующий процесс не меняется");
                return Ok(());
            }
            if let Some(appdata) = std::env::var_os("APPDATA") {
                // Same optional onboarding settings as the reference. Malformed existing
                // settings are preserved; failure here must not block starting Desktop.
                let _ = prepare_docker_start_settings(
                    &PathBuf::from(appdata).join("Docker/settings-store.json"),
                );
            }
            progress("Запускаем Docker Desktop…");
            start_docker_desktop(
                &cli,
                desktop.as_deref(),
                cancel,
                |mut command, timeout| {
                    command.env(
                        "HH_RUST_COMMAND_LOG",
                        layout.data.join("logs/docker-start.log"),
                    );
                    process::run_desktop_launcher(command, timeout, cancel)
                },
                |mut command| {
                    check_cancelled(cancel)?;
                    command
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    command.spawn()?;
                    Ok(())
                },
            )?;
            progress("Ожидаем готовность Docker Engine…");
            wait_for_engine(cancel, Duration::from_secs(300), engine_ready)?;
            progress("Docker Engine готов");
            Ok(())
        }
        Preparation::DockerInstall => {
            let existing = docker_desktop();
            if let Some(path) = &existing {
                if desktop_version(path)? >= [4, 0, 0] {
                    progress(
                        "Docker Desktop соответствует минимальной версии. Обновление не требуется",
                    );
                    return prepare_cancellable(layout, Preparation::DockerStart, cancel, progress);
                }
            }
            if existing.is_none() && docker_cli().is_some() {
                bail!("Найден Docker CLI без подтверждённой установки Desktop. Автоматическая замена запрещена; проверьте установку вручную");
            }
            virtualization()?;
            wsl_version()?;
            let package = layout.data.join("downloads/Docker Desktop Installer.exe");
            progress("Скачиваем Docker Desktop…");
            download(
                "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe",
                &package,
                1_500_000_000,
                |n, total| progress(&download_progress("Docker", n, total)),
            )?;
            verify_signature(&package, "Docker Inc")?;
            progress("Устанавливаем Docker Desktop…");
            let mut c = process::command(&package);
            c.args(["install", "--quiet", "--accept-license"]);
            let user_scope = match existing.as_ref() {
                Some(path) => docker_user_scope(path)?,
                None => true,
            };
            if user_scope {
                c.arg("--user");
            }
            if existing.is_none() {
                c.args(["--backend=wsl-2", "--no-windows-containers"]);
            }
            if user_scope {
                process::run(c, Duration::from_secs(1800))?;
            } else {
                process::powershell("$ErrorActionPreference='Stop'; $p=Start-Process -FilePath $env:HH_RUST_PACKAGE -ArgumentList @('install','--quiet','--accept-license') -Verb RunAs -WindowStyle Hidden -Wait -PassThru; if($p.ExitCode -notin @(0,3010)){throw ('Docker installer exit '+$p.ExitCode)}",&[("HH_RUST_PACKAGE",package.as_os_str())],Duration::from_secs(1800))?;
            }
            let verified = docker_desktop()
                .context("Установщик завершился, но Docker Desktop не найден")
                .and_then(|path| {
                    if desktop_version(&path)? < [4, 0, 0] {
                        bail!("Установка Docker Desktop не подтверждена");
                    }
                    Ok(())
                });
            complete_installation(layout, &package, verified, &mut progress)?;
            progress("Docker Desktop установлен. Запускаем и ожидаем готовность…");
            prepare_cancellable(layout, Preparation::DockerStart, cancel, progress)
        }
        Preparation::Chrome => {
            if chrome().is_some() {
                progress("Google Chrome уже установлен; изменений нет");
                return Ok(());
            }
            let package = layout.data.join("downloads/GoogleChromeEnterprise64.msi");
            progress("Скачиваем Google Chrome…");
            download(
                "https://dl.google.com/dl/chrome/install/googlechromestandaloneenterprise64.msi",
                &package,
                300_000_000,
                |n, total| progress(&download_progress("Chrome", n, total)),
            )?;
            verify_signature(&package, "Google LLC")?;
            progress("Устанавливаем Google Chrome. Подтвердите запрос Windows, если он появится…");
            crate::platform::install_msi(
                &package,
                &layout.data.join("logs/chrome-install.log"),
                Duration::from_secs(900),
            )?;
            let verified = chrome()
                .context("Установка завершилась, но Chrome не найден")
                .map(|_| ());
            complete_installation(layout, &package, verified, &mut progress)?;
            progress("Chrome установлен. При первом открытии может потребоваться выбор поисковой системы");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downloads_are_removed_only_after_verified_success() {
        let root = std::env::temp_dir().join(format!("hh-cleanup-{}", uuid::Uuid::new_v4()));
        let layout = Layout::new(root.clone(), None);
        fs::create_dir_all(layout.data.join("downloads")).unwrap();
        let chrome = layout.data.join("downloads/chrome.msi");
        let docker = layout.data.join("downloads/docker.exe");
        let node = layout.data.join("downloads/node.zip");
        for package in [&chrome, &docker, &node] {
            fs::write(package, b"installer").unwrap();
        }
        for reason in [
            "installation failed",
            "cancelled",
            "component verification failed",
        ] {
            assert!(complete_installation(
                &layout,
                &chrome,
                Err(anyhow::anyhow!(reason)),
                |_| panic!("Must preserve download")
            )
            .is_err());
            assert!(chrome.is_file());
        }
        complete_installation(&layout, &chrome, Ok(()), |_| {
            panic!("Cleanup should succeed")
        })
        .unwrap();
        assert!(!chrome.exists());
        assert!(docker.is_file() && node.is_file());
        let protected = layout.data.join("keep-profile");
        fs::write(&protected, b"keep").unwrap();
        assert!(complete_installation(&layout, &protected, Ok(()), |_| {}).is_err());
        assert_eq!(fs::read(&protected).unwrap(), b"keep");
        complete_installation(&layout, &docker, Ok(()), |_| {}).unwrap();
        complete_installation(&layout, &node, Ok(()), |_| {}).unwrap();
        assert!(!docker.exists() && !node.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cleanup_error_does_not_fail_an_installed_component_or_remove_directories() {
        let root = std::env::temp_dir().join(format!("hh-cleanup-error-{}", uuid::Uuid::new_v4()));
        let layout = Layout::new(root.clone(), None);
        let path = layout.data.join("downloads/not-a-file");
        fs::create_dir_all(&path).unwrap();
        let mut warning = false;
        complete_installation(&layout, &path, Ok(()), |_| warning = true).unwrap();
        assert!(warning && path.is_dir());
        assert!(layout.data.join("logs/setup.log").is_file());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn engine_wait_is_cancelled_without_waiting_for_ready_or_timeout() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, Ordering::Release);
        });
        let start = std::time::Instant::now();
        assert!(wait_for_engine(&cancel, Duration::from_secs(300), || false).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        thread.join().unwrap();
    }
    #[test]
    fn closing_during_cli_start_does_not_launch_fallback_or_new_preparation() {
        let cancel = AtomicBool::new(false);
        let result = start_docker_desktop(
            Path::new(r"C:\Docker\docker.exe"),
            Some(Path::new(r"C:\Docker\Docker Desktop.exe")),
            &cancel,
            |_, _| {
                cancel.store(true, Ordering::Release);
                bail!("Cancelled");
            },
            |_| panic!("Closing must not start a fallback Desktop"),
        );
        assert!(result.is_err());
        let layout = Layout::new(
            std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
            None,
        );
        assert!(
            prepare_cancellable(&layout, Preparation::DockerInstall, &cancel, |_| panic!(
                "No work after cancellation"
            ))
            .is_err()
        );
        assert!(!layout.data.exists());
    }
    #[test]
    fn docker_cli_start_uses_executable_fallback_only_on_failure() {
        use std::cell::Cell;
        for cli_fails in [false, true] {
            let called = Cell::new(0);
            let cli = Path::new(r"C:\Docker\resources\bin\docker.exe");
            let desktop = Path::new(r"C:\Docker\Docker Desktop.exe");
            start_docker_desktop(
                cli,
                Some(desktop),
                &AtomicBool::new(false),
                |command, timeout| {
                    assert_eq!(called.get(), 0);
                    called.set(1);
                    assert_eq!(command.get_program(), cli.as_os_str());
                    assert_eq!(
                        command.get_args().collect::<Vec<_>>(),
                        vec!["desktop", "start", "--detach"]
                    );
                    assert_eq!(timeout, Duration::from_secs(120));
                    if cli_fails {
                        bail!("CLI unavailable");
                    }
                    Ok(String::new())
                },
                |command| {
                    assert_eq!(called.get(), 1);
                    called.set(2);
                    assert_eq!(command.get_program(), desktop.as_os_str());
                    assert_eq!(command.get_current_dir(), desktop.parent());
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(called.get(), if cli_fails { 2 } else { 1 });
        }
    }
    #[test]
    fn unknown_download_size_has_no_zero_denominator() {
        assert_eq!(
            download_progress("Chrome", 50_000_000, None),
            "Chrome: загружено 47 МБ"
        );
        assert_eq!(
            download_progress("Chrome", 50_000_000, Some(0)),
            "Chrome: загружено 47 МБ"
        );
        assert_eq!(
            download_progress("Docker", 50_000_000, Some(623_000_000)),
            "Docker: 47 из 594 МБ (8%)"
        );
        assert_eq!(
            download_progress("Chrome", 80, Some(80)),
            "Chrome: 0 из 0 МБ (100%)"
        );
        assert!(download_progress("Chrome", u64::MAX, Some(1)).ends_with("(100%)"));
    }
    #[test]
    fn installer_download_requests_identity_and_finishes_with_known_total() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/installer", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(std::time::Instant::now() < deadline, "No download request");
                std::thread::sleep(Duration::from_millis(10));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
                assert!(request.len() < 16384);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .to_ascii_lowercase()
                .contains("accept-encoding: identity\r\n"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ninstaller",
                )
                .unwrap();
        });
        let root = std::env::temp_dir().join(format!("hh-download-{}", uuid::Uuid::new_v4()));
        let target = root.join("installer.msi");
        let mut updates = Vec::new();
        download(&url, &target, 1024, |done, total| {
            updates.push((done, total))
        })
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"installer");
        assert_eq!(updates.last(), Some(&(9, Some(9))));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn docker_start_settings_preserve_unknown_and_invalid_values() {
        let root =
            std::env::temp_dir().join(format!("hh-docker-settings-{}", uuid::Uuid::new_v4()));
        let path = root.join("settings-store.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &path,
            br#"{"custom":{"keep":true},"DisplayedOnboarding":false}"#,
        )
        .unwrap();
        prepare_docker_start_settings(&path).unwrap();
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(settings["custom"]["keep"], true);
        assert_eq!(settings["DisplayedOnboarding"], true);
        assert_eq!(settings["OpenUIOnStartupDisabled"], true);
        fs::write(&path, b"broken settings").unwrap();
        assert!(prepare_docker_start_settings(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken settings");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_local_windows_docker_endpoint() {
        assert!(local_endpoint("npipe:////./pipe/dockerDesktopLinuxEngine"));
        for host in [
            "tcp://127.0.0.1:2375",
            "ssh://host",
            "npipe:////remote/pipe/docker_engine",
        ] {
            assert!(!local_endpoint(host));
        }
    }
    #[test]
    fn exact_signature_publisher() {
        assert!(publisher_matches(
            "CN=Docker Inc., O=Docker Inc., C=US",
            "Docker Inc"
        ));
        assert!(publisher_matches(
            "CN=Google LLC, O=Google LLC, C=US",
            "Google LLC"
        ));
        assert!(!publisher_matches("O=Docker Inc Evil, C=US", "Docker Inc"));
        assert!(!publisher_matches(
            "CN=Google LLC, O=Other Company",
            "Google LLC"
        ));
    }
    #[test]
    fn wsl_first_line_version() {
        assert_eq!(parse_version("WSL: 2.1.5.0"), Some([2, 1, 5]));
        assert_eq!(parse_version("WSL package missing"), None);
    }
    #[test]
    fn zip_traversal_rejected() {
        use zip::{write::SimpleFileOptions, ZipWriter};
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("test.zip");
        let mut z = ZipWriter::new(fs::File::create(&archive).unwrap());
        z.start_file("../escape", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        assert!(extract_zip(&archive, &root.join("out")).is_err());
        assert!(!root.join("escape").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
