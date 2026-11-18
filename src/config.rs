use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CODEX_VERSION: &str = "0.163.0";
pub const BROWSER_SELECTOR_FILES: [&str; 7] = [
    "profile.json",
    "common.json",
    "search.json",
    "vacancy.json",
    "response-form.json",
    "letter-form.json",
    "resume-text.json",
];
pub fn browser_resources_present(directory: &Path) -> bool {
    BROWSER_SELECTOR_FILES.iter().all(|name| {
        directory
            .join("resources/hh-selectors")
            .join(name)
            .is_file()
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: u32,
    pub instance: Uuid,
    pub backend_port: u16,
    pub frontend_port: u16,
    pub generation_port: u16,
    pub postgres_password: String,
    pub codex_version: String,
    pub codex_model: String,
    pub codex_effort: String,
}

#[derive(Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub data: PathBuf,
    pub payload: PathBuf,
}

impl Layout {
    pub fn browser_directory(&self) -> Result<PathBuf> {
        let manifest = fs::read(self.payload.join("release-manifest.json"))?;
        Ok(self
            .data
            .join("browser-agent")
            .join(payload_id(&manifest, None)))
    }
    pub fn new(root: PathBuf, data: Option<PathBuf>) -> Self {
        Self {
            data: data.unwrap_or_else(|| root.join("data")),
            payload: root.join("app/payload"),
            root,
        }
    }
    pub fn load(&self) -> Result<Config> {
        fs::create_dir_all(&self.data)?;
        let file = self.data.join("config.json");
        if file.exists() {
            let config: Config = serde_json::from_slice(&fs::read(&file)?)
                .context("Повреждён data/config.json; файл сохранён без изменений")?;
            config.validate()?;
            return Ok(config);
        }
        let mut ports = Vec::new();
        let config = Config {
            schema: 1,
            instance: Uuid::new_v4(),
            backend_port: choose_port(28080, &mut ports)?,
            frontend_port: choose_port(23000, &mut ports)?,
            generation_port: choose_port(27374, &mut ports)?,
            postgres_password: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            codex_version: CODEX_VERSION.into(),
            codex_model: "gpt-6-luna".into(),
            codex_effort: "low".into(),
        };
        self.save(&config)?;
        Ok(config)
    }
    pub fn save(&self, config: &Config) -> Result<()> {
        config.validate()?;
        atomic_write(
            &self.data.join("config.json"),
            &serde_json::to_vec_pretty(config)?,
        )
    }
    pub fn write_env(&self, config: &Config) -> Result<()> {
        let env = format!(
            "HH_CLICKER_INSTANCE_ID={}\nHH_CLICKER_VERSION={}\nHH_CLICKER_PROFILE=rust-portable\nPOSTGRES_DB=hh_clicker\nPOSTGRES_USER=hh_clicker\nPOSTGRES_PASSWORD={}\nDB_SCHEMA=hhclicker\nBACKEND_PORT={}\nFRONTEND_PORT={}\nGENERATION_PORT={}\nCODEX_CLI_VERSION={}\nCODEX_MODEL={}\nCODEX_REASONING_EFFORT={}\n",
            config.instance, APP_VERSION, config.postgres_password, config.backend_port, config.frontend_port,
            config.generation_port, config.codex_version, config.codex_model, config.codex_effort);
        atomic_write(&self.data.join("runtime.env"), env.as_bytes())
    }
    pub fn check_payload(&self) -> Result<()> {
        for path in [
            "infra/compose.yaml",
            "backend/Dockerfile",
            "frontend/Dockerfile",
            "generation-agent/Dockerfile",
            "browser-agent/package.json",
            "browser-agent/dist/main.js",
            "release-manifest.json",
        ] {
            if !self.payload.join(path).is_file() {
                bail!("В комплекте отсутствует {path}. Восстановите файлы app повторным запуском portable EXE");
            }
        }
        if !browser_resources_present(&self.payload.join("browser-agent")) {
            bail!("В комплекте отсутствуют resources/hh-selectors Browser Agent. Распакуйте полный portable ZIP");
        }
        Ok(())
    }
    pub fn verify_payload(&self) -> Result<()> {
        self.check_payload()?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(self.payload.join("release-manifest.json"))?)?;
        let files = manifest["files"]
            .as_object()
            .context("Нет контрольных сумм payload")?;
        if files.is_empty() {
            bail!("Пустой payload manifest");
        }
        for (name, expected) in files {
            let path = Path::new(name);
            if path.is_absolute()
                || path
                    .components()
                    .any(|p| !matches!(p, std::path::Component::Normal(_)))
                || name.contains('\\')
            {
                bail!("Небезопасный путь manifest");
            }
            let path = self.payload.join(path);
            let metadata =
                fs::symlink_metadata(&path).with_context(|| format!("Отсутствует файл {name}"))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                bail!("Неподдерживаемый тип файла payload: {name}");
            }
            let actual = format!("{:x}", Sha256::digest(fs::read(&path)?));
            if Some(actual.as_str()) != expected.as_str() {
                bail!("Файл приложения изменён: {name}. Распакуйте полный portable ZIP");
            }
        }
        Ok(())
    }
    pub fn project(&self, config: &Config, generation: bool) -> String {
        format!(
            "hh-rust-{}{}",
            config.instance.simple(),
            if generation { "-generation" } else { "" }
        )
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.instance.is_nil() {
            bail!("Неподдерживаемая конфигурация");
        }
        let ports = [self.backend_port, self.frontend_port, self.generation_port];
        if ports.iter().any(|p| *p < 1024)
            || ports[0] == ports[1]
            || ports[0] == ports[2]
            || ports[1] == ports[2]
        {
            bail!("Неверные или повторяющиеся порты");
        }
        if self.postgres_password.len() < 32
            || !self
                .postgres_password
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            bail!("Неверный пароль изолированной БД");
        }
        for value in [&self.codex_version, &self.codex_model, &self.codex_effort] {
            if value.is_empty()
                || value.len() > 100
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            {
                bail!("Неверное значение настроек Codex");
            }
        }
        Ok(())
    }
    pub fn url(&self, component: usize) -> String {
        format!(
            "http://127.0.0.1:{}",
            match component {
                1 => self.backend_port,
                2 => self.frontend_port,
                _ => self.generation_port,
            }
        )
    }
}

pub fn available(port: u16) -> bool {
    // A listening test socket triggers Windows Firewall notifications. Inspect
    // existing listeners instead; an unreadable table is never treated as free.
    crate::platform::port_available(port).unwrap_or(false)
}
pub fn choose_port(start: u16, claimed: &mut Vec<u16>) -> Result<u16> {
    for port in start..start.saturating_add(100) {
        if !claimed.contains(&port) && available(port) {
            claimed.push(port);
            return Ok(port);
        }
    }
    bail!("Нет свободного порта в диапазоне {start}–{}", start + 99)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    use std::io::Write;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    // Windows MoveFileEx replaces existing files atomically, unlike remove + rename.
    crate::platform::replace_file(&tmp, path)
}
pub fn payload_id(manifest: &[u8], codex: Option<&str>) -> String {
    let mut digest = Sha256::new();
    digest.update(manifest);
    if let Some(version) = codex {
        digest.update(b"\0CODEX_CLI_VERSION\0");
        digest.update(version.as_bytes());
    }
    format!("{:x}", digest.finalize())
}
pub fn node_compatible(version: &str) -> bool {
    let Ok(nums) = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
    else {
        return false;
    };
    if nums.len() != 3 {
        return false;
    }
    matches!(nums.as_slice(), [22, minor, ..] if *minor >= 18)
        || matches!(nums.as_slice(), [24, minor, ..] if *minor >= 12)
        || nums.first().is_some_and(|n| *n > 24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    #[test]
    fn node_contract() {
        for (v, ok) in [
            ("v22.18.0", true),
            ("22.17.9", false),
            ("23.9.0", false),
            ("24.11.0", false),
            ("24.12.0", true),
            ("25.0.0", true),
            ("broken", false),
            ("22.x.18", false),
            ("24.12", false),
        ] {
            assert_eq!(node_compatible(v), ok, "{v}");
        }
    }
    #[test]
    fn build_inputs_invalidate_marker() {
        assert_ne!(payload_id(b"one", None), payload_id(b"two", None));
        assert_ne!(payload_id(b"one", Some("1")), payload_id(b"one", Some("2")));
    }
    #[test]
    fn payload_integrity_and_browser_version_isolation() {
        let root = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let layout = Layout::new(root.clone(), None);
        fs::create_dir_all(&layout.payload).unwrap();
        let mut files = serde_json::Map::new();
        for name in [
            "infra/compose.yaml",
            "backend/Dockerfile",
            "frontend/Dockerfile",
            "generation-agent/Dockerfile",
            "browser-agent/package.json",
            "browser-agent/dist/main.js",
        ] {
            let path = layout.payload.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"synthetic").unwrap();
            files.insert(
                name.into(),
                serde_json::json!(format!("{:x}", Sha256::digest(b"synthetic"))),
            );
        }
        let manifest = layout.payload.join("release-manifest.json");
        for name in BROWSER_SELECTOR_FILES {
            let relative = format!("browser-agent/resources/hh-selectors/{name}");
            let path = layout.payload.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"{}").unwrap();
            files.insert(
                relative,
                serde_json::json!(format!("{:x}", Sha256::digest(b"{}"))),
            );
        }
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({"files":files})).unwrap(),
        )
        .unwrap();
        layout.verify_payload().unwrap();
        let required = layout
            .payload
            .join("browser-agent/resources/hh-selectors/profile.json");
        fs::remove_file(&required).unwrap();
        assert!(layout
            .verify_payload()
            .unwrap_err()
            .to_string()
            .contains("hh-selectors"));
        fs::write(&required, b"{}").unwrap();
        let old = layout.browser_directory().unwrap();
        fs::write(layout.payload.join("backend/Dockerfile"), b"modified").unwrap();
        assert!(layout.verify_payload().is_err());
        fs::write(&manifest, b"another payload").unwrap();
        assert_ne!(old, layout.browser_directory().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn occupied_port_is_skipped() {
        let listener = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let p = listener.local_addr().unwrap().port();
        assert!(!available(p));
        if p < 65400 {
            assert_ne!(choose_port(p, &mut vec![]).unwrap(), p);
        }
    }
    #[test]
    fn loopback_listener_is_a_port_conflict() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!available(port));
    }
    #[test]
    fn damaged_configuration_is_not_replaced() {
        let root = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let layout = Layout::new(root.clone(), None);
        fs::create_dir_all(&layout.data).unwrap();
        fs::write(layout.data.join("config.json"), "broken").unwrap();
        assert!(layout.load().is_err());
        assert_eq!(
            fs::read_to_string(layout.data.join("config.json")).unwrap(),
            "broken"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn durable_isolated_identity() {
        let root = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let layout = Layout::new(root.clone(), None);
        let a = layout.load().unwrap();
        let b = layout.load().unwrap();
        assert_eq!(a.instance, b.instance);
        assert!(layout.project(&a, false).starts_with("hh-rust-"));
        assert_ne!(layout.project(&a, false), layout.project(&a, true));
        fs::remove_dir_all(root).unwrap();
    }
}
