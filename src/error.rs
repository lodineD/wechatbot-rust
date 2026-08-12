use ds_api::error::ApiError as DeepSeekError;
use std::fmt;
use wecom_aibot_rust_sdk::SdkError;

/// 应用统一错误类型。
#[derive(Debug)]
pub enum AppError {
    WeCom(SdkError),
    DeepSeek(DeepSeekError),
    Config(String),
    Internal(String),
    Other(anyhow::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::WeCom(e) => write!(f, "企业微信 SDK 错误: {e}"),
            AppError::DeepSeek(e) => write!(f, "DeepSeek API 错误: {e}"),
            AppError::Config(e) => write!(f, "配置错误: {e}"),
            AppError::Internal(e) => write!(f, "内部错误: {e}"),
            AppError::Other(e) => write!(f, "其他错误: {e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<SdkError> for AppError {
    fn from(e: SdkError) -> Self {
        AppError::WeCom(e)
    }
}

impl From<DeepSeekError> for AppError {
    fn from(e: DeepSeekError) -> Self {
        AppError::DeepSeek(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e)
    }
}

impl From<String> for AppError {
    fn from(e: String) -> Self {
        AppError::Config(e)
    }
}

impl From<&str> for AppError {
    fn from(e: &str) -> Self {
        AppError::Config(e.to_string())
    }
}
