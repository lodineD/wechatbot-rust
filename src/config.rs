use anyhow::{Context, Result};
use std::env;

/// 应用配置，全部从环境变量读取。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub wechat_bot_id: String,
    pub wechat_bot_secret: String,
    pub deepseek_api_key: String,
    pub deepseek_system_prompt: String,
}

impl AppConfig {
    /// 从当前进程环境变量加载配置。
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            wechat_bot_id: env::var("WECHAT_BOT_ID")
                .context("缺少环境变量 WECHAT_BOT_ID")?,
            wechat_bot_secret: env::var("WECHAT_BOT_SECRET")
                .context("缺少环境变量 WECHAT_BOT_SECRET")?,
            deepseek_api_key: env::var("DEEPSEEK_API_KEY")
                .context("缺少环境变量 DEEPSEEK_API_KEY")?,
            deepseek_system_prompt: env::var("DEEPSEEK_SYSTEM_PROMPT")
                .unwrap_or_else(|_| "你是一个 helpful 的 AI 助手".to_string()),
        })
    }
}
