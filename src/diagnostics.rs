//! Redaction at persistence boundaries. Never change application requests or user data.
use std::io::{Read, Write};

#[derive(Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}
impl Redactor {
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        Self {
            secrets: values.into_iter().filter(|s| !s.is_empty()).collect(),
        }
    }
    pub fn text(&self, value: &str) -> String {
        let mut text = value.to_owned();
        for secret in &self.secrets {
            text = text.replace(secret, "[hidden]");
        }
        // npm/proxy errors can include URL userinfo without a named secret field.
        for scheme in ["https://", "http://"] {
            let mut from = 0;
            while let Some(relative) = text[from..].find(scheme) {
                let start = from + relative + scheme.len();
                let end = text[start..]
                    .find(|c: char| c == '/' || c.is_whitespace())
                    .map_or(text.len(), |n| start + n);
                if let Some(at) = text[start..end].rfind('@') {
                    text.replace_range(start..start + at, "[hidden]");
                }
                from = start;
            }
        }
        // Keep the context preceding a sensitive field, never its value or tail.
        let lower = text.to_ascii_lowercase();
        let keys = [
            "authorization:",
            "bearer ",
            "access_token",
            "refresh_token",
            "id_token",
            "api_key",
            "apikey",
            "token=",
            "token:",
            "_authtoken",
            "secret=",
            "password=",
            "password:",
            "password\":",
            "device_code",
            "user_code",
            "credentials",
            "plaintext\":",
            "resume_text",
            "letter_text",
            "prompt\":",
        ];
        if let Some(at) = keys.iter().filter_map(|key| lower.find(key)).min() {
            text.truncate(at);
            text.push_str("[sensitive field hidden]");
        }
        text
    }
}

/// Logs from Browser Agent may contain applicant text in arbitrary error messages.
/// Persist lifecycle events and recognizable failure codes, not raw messages.
fn browser_event(line: &str) -> String {
    let line = line.trim();
    if line == "[browser-agent] Started" {
        return line.into();
    }
    let event = if line.contains("shutting down") {
        "shutdown"
    } else if line.contains("Executing command") {
        "command started"
    } else if line.contains("failed") || line.contains("Failed") || line.contains("unavailable") {
        "operation failed"
    } else {
        "diagnostic event"
    };
    let codes: Vec<_> = [
        "ENOENT",
        "EACCES",
        "ECONNREFUSED",
        "ECONNRESET",
        "ETIMEDOUT",
        "ENOTFOUND",
        "TimeoutError",
        "HTTP 400",
        "HTTP 401",
        "HTTP 403",
        "HTTP 404",
        "HTTP 409",
        "HTTP 429",
        "HTTP 500",
        "HTTP 502",
        "HTTP 503",
    ]
    .into_iter()
    .filter(|code| line.contains(code))
    .collect();
    format!("[browser-agent] {event} {}", codes.join(", "))
}

/// Bounded line reads avoid leaking secrets split across OS pipe chunks.
pub(crate) fn copy_log(
    mut reader: impl Read,
    writer: impl Write,
    redactor: Redactor,
    browser: bool,
) {
    let mut sink = LogWriter::new(writer, redactor, browser);
    let _ = std::io::copy(&mut reader, &mut sink);
    let _ = sink.finish();
}

pub(crate) struct LogWriter<W: Write> {
    writer: W,
    redactor: Redactor,
    browser: bool,
    line: Vec<u8>,
    too_long: bool,
}
impl<W: Write> LogWriter<W> {
    pub(crate) fn new(writer: W, redactor: Redactor, browser: bool) -> Self {
        Self {
            writer,
            redactor,
            browser,
            line: Vec::new(),
            too_long: false,
        }
    }
    pub(crate) fn finish(&mut self) -> std::io::Result<()> {
        if self.line.is_empty() && !self.too_long {
            return Ok(());
        }
        let text = if self.too_long {
            "[oversized diagnostic line omitted]".into()
        } else if self.browser {
            browser_event(&String::from_utf8_lossy(&self.line))
        } else {
            self.redactor
                .text(&crate::process::decode_output(&self.line))
        };
        self.line.clear();
        self.too_long = false;
        writeln!(self.writer, "{}", text.trim_end())
    }
}
impl<W: Write> Write for LogWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if !self.too_long && self.line.len() + part.len() <= 65536 {
                self.line.extend_from_slice(part);
            } else {
                self.too_long = true;
            }
            if part.ends_with(b"\n") {
                self.finish()?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_redaction_handles_chunk_boundaries_and_sensitive_tails() {
        let mut output = Vec::new();
        {
            let mut sink = LogWriter::new(&mut output, Redactor::default(), false);
            sink.write_all(b"pull failed: pass").unwrap();
            sink.write_all(b"word=sec").unwrap();
            sink.write_all(b"ret\nuseful ENOENT\n").unwrap();
            sink.finish().unwrap();
        }
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("secret"));
        assert!(output.contains("pull failed"));
        assert!(output.contains("ENOENT"));
        assert!(!Redactor::new(["sample-device-code".into()])
            .text("failed sample-device-code")
            .contains("sample-device-code"));
        assert_eq!(
            Redactor::default().text("fetch https://sample:private@registry.example/pkg"),
            "fetch https://[hidden]@registry.example/pkg"
        );
    }
    #[test]
    fn browser_errors_keep_codes_without_applicant_text() {
        let mut output = Vec::new();
        copy_log(
            &b"[browser-agent] Command sample failed: ENOENT personal resume text\n"[..],
            &mut output,
            Redactor::default(),
            true,
        );
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("ENOENT"));
        assert!(!text.contains("personal"));
    }
}
