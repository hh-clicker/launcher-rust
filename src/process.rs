//! Bounded command output, cancellation, timeouts and owned child processes.
mod error;
use crate::diagnostics::{LogWriter, Redactor};
pub use error::CommandError;

use crate::platform::ChildJob;
use anyhow::{Context, Result};
use std::os::windows::process::CommandExt;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub const NO_WINDOW: u32 = 0x08000000;
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(NO_WINDOW);
    c.stdin(Stdio::null());
    c
}
/// Docker invokes credential helpers by name, including immediately after installation.
/// An already-running launcher has the old PATH, so repair only the child's environment.
pub fn docker_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = command(&program);
    if let Some(directory) = Path::new(program.as_ref())
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        // Windows paths cannot contain ';'; preserve the inherited PATH byte-for-byte.
        let mut path = directory.as_os_str().to_os_string();
        path.push(";");
        path.push(inherited);
        c.env("PATH", path);
    }
    c
}
pub fn clean_compose_env(c: &mut Command) {
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("COMPOSE_")
            || name.starts_with("HH_CLICKER_")
            || name.starts_with("POSTGRES_")
            || name.starts_with("CODEX_")
            || name.starts_with("GENERATION_")
            || [
                "BACKEND_PORT",
                "FRONTEND_PORT",
                "DB_SCHEMA",
                "DB_URL",
                "DB_USERNAME",
                "DB_PASSWORD",
            ]
            .contains(&name.as_str())
        {
            c.env_remove(key);
        }
    }
}
/// Internal metadata, removed before spawn and never passed as command arguments.
pub fn redact_output(command: &mut Command, values: &[String]) {
    command.env(
        "HH_RUST_REDACT_VALUES",
        serde_json::to_string(values).unwrap(),
    );
}
pub fn run(cmd: Command, timeout: Duration) -> Result<String> {
    run_inner(cmd, timeout, None, true)
}
pub fn run_cancellable(cmd: Command, timeout: Duration, cancel: &AtomicBool) -> Result<String> {
    run_inner(cmd, timeout, Some(cancel), true)
}
/// A Desktop launcher intentionally starts an independently owned application.
/// Do not put its descendants in our kill-on-close job, even for --detach.
pub fn run_desktop_launcher(
    cmd: Command,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String> {
    run_inner(cmd, timeout, Some(cancel), false)
}
fn run_inner(
    mut cmd: Command,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
    own_tree: bool,
) -> Result<String> {
    if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
        return Err(CommandError::Cancelled.into());
    }
    let mut secrets = Vec::new();
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            let name = key.to_string_lossy().to_ascii_lowercase();
            if name == "hh_rust_redact_values" {
                secrets.extend(
                    serde_json::from_str::<Vec<String>>(&value.to_string_lossy())
                        .unwrap_or_default(),
                );
            } else if ["password", "token", "secret", "api_key"]
                .iter()
                .any(|key| name.contains(key))
            {
                secrets.push(value.to_string_lossy().into_owned());
            }
        }
    }
    cmd.env_remove("HH_RUST_REDACT_VALUES");
    let redactor = Redactor::new(secrets);
    let log_path = cmd
        .get_envs()
        .find(|(key, _)| *key == "HH_RUST_COMMAND_LOG")
        .and_then(|(_, v)| v.map(PathBuf::from));
    let log = if let Some(path) = log_path {
        std::fs::create_dir_all(path.parent().context("log directory")?)?;
        Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        )
    } else {
        None
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let job = if own_tree {
        Some(ChildJob::new()?)
    } else {
        None
    };
    let mut child = cmd.spawn().context("Не удалось запустить команду")?;
    if let Err(e) = job.as_ref().map(|job| job.assign(&child)).transpose() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.context("Не удалось изолировать дочернюю команду"));
    }
    let (tx, rx) = mpsc::channel();
    for (kind, stream) in [
        Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
        Box::new(child.stderr.take().unwrap()),
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        let mut log = log
            .as_ref()
            .and_then(|f| f.try_clone().ok())
            .map(|file| LogWriter::new(file, redactor.clone(), false));
        thread::spawn(move || {
            let mut stream = stream;
            let mut tail = Vec::new();
            let mut truncated = false;
            let mut buf = [0; 4096];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if kind == 0 {
                    let remaining = (16 * 1024 * 1024usize).saturating_sub(tail.len());
                    tail.extend_from_slice(&buf[..n.min(remaining)]);
                    if n > remaining {
                        truncated = true;
                    }
                } else {
                    tail.extend_from_slice(&buf[..n]);
                    if tail.len() > 32768 {
                        tail.drain(..tail.len() - 32768);
                    }
                }
                if let Some(log) = log.as_mut() {
                    let _ = log.write_all(&buf[..n]);
                }
            }
            if let Some(log) = log.as_mut() {
                let _ = log.finish();
            }
            let _ = tx.send((kind, tail, truncated));
        });
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandError::Timeout {
                seconds: timeout.as_secs(),
            }
            .into());
        }
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandError::Cancelled.into());
        }
        thread::sleep(Duration::from_millis(40));
    };
    // Release descendants holding pipes before waiting for readers.
    drop(job);
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut truncated = false;
    for _ in 0..2 {
        if let Ok((kind, bytes, cut)) = rx.recv_timeout(Duration::from_secs(2)) {
            if kind == 0 {
                out = bytes;
                truncated = cut;
            } else {
                errors = bytes;
            }
        }
    }
    let text = decode_output(&out);
    if !status.success() {
        let diagnostics = decode_output(&errors);
        let diagnostics = if diagnostics.trim().is_empty() {
            &text
        } else {
            &diagnostics
        };
        return Err(CommandError::Exit {
            code: status.code().unwrap_or(-1),
            message: redactor.text(
                diagnostics
                    .lines()
                    .rev()
                    .find(|s| !s.trim().is_empty())
                    .unwrap_or("без сообщения"),
            ),
        }
        .into());
    }
    if truncated {
        return Err(CommandError::OutputLimit.into());
    }
    Ok(text.trim().to_owned())
}
pub fn decode_output(bytes: &[u8]) -> String {
    if bytes.starts_with(&[255, 254])
        || (!bytes.is_empty()
            && bytes.iter().take(80).filter(|b| **b == 0).count() > bytes.len().min(80) / 4)
    {
        String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|s| u16::from_le_bytes([s[0], s[1]]))
                .filter(|c| *c != 0xfeff)
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
pub fn query(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Result<String> {
    let mut c = command(program);
    c.args(args);
    run(c, Duration::from_secs(12))
}
pub fn powershell(
    script: &str,
    vars: &[(&str, &std::ffi::OsStr)],
    timeout: Duration,
) -> Result<String> {
    run(powershell_command(script, vars), timeout)
}
fn powershell_command(script: &str, vars: &[(&str, &std::ffi::OsStr)]) -> Command {
    let mut c = command("powershell.exe");
    let script = format!("[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); {script}");
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-OutputFormat",
        "Text",
        "-EncodedCommand",
        &encoded,
    ]);
    for (k, v) in vars {
        c.env(k, v);
    }
    c
}
pub fn find_executable(name: &str, extras: &[PathBuf]) -> Option<PathBuf> {
    extras
        .iter()
        .cloned()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join(name)),
        )
        .find(|p| p.is_file())
}

pub struct ManagedChild {
    child: Child,
    _job: ChildJob,
    pub agent_id: String,
}
impl ManagedChild {
    pub fn spawn(
        program: &Path,
        entry: &Path,
        cwd: &Path,
        env: BTreeMap<String, String>,
        agent_id: String,
    ) -> Result<Self> {
        let log_dir = cwd.join("logs");
        std::fs::create_dir_all(&log_dir)?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("browser-agent.log"))?;
        let mut c = command(program);
        c.arg(entry)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let job = ChildJob::new()?;
        let mut child = c.spawn()?;
        if let Err(e) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        for reader in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let file = log.try_clone()?;
            thread::spawn(move || {
                crate::diagnostics::copy_log(reader, file, Redactor::default(), true)
            });
        }
        Ok(Self {
            child,
            _job: job,
            agent_id,
        })
    }
    pub fn alive(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|s| s.is_none())
    }
    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"shutdown\n");
        }
        let start = Instant::now();
        while self.alive() && start.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(100));
        }
        if self.alive() {
            self.child.kill()?;
        }
        self.child.wait()?;
        Ok(())
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_launcher_preserves_its_detached_child() {
        let root = std::env::temp_dir().join(format!("hh-detached-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("alive");
        let script = r#"$script='Start-Sleep -Milliseconds 650; [IO.File]::WriteAllText($env:HH_TEST_MARKER,''alive'')'; $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script)); $p=New-Object Diagnostics.ProcessStartInfo; $p.FileName=(Get-Process -Id $PID).Path; $p.Arguments='-NoProfile -NonInteractive -EncodedCommand '+$encoded; $p.UseShellExecute=$false; $p.CreateNoWindow=$true; $p.RedirectStandardOutput=$true; $p.RedirectStandardError=$true; [void][Diagnostics.Process]::Start($p)"#;
        // Reproduce the old behavior: a successful launcher command killed its child.
        run(
            powershell_command(script, &[("HH_TEST_MARKER", marker.as_os_str())]),
            Duration::from_secs(10),
        )
        .unwrap();
        thread::sleep(Duration::from_millis(1000));
        assert!(!marker.exists(), "Fixture did not reproduce kill-on-close");
        run_desktop_launcher(
            powershell_command(script, &[("HH_TEST_MARKER", marker.as_os_str())]),
            Duration::from_secs(10),
            &AtomicBool::new(false),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(std::fs::read(&marker).unwrap(), b"alive");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn docker_child_finds_helper_without_parent_path_refresh() {
        let root =
            std::env::temp_dir().join(format!("Docker с пробелами {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let docker = root.join("docker.exe");
        let helper = root.join("docker-credential-desktop.exe");
        std::fs::write(&helper, b"lookup only; never executed").unwrap();
        let inherited = std::env::var_os("PATH");
        let c = docker_command(&docker);
        let child_path = c
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        // Exercise helper lookup in a real child with precisely Docker's environment;
        // never invoke a credential helper or contact the engine.
        let output = powershell("[Console]::Out.Write((Get-Command docker-credential-desktop.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source)", &[("PATH", child_path)], Duration::from_secs(10)).unwrap();
        assert!(output
            .to_lowercase()
            .contains(&helper.to_string_lossy().to_lowercase()));
        assert_eq!(std::env::var_os("PATH"), inherited);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn powershell_preserves_quotes_unicode_and_installer_arguments() {
        let path = PathBuf::from("D:\\Каталог с пробелами\\GoogleChromeEnterprise64.msi");
        let script =
            r#"$a='/i "'+$env:HH_RUST_PACKAGE+'" /qn /norestart'; [Console]::Out.Write($a)"#;
        let output = powershell(
            script,
            &[("HH_RUST_PACKAGE", path.as_os_str())],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(output, format!("/i \"{}\" /qn /norestart", path.display()));
    }
    #[test]
    fn structured_stdout_is_complete_and_stderr_is_separate() {
        let result=powershell("[Console]::Out.Write('{\"padding\":\"'+('x'*50000)+'\"}');[Console]::Error.WriteLine('a warning on stderr')",&[],Duration::from_secs(10)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["padding"].as_str().unwrap().len(), 50000);
        assert!(!result.contains("warning"));
    }
    #[test]
    fn cancellation_terminates_only_its_command() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let waiter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            trigger.store(true, Ordering::Release);
        });
        let mut c = command("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        let started = Instant::now();
        assert!(run_cancellable(c, Duration::from_secs(10), &cancel)
            .unwrap_err()
            .to_string()
            .contains("отменена"));
        assert!(started.elapsed() < Duration::from_secs(3));
        waiter.join().unwrap();
    }
    #[test]
    fn utf16_wsl_output() {
        let bytes: Vec<u8> = "WSL 2.6.0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_output(&bytes), "WSL 2.6.0");
    }
    #[test]
    fn bounded_process_timeout() {
        let mut c = command("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 10",
        ]);
        assert!(run(c, Duration::from_millis(100))
            .unwrap_err()
            .to_string()
            .contains("истекло"));
    }
}
