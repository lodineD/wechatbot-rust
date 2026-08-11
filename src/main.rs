mod config;
mod deepseek;
mod error;
mod rss;
mod search;
mod wecom;

use anyhow::Result;
use config::AppConfig;
use deepseek::DeepseekClient;
use tracing::{info, warn};
use wecom::WecomBot;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志订阅器（先于 dotenv 加载，方便打印诊断信息）
    tracing_subscriber::fmt::init();

    // 显式加载 .env，并记录诊断信息
    let cwd = std::env::current_dir().ok();
    let env_path = std::path::Path::new(".env");
    match dotenvy::from_path(env_path) {
        Ok(_) => info!("已加载 .env 文件: cwd={:?}, path={:?}", cwd, env_path),
        Err(e) => warn!("加载 .env 文件失败: cwd={:?}, path={:?}, err={}", cwd, env_path, e),
    }

    info!("原始 env: DAILY_NEWS_CHAT_ID = {:?}", std::env::var("DAILY_NEWS_CHAT_ID"));

    info!("正在启动 wechatbot-rust...");

    // 加载配置
    let config = AppConfig::from_env()?;

    // 初始化 DeepSeek 客户端
    let deepseek = DeepseekClient::new(
        config.deepseek_api_key,
        config.deepseek_system_prompt,
    );

    // 初始化并运行企业微信机器人
    let chat_id = config.daily_news_chat_id.clone();
    info!("配置加载完成, DAILY_NEWS_CHAT_ID = {:?}", chat_id);

    let bot = WecomBot::new(config.wechat_bot_id, config.wechat_bot_secret, deepseek);
    bot.run(chat_id).await?;

    Ok(())
}
