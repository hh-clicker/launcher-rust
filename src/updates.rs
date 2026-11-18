use anyhow::{Context, Result};
use serde_json::Value;
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub latest: String,
    pub critical: bool,
    pub available: bool,
    pub message: String,
}
fn version(s: &str) -> Option<[u32; 3]> {
    let n: Vec<u32> = s
        .trim_start_matches('v')
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    n.try_into().ok()
}
pub fn compare(current: &str, latest: &str, minimum: Option<&str>) -> Result<Update> {
    let current = version(current).context("Неизвестная текущая версия")?;
    let newest = version(latest).context("Неизвестный формат версии релиза")?;
    let critical = minimum
        .and_then(version)
        .is_some_and(|min| current < min && min <= newest);
    Ok(Update {
        latest: latest.trim_start_matches('v').into(),
        critical,
        available: newest > current,
        message: if newest > current {
            if critical {
                "Критическое обновление"
            } else {
                "Доступно обновление"
            }
        } else {
            "Новых релизов HH Clicker нет"
        }
        .into(),
    })
}
pub fn check() -> Result<Update> {
    let release = crate::auth::json(
        "https://api.github.com/repos/hh-clicker/.github/releases/latest",
        false,
        15,
    )?;
    let policy: Value = crate::auth::json(
        "https://raw.githubusercontent.com/hh-clicker/.github/master/update-policy.json",
        false,
        15,
    )
    .unwrap_or(Value::Null);
    compare(
        crate::config::APP_VERSION,
        release["tag_name"].as_str().context("Нет tag_name")?,
        policy["minimumSupportedVersion"].as_str(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_critical_and_numeric_versions() {
        assert!(compare("3.0.0", "v3.1.0", Some("2.0.0")).unwrap().available);
        assert!(!compare("3.0.0", "3.1.0", Some("2.0.0")).unwrap().critical);
        assert!(compare("3.0.0", "4.0.0", Some("3.1.0")).unwrap().critical);
        assert!(compare("3.9.0", "3.10.0", None).unwrap().available);
        assert!(compare("bad", "3.0.0", None).is_err());
    }
}
