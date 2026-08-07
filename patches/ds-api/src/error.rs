//! 统一的错误类型
//!
//! 该模块使用 `thiserror` 提供库内部统一的 `ApiError` 类型并导出通用 `Result<T>` 别名。
//! 目标：把库中散落的各种错误（例如 `Box<dyn Error>`、`reqwest::Error`、`serde_json::Error`）统一到一个基于 `thiserror` 的 `ApiError`，并导出便捷的 `Result<T>` 别名，方便 `?` 自动转换与更友好的错误展示。

use thiserror::Error;

/// 统一的错误类型，覆盖常见的错误来源并保留一个通用字符串变体用于快速转换。
#[derive(Error, Debug)]
pub enum ApiError {
    /// HTTP 层面的失败（当我们想保留状态码与响应文本时使用）
    #[error("HTTP error {status}: {text}")]
    Http {
        status: reqwest::StatusCode,
        text: String,
    },

    /// reqwest 网络/请求错误
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// serde_json 解析/序列化错误
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// EventSource / SSE 处理错误（来自 `eventsource-stream` crate）
    /// 以字符串形式保存错误信息（避免直接依赖具体 crate 的错误类型签名）
    #[error("EventSource error: {0}")]
    EventSource(String),

    /// IO 错误（保底）
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// 通用字符串错误（方便从 `String` / `&str` 直接转换）
    #[error("{0}")]
    Other(String),

    /// 未知或占位错误
    #[error("Unknown error")]
    Unknown,
}

/// 常用的 `Result` 别名，方便在库内统一返回类型。
pub type Result<T> = std::result::Result<T, ApiError>;

impl ApiError {
    /// 创建一个 `Http` 变体的快捷方法
    pub fn http_error(status: reqwest::StatusCode, text: impl Into<String>) -> Self {
        ApiError::Http {
            status,
            text: text.into(),
        }
    }
}

impl From<&str> for ApiError {
    fn from(s: &str) -> Self {
        ApiError::Other(s.to_string())
    }
}

impl From<String> for ApiError {
    fn from(s: String) -> Self {
        ApiError::Other(s)
    }
}
