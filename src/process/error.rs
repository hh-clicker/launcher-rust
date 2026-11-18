//! Machine-readable command failures, independent of UI presentation.
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum CommandError {
    Cancelled,
    Timeout { seconds: u64 },
    Exit { code: i32, message: String },
    OutputLimit,
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => {
                f.write_str("Операция отменена. Созданные ресурсы будут проверены перед остановкой")
            }
            Self::Timeout { seconds } => write!(f, "Время ожидания команды истекло ({seconds} с)"),
            Self::Exit { code, message } => {
                write!(f, "Команда завершилась с кодом {code}: {message}")
            }
            Self::OutputLimit => f.write_str("Ответ команды превышает безопасный предел 16 МиБ"),
        }
    }
}
impl Error for CommandError {}
