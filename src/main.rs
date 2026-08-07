mod config;
mod deepseek;
mod error;
mod wecom;

use anyhow::Result;
use config::AppConfig;
use deepseek::DeepseekClient;
use tracing::info;
use wecom::WecomBot;

#[tokio::main]
async fn main() -> Result<()> {
    // 加载 .env 环境变量
    dotenvy::dotenv().ok();

    // 初始化日志订阅器
    tracing_subscriber::fmt::init();

    info!("正在启动 wechatbot-rust...");

    // 加载配置
    let config = AppConfig::from_env()?;

    // 初始化 DeepSeek 客户端
    let deepseek = DeepseekClient::new(
        config.deepseek_api_key,
        config.deepseek_system_prompt,
    );

    // 初始化并运行企业微信机器人
    let bot = WecomBot::new(config.wechat_bot_id, config.wechat_bot_secret, deepseek);
    bot.run().await?;

    Ok(())
}
