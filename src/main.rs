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

    // 尝试从多个位置加载 .env
    load_dotenv();

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

/// 尝试从多个候选路径加载 .env 文件。
fn load_dotenv() {
    let mut candidates: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from(".env")];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // cargo run / target/release/wechatbot-rust -> 项目根目录
            let project_root = exe_dir.join("..").join("..").join(".env");
            candidates.push(project_root.clone());
            // 也可能二进制和 .env 在同一目录
            candidates.push(exe_dir.join(".env"));
            // 使用规范化后的绝对路径，便于日志排查
            if let Ok(canonical) = project_root.canonicalize() {
                candidates.push(canonical);
            }
        }
    }

    // 去重，保留查找顺序
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|p| seen.insert(p.clone()));

    for path in candidates {
        if !path.exists() {
            continue;
        }
        match dotenvy::from_path(&path) {
            Ok(_) => {
                info!("已加载 .env 文件: {:?}", path);
                return;
            }
            Err(e) => warn!("加载 .env 文件失败: {:?}, err={}", path, e),
        }
    }

    warn!("未找到可用的 .env 文件，将完全依赖系统环境变量");
}
