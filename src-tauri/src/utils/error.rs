// src/utils/error.rs
use anyhow::Error as AnyhowError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Regex(#[from] regex::Error),
    #[error("failed to parse as string: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Anyhow(#[from] AnyhowError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error("{0}")]
    Utils(Box<Error>),
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "message")]
#[serde(rename_all = "camelCase")]
enum ErrorKind {
    Io(String),
    Regex(String),
    Utf8(String),
    Msg(String),
    Anyhow(String),
    Json(String),
    Join(String),
    Utils(String),
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let error_message = self.to_string();
        let error_kind = match self {
            Self::Io(_) => ErrorKind::Io(error_message),
            Self::Regex(_) => ErrorKind::Regex(error_message),
            Self::Utf8(_) => ErrorKind::Utf8(error_message),
            Self::Msg(_) => ErrorKind::Msg(error_message),
            Self::Anyhow(_) => ErrorKind::Anyhow(error_message),
            Self::Json(_) => ErrorKind::Json(error_message),
            Self::Join(_) => ErrorKind::Join(error_message),
            Self::Utils(_) => ErrorKind::Utils(error_message),
        };
        error_kind.serialize(serializer)
    }
}

impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Error::Msg(s.to_string())
    }
}

#[macro_export]
macro_rules! err {
    ($msg:literal $(,)?) => {
        $crate::utils::error::Error::Msg($msg.to_string())
    };
    ($err_expr:expr $(,)?) => {
        $crate::utils::error::Error::Msg($err_expr.into())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::utils::error::Error::Msg(format!($fmt, $($arg)*))
    };
}

#[macro_export]
macro_rules! ensure_some {
    ($option_expr:expr, $fmt_str:literal $(, $($args:tt)*)?) => {
        match $option_expr {
            Some(val) => ::core::result::Result::Ok(val),
            None => ::core::result::Result::Err(err!($fmt_str $(, $($args)*)?)),
        }
    };
}