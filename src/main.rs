mod config;
mod deepseek;
mod error;
mod rss;
mod search;
mod wecom;
mod xiaohongshu;

#[cfg(all(unix, feature = "xhs-camoufox"))]
mod camoufox_backend;

use anyhow::Result;
use config::AppConfig;
use deepseek::DeepseekClient;
use tracing::{error, info, warn};
use wecom::{ReminderScene, WecomBot};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志订阅器（先于 dotenv 加载，方便打印诊断信息）
    tracing_subscriber::fmt::init();

    // 尝试从多个位置加载 .env
    load_dotenv();

    // 支持 --xhs-probe <关键词> 直接触发一次小红书搜索（用于验证 camoufox 全链路）
    let args: Vec<String> = std::env::args().collect();
    let load_cookie = || {
        std::fs::read_to_string("xhs_cookie.txt")
            .or_else(|_| {
                std::env::var("XHS_COOKIE")
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "无 XHS_COOKIE"))
            })
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // --xhs-detail-probe <URL> 用 HTTP 直连获取小红书笔记详情（走 xhs_note_detail 链路）
    if let Some(pos) = args.iter().position(|a| a == "--xhs-detail-probe") {
        if let Some(url) = args.get(pos + 1) {
            info!("XHS 详情探测：{url}");
            let cookie = load_cookie();
            let result = xiaohongshu::xhs_note_detail(url, &cookie).await;
            println!("===== 小红书笔记详情 =====\n{result}\n============================");
            return Ok(());
        }
    }
    // 支持 --xhs-probe <关键词> 直接触发一次小红书搜索（用于验证 camoufox 全链路）
    if let Some(pos) = args.iter().position(|a| a == "--xhs-probe") {
        if let Some(q) = args.get(pos + 1) {
            info!("XHS 探测模式：搜索「{q}」");
            let cookie = load_cookie();
            let result = xiaohongshu::xhs_search(q, 5, &cookie).await;
            println!("===== 小红书搜索结果 =====\n{result}\n============================");
            return Ok(());
        }
    }

    // --send-test <userid> 向指定用户发送测试消息（验证单聊推送）
    if let Some(pos) = args.iter().position(|a| a == "--test-reminder") {
        let scene = match args.get(pos + 1).map(String::as_str) {
            Some("morning") => ReminderScene::Morning,
            Some("lunch") => ReminderScene::Lunch,
            Some("dinner") => ReminderScene::Dinner,
            _ => anyhow::bail!("用法: --test-reminder <morning|lunch|dinner>"),
        };
        let config = AppConfig::from_env()?;
        let target_user_id = config.special_user_id.clone();
        let deepseek = DeepseekClient::new(config.deepseek_api_key, config.deepseek_system_prompt)
            .with_special_user(target_user_id.clone());
        let bot = WecomBot::new(config.wechat_bot_id, config.wechat_bot_secret, deepseek);
        let message = bot.test_reminder(&target_user_id, scene).await?;
        println!("===== 定时提醒测试已发送 =====\n{message}");
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--test-rss") {
        let config = AppConfig::from_env()?;
        let target_user_id = config.special_user_id.clone();
        let deepseek = DeepseekClient::new(config.deepseek_api_key, config.deepseek_system_prompt)
            .with_special_user(target_user_id.clone());
        let bot = WecomBot::new(config.wechat_bot_id, config.wechat_bot_secret, deepseek);
        bot.test_rss(&target_user_id).await?;
        println!("===== RSS 单聊测试已发送 =====");
        return Ok(());
    }

    if let Some(pos) = args.iter().position(|a| a == "--send") {
        let userid = args.get(pos + 1).filter(|v| !v.trim().is_empty());
        let message = args.get(pos + 2).filter(|v| !v.is_empty());
        match (userid, message) {
            (Some(userid), Some(message)) => {
                send_one_message(userid, message).await?;
                return Ok(());
            }
            _ => anyhow::bail!("用法: --send <userid> <message>"),
        }
    }

    if let Some(pos) = args.iter().position(|a| a == "--send-test") {
        if let Some(userid) = args.get(pos + 1) {
            info!("发送测试模式：向 {userid} 发送 '你好'");
            let config = AppConfig::from_env()?;
            let client =
                wecom_aibot_rust_sdk::WSClient::new(wecom_aibot_rust_sdk::WSClientOptions::new(
                    config.wechat_bot_id,
                    config.wechat_bot_secret,
                ));

            // 连接 WebSocket
            client.connect().await?;
            info!("已连接到企业微信");

            // 发送测试消息（主动推送必须用 markdown，text 会报 40008）
            let body = serde_json::json!({
                "msgtype": "markdown",
                "markdown": {
                    "content": "你好！这是一条测试消息 🎉"
                }
            });

            match client.send_message(userid, body).await {
                Ok(_) => {
                    info!("✓ 消息发送成功");
                    println!("===== 测试消息已发送给 {userid} =====");
                }
                Err(e) => {
                    error!("✗ 发送失败: {e}");
                    eprintln!("===== 发送失败 =====\n错误: {e}");
                }
            }

            // 断开连接
            client.disconnect_async().await;
            return Ok(());
        }
    }

    info!(
        "原始 env: DAILY_NEWS_CHAT_ID = {:?}",
        std::env::var("DAILY_NEWS_CHAT_ID")
    );

    info!("正在启动 wechatbot-rust...");

    // 加载配置
    let config = AppConfig::from_env()?;

    // 初始化 DeepSeek 客户端
    let deepseek = DeepseekClient::new(config.deepseek_api_key, config.deepseek_system_prompt)
        .with_xhs(config.xhs_enabled, config.xhs_cookie.clone())
        .with_special_user(config.special_user_id.clone());

    // 初始化并运行企业微信机器人
    let target_user_id = config.special_user_id.clone();
    let xhs_on = config.xhs_enabled;
    info!(
        "配置加载完成, DAILY_NEWS_CHAT_ID = {:?}, XHS_ENABLED = {xhs_on}",
        target_user_id
    );

    let bot = WecomBot::new(config.wechat_bot_id, config.wechat_bot_secret, deepseek);
    bot.run(target_user_id).await?;

    Ok(())
}

async fn send_one_message(userid: &str, message: &str) -> Result<()> {
    info!("主动发送消息给 {userid}");
    let config = AppConfig::from_env()?;
    let client = wecom_aibot_rust_sdk::WSClient::new(wecom_aibot_rust_sdk::WSClientOptions::new(
        config.wechat_bot_id,
        config.wechat_bot_secret,
    ));
    client.connect().await?;
    let body = serde_json::json!({
        "msgtype": "markdown",
        "markdown": { "content": message }
    });
    let result = client.send_message(userid, body).await;
    client.disconnect_async().await;
    result?;
    println!("===== 消息已发送给 {userid} =====");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dotenv_loads_without_error() {
        // 初始化日志，确保能打印诊断信息
        let _ = tracing_subscriber::fmt::try_init();
        load_dotenv();

        // 只要能执行到这里，说明 .env 没有解析错误。
        // 进一步验证关键环境变量已加载。
        assert!(
            std::env::var("WECHAT_BOT_ID").is_ok() || std::env::var("WECHAT_BOT_SECRET").is_ok(),
            "至少应能读到部分环境变量"
        );
    }
}
