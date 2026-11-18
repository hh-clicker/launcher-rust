use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Auth {
    pub signed_in: bool,
    pub pending: bool,
    pub code: Option<String>,
    pub url: Option<String>,
    pub message: String,
    pub details: String,
}
pub fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout(Duration::from_secs(timeout))
        .user_agent(concat!("HHClicker-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
}
pub fn json(url: &str, post: bool, timeout: u64) -> Result<Value> {
    let a = if url.starts_with("http://127.0.0.1:") {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(Duration::from_secs(timeout))
            .try_proxy_from_env(false)
            .redirects(0)
            .build()
    } else {
        agent(timeout)
    };
    let r = if post {
        a.post(url).set("Accept", "application/json").call()?
    } else {
        a.get(url).set("Accept", "application/json").call()?
    };
    let body: Value = serde_json::from_reader(r.into_reader().take(1_048_576))?;
    Ok(body)
}
use std::io::Read;
pub fn trusted_device_url(url: &str) -> bool {
    // Exact host and path. No userinfo, alternate port, fragment or deceptive suffix.
    url == "https://auth.openai.com/codex/device"
}
pub fn account(v: &Value) -> Result<Auth> {
    let signed = v["authenticated"]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("Некорректный ответ авторизации"))?;
    let mut details = Vec::new();
    for (key, label) in [
        ("primary", "Основной лимит"),
        ("secondary", "Дополнительный лимит"),
    ] {
        let w = &v["limits"][key];
        if let Some(used) = w["usedPercent"]
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        {
            let window = match w["windowDurationMins"].as_u64() {
                Some(300) => "5 часов".into(),
                Some(10080) => "Неделя".into(),
                Some(n) if n > 0 => format!("{n} мин."),
                _ => label.into(),
            };
            let reset = w["resetsAt"]
                .as_u64()
                .and_then(crate::platform::local_date)
                .map(|s| format!(" · сброс {s}"))
                .unwrap_or_default();
            details.push(format!("{window}: осталось {:.0}%{reset}", 100.0 - used));
        }
    }
    if let Some(t) = v["checkedAt"].as_str() {
        details.push(format!("Проверено: {t}"));
    }
    Ok(Auth {
        signed_in: signed,
        message: if signed {
            format!(
                "Codex подключён · {}",
                v["method"].as_str().unwrap_or("авторизован")
            )
        } else if v["error"]["code"]
            .as_str()
            .is_some_and(|s| s != "AUTH_REQUIRED")
        {
            "Не удалось проверить авторизацию. Проверьте соединение".into()
        } else {
            "Требуется вход через ChatGPT".into()
        },
        details: details.join("\n"),
        ..Default::default()
    })
}
pub fn login(v: &Value) -> Result<Auth> {
    let status = v["status"].as_str().unwrap_or("");
    let mut a = Auth::default();
    match status {
        "starting" => {
            a.pending = true;
            a.message = "Получаем код авторизации…".into();
        }
        "pending" => {
            let url = v["verificationUrl"]
                .as_str()
                .filter(|s| trusted_device_url(s));
            let code = v["userCode"].as_str().filter(|s| {
                (4..=32).contains(&s.len())
                    && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            });
            if url.is_none() || code.is_none() {
                bail!("Generation Agent вернул некорректный код авторизации");
            }
            a.pending = true;
            a.code = code.map(str::to_owned);
            a.url = url.map(str::to_owned);
            a.message = "Введите код на странице авторизации".into();
        }
        "succeeded" => {
            a.signed_in = true;
            a.message = "Codex подключён".into();
        }
        "idle" | "signed_out" => a.message = "Требуется вход через ChatGPT".into(),
        "failed" => {
            a.message = v["message"]
                .as_str()
                .unwrap_or("Авторизация не завершена")
                .chars()
                .take(500)
                .collect()
        }
        _ => bail!("Неожиданный ответ авторизации"),
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_auth_http_uses_real_methods_and_status() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for (method, path, body) in [
                ("GET", "/auth/status", r#"{"authenticated":false}"#),
                ("POST", "/auth/login/start", r#"{"status":"starting"}"#),
            ] {
                let (mut socket, _) = server.accept().unwrap();
                let mut bytes = [0u8; 4096];
                let n = socket.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                assert!(request.starts_with(&format!("{method} {path} HTTP/1.1")));
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        assert!(
            !account(&json(&format!("http://127.0.0.1:{port}/auth/status"), false, 3).unwrap())
                .unwrap()
                .signed_in
        );
        assert!(
            login(
                &json(
                    &format!("http://127.0.0.1:{port}/auth/login/start"),
                    true,
                    3
                )
                .unwrap()
            )
            .unwrap()
            .pending
        );
        handle.join().unwrap();
    }
    #[test]
    fn malicious_login_urls_rejected() {
        for u in [
            "http://auth.openai.com/codex/device",
            "https://auth.openai.com.evil/codex/device",
            "https://auth.openai.com@evil/codex/device",
            "file:///C:/file.exe",
        ] {
            assert!(!trusted_device_url(u));
        }
    }
    #[test]
    fn credentials_do_not_prove_auth() {
        assert!(account(&serde_json::json!({"authFileExists":true})).is_err());
        assert!(
            !account(&serde_json::json!({"authenticated":false}))
                .unwrap()
                .signed_in
        );
    }
    #[test]
    fn login_validation() {
        assert!(login(&serde_json::json!({"status":"pending","verificationUrl":"https://auth.openai.com/codex/device","userCode":"ABCD-1234"})).unwrap().pending);
        assert!(login(&serde_json::json!({"status":"pending","verificationUrl":"https://evil","userCode":"ABCD-1234"})).is_err());
    }
}
