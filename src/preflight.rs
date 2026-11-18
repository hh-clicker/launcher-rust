//! Read-only discovery and prerequisite checks. Never installs or changes system settings.
use crate::{
    config::{self, Layout},
    process,
    runtime::Check,
};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub const NODE_VERSION: &str = "24.20.0";
fn docker_query(cli: &Path, args: &[&str]) -> Result<String> {
    let mut command = process::docker_command(cli);
    command.args(args);
    process::run(command, Duration::from_secs(12))
}
pub fn browser_directory(layout: &Layout) -> Option<PathBuf> {
    let bundled = layout.payload.join("browser-agent");
    [Some(bundled.clone()), layout.browser_directory().ok()]
        .into_iter()
        .flatten()
        .find(|p| {
            p.join("dist/main.js").is_file()
                && p.join("node_modules/playwright/package.json").is_file()
                && config::browser_resources_present(p)
                && (p == &bundled || p.join(".dependencies-ready").is_file())
        })
}
pub fn docker_cli() -> Option<PathBuf> {
    let mut extras = Vec::new();
    if let Some(path) =
        docker_desktop().and_then(|p| p.parent().map(|p| p.join("resources/bin/docker.exe")))
    {
        extras.push(path);
    }
    for base in [
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs")),
    ]
    .into_iter()
    .flatten()
    {
        extras.push(base.join("Docker/Docker/resources/bin/docker.exe"));
        extras.push(base.join("Docker/resources/bin/docker.exe"));
    }
    process::find_executable("docker.exe", &extras)
}
pub fn chrome() -> Option<PathBuf> {
    for machine in [false, true] {
        if let Some(path) = crate::platform::registry_string(
            machine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe",
            "",
        )
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        {
            return Some(path);
        }
    }
    ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("Google/Chrome/Application/chrome.exe"))
        .find(|p| p.is_file())
}
pub fn docker_desktop() -> Option<PathBuf> {
    let mut paths = Vec::new();
    for machine in [false, true] {
        if let Some(path) = crate::platform::registry_string(
            machine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Docker Desktop",
            "InstallLocation",
        ) {
            paths.push(PathBuf::from(path.trim_matches('"')).join("Docker Desktop.exe"));
        }
        if let Some(path) = crate::platform::registry_string(
            machine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Docker Desktop.exe",
            "",
        ) {
            paths.push(PathBuf::from(path.trim_matches('"')));
        }
    }
    if let Some(base) = std::env::var_os("ProgramFiles") {
        paths.push(PathBuf::from(base).join("Docker/Docker/Docker Desktop.exe"));
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(base);
        paths.push(base.join("Programs/DockerDesktop/Docker Desktop.exe"));
        paths.push(base.join("Programs/Docker/Docker/Docker Desktop.exe"));
        paths.push(base.join("Programs/Docker/Docker Desktop.exe"));
    }
    paths.into_iter().find(|p| p.is_file())
}
pub fn local_endpoint(host: &str) -> bool {
    host.trim()
        .to_ascii_lowercase()
        .starts_with("npipe:////./pipe/")
}
pub fn local_docker() -> Result<String> {
    let docker = docker_cli().context("Docker CLI не найден")?;
    let explicit_context = std::env::var_os("DOCKER_CONTEXT").is_some_and(|v| !v.is_empty());
    let host = if let Some(value) =
        std::env::var_os("DOCKER_HOST").filter(|v| !v.is_empty() && !explicit_context)
    {
        value.to_string_lossy().into_owned()
    } else {
        docker_query(
            &docker,
            &[
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ],
        )?
    };
    if !local_endpoint(&host) {
        bail!("Выбран удалённый или неподдерживаемый Docker endpoint. Выберите локальный контекст Docker Desktop; лаунчер не меняет контексты автоматически");
    }
    Ok("Локальный Windows named pipe Docker Desktop".into())
}
pub fn parse_version(s: &str) -> Option<[u32; 3]> {
    for token in s.split(|c: char| !c.is_ascii_digit() && c != '.') {
        let parts: Vec<_> = token.trim_matches('.').split('.').collect();
        if parts.len() >= 3 {
            if let (Ok(a), Ok(b), Ok(c)) = (parts[0].parse(), parts[1].parse(), parts[2].parse()) {
                return Some([a, b, c]);
            }
        }
    }
    None
}
pub(crate) fn desktop_version(path: &Path) -> Result<[u32; 3]> {
    crate::platform::product_version(path).context("Не удалось определить версию Docker Desktop")
}
pub fn node_runtime(layout: &Layout) -> Option<PathBuf> {
    node_runtime_version(layout).map(|(path, _)| path)
}
fn node_runtime_version(layout: &Layout) -> Option<(PathBuf, String)> {
    let portable = layout
        .data
        .join(format!("runtimes/node-v{NODE_VERSION}-win-x64/node.exe"));
    let candidates = process::find_executable("node.exe", &[])
        .into_iter()
        .chain(Some(portable));
    candidates.filter(|p| p.is_file()).find_map(|p| {
        let version = process::query(p.as_os_str(), &["--version"]).ok()?;
        config::node_compatible(&version).then_some((p, version))
    })
}
fn check(name: &str, outcome: Result<String>, required: bool) -> Check {
    match outcome {
        Ok(message) => Check {
            name: name.into(),
            passed: true,
            message,
            required,
        },
        Err(e) => Check {
            name: name.into(),
            passed: false,
            message: e.to_string(),
            required,
        },
    }
}
pub fn virtualization() -> Result<String> {
    // Hyper-V may hide firmware features; keep the CIM fallback for negatives.
    if crate::platform::virtualization_features_available() {
        return Ok("Аппаратная виртуализация доступна".into());
    }
    let out=process::powershell("$ErrorActionPreference='Stop'; $s=Get-CimInstance Win32_ComputerSystem; $c=@(Get-CimInstance Win32_Processor); if($s.HypervisorPresent -or ($c.Count -gt 0 -and @($c | Where-Object { -not $_.VirtualizationFirmwareEnabled -or -not $_.SecondLevelAddressTranslationExtensions }).Count -eq 0)){'enabled'}else{'disabled'}",&[],Duration::from_secs(15))?;
    if out.trim() != "enabled" {
        bail!("Включите аппаратную виртуализацию в BIOS/UEFI");
    }
    Ok("Аппаратная виртуализация доступна".into())
}
pub fn wsl_version() -> Result<String> {
    let output = process::query("wsl.exe", &["--version"])
        .context("WSL не установлен или версия недоступна")?;
    let version = parse_version(output.lines().find(|s| !s.trim().is_empty()).unwrap_or(""))
        .context("Не удалось определить версию пакета WSL")?;
    if version < [2, 1, 5] {
        bail!(
            "WSL {}.{}.{} устарел. Требуется 2.1.5 или новее",
            version[0],
            version[1],
            version[2]
        );
    }
    Ok(format!("WSL {}.{}.{}", version[0], version[1], version[2]))
}
pub fn wsl_default() -> Result<String> {
    let value = crate::platform::registry_dword(
        false,
        r"Software\Microsoft\Windows\CurrentVersion\Lxss",
        "DefaultVersion",
    )
    .context("Не удалось прочитать версию WSL по умолчанию")?;
    if value != 2 {
        bail!("Выберите WSL 2 по умолчанию");
    }
    Ok("WSL 2 по умолчанию".into())
}
pub fn probe(layout: &Layout) -> Vec<Check> {
    let mut checks = vec![
        check("Виртуализация", virtualization(), true),
        check(
            "WSL",
            wsl_version().map(|v| {
                v.lines()
                    .find(|s| !s.is_empty())
                    .unwrap_or("WSL доступен")
                    .into()
            }),
            true,
        ),
        check("WSL 2", wsl_default(), true),
    ];
    if layout.data.join("reboot-required.json").is_file() {
        let reboot = check("Перезагрузка Windows", reboot_check(layout), true);
        // A completed reboot is no longer a preparation step. Keep its record
        // intact: observing readiness must not delete application state.
        if !reboot.passed {
            checks.push(reboot);
        }
    }
    let docker = docker_cli();
    checks.push(check("Docker endpoint", local_docker(), true));
    checks.push(check(
        "Docker Desktop",
        docker_desktop()
            .context("Docker Desktop не найден")
            .and_then(|p| {
                let version = desktop_version(&p)?;
                if version < [4, 0, 0] {
                    bail!("Docker Desktop ниже минимальной версии 4.0.0; обновите через интерфейс");
                }
                Ok(format!(
                    "{}.{}.{}{}",
                    version[0],
                    version[1],
                    version[2],
                    if version < [4, 37, 0] {
                        " · рекомендуется обновление"
                    } else {
                        ""
                    }
                ))
            }),
        true,
    ));
    checks.push(check(
        "Docker CLI",
        docker
            .as_ref()
            .context("Установите Docker Desktop")
            .and_then(|p| docker_query(p, &["--version"])),
        true,
    ));
    checks.push(check(
        "Docker Compose V2",
        docker
            .as_ref()
            .context("Docker CLI не найден")
            .and_then(|p| {
                let v = docker_query(p, &["compose", "version", "--short"])?;
                let major = v
                    .trim()
                    .trim_start_matches('v')
                    .split('.')
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                if major < 2 {
                    bail!("Нужен Docker Compose V2");
                }
                Ok(v)
            }),
        true,
    ));
    checks.push(check(
        "Docker Engine",
        docker
            .as_ref()
            .context("Docker Desktop не найден")
            .and_then(|p| {
                let info = docker_query(p, &["info", "--format", "{{json .}}"])?;
                let v: serde_json::Value = serde_json::from_str(&info)?;
                if v["OSType"] != "linux" {
                    bail!("Переключите Docker Desktop на Linux containers");
                }
                if !v["KernelVersion"]
                    .as_str()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("wsl2")
                {
                    bail!("Для Windows требуется backend WSL 2 в Docker Desktop");
                }
                Ok(format!(
                    "Linux · WSL 2 · {}",
                    v["ServerVersion"].as_str().unwrap_or("")
                ))
            }),
        true,
    ));
    checks.push(check(
        "Node.js",
        node_runtime_version(layout)
            .context("Нужен Node.js 22.18+ или 24.12+. Можно скачать portable runtime")
            .map(|(_, version)| version),
        true,
    ));
    checks.push(check(
        "Google Chrome",
        chrome()
            .map(|_| "Google Chrome найден".into())
            .context("Установите Google Chrome для Browser Agent"),
        true,
    ));
    let directory = browser_directory(layout)
        .unwrap_or_else(|| layout.data.join("browser-agent/missing-payload"));
    let entry = directory.join("dist/main.js");
    let module = directory.join("node_modules/playwright/package.json");
    checks.push(check(
        "Browser Agent",
        if entry.is_file() && module.is_file() && config::browser_resources_present(&directory) {
            Ok("Собранный агент и Playwright доступны".into())
        } else {
            Err(anyhow::anyhow!(
                "Подготовьте зависимости агента кнопкой ниже"
            ))
        },
        true,
    ));
    checks
}
pub(crate) fn boot_identity() -> Result<String> {
    process::powershell("$ErrorActionPreference='Stop'; (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')",&[],Duration::from_secs(15))
}
fn reboot_check(layout: &Layout) -> Result<String> {
    let current = boot_identity()?;
    reboot_check_for_boot(layout, current.trim())
}
fn reboot_check_for_boot(layout: &Layout, current: &str) -> Result<String> {
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(layout.data.join("reboot-required.json"))?)?;
    if record["boot"].as_str() == Some(current) {
        bail!("Компоненты WSL изменены. Перезагрузите Windows, затем повторите проверку");
    }
    Ok("Windows перезагружена после подготовки WSL".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observing_reboot_preserves_the_original_record() {
        let root = std::env::temp_dir().join(format!("hh-reboot-{}", uuid::Uuid::new_v4()));
        let layout = Layout::new(root.clone(), None);
        fs::create_dir_all(&layout.data).unwrap();
        let marker = layout.data.join("reboot-required.json");
        let original = br#"{"boot":"synthetic-boot-a"}"#;
        fs::write(&marker, original).unwrap();
        assert!(reboot_check_for_boot(&layout, "synthetic-boot-a").is_err());
        assert!(reboot_check_for_boot(&layout, "synthetic-boot-b").is_ok());
        assert_eq!(fs::read(marker).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }
}
