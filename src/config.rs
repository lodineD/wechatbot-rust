use anyhow::{Context, Result};
use std::env;

/// 应用配置，全部从环境变量读取。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub wechat_bot_id: String,
    pub wechat_bot_secret: String,
    pub deepseek_api_key: String,
    pub deepseek_system_prompt: String,
    pub special_user_id: String,
    /// 定时推送日报的目标群聊 chatid（留空则不启用定时推送）。
    /// 是否启用小红书搜索。
    pub xhs_enabled: bool,
    /// 小红书 Cookie 字符串（从 xhs_cookie.txt 文件读取，避免 .env 解析问题）。
    pub xhs_cookie: String,
}

impl AppConfig {
    /// 从当前进程环境变量加载配置。
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            wechat_bot_id: env::var("WECHAT_BOT_ID").context("缺少环境变量 WECHAT_BOT_ID")?,
            wechat_bot_secret: env::var("WECHAT_BOT_SECRET")
                .context("缺少环境变量 WECHAT_BOT_SECRET")?,
            deepseek_api_key: env::var("DEEPSEEK_API_KEY")
                .context("缺少环境变量 DEEPSEEK_API_KEY")?,
            deepseek_system_prompt: env::var("DEEPSEEK_SYSTEM_PROMPT")
                .unwrap_or_else(|_| "你是一个 helpful 的 AI 助手".to_string()),
            special_user_id: env::var("SPECIAL_USER_ID")
                .unwrap_or_else(|_| "15771075163".to_string()),
            xhs_enabled: env::var("XHS_ENABLED")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            // 从独立文件读取 Cookie，避免塞进 .env 导致 dotenvy 解析失败
            xhs_cookie: load_xhs_cookie(),
        })
    }
}

/// 从 xhs_cookie.txt 文件读取小红书 Cookie（单行完整字符串）。
///
/// 不要放入 .env：Cookie 值含有大量 `;` 和 `{}` 等字符，
/// 会导致 dotenvy 解析报错，进而使整个 .env 无法加载。
fn load_xhs_cookie() -> String {
    // 候选路径：当前目录、可执行文件目录、项目根目录
    let mut candidates: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from("xhs_cookie.txt")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("xhs_cookie.txt"));
            candidates.push(exe_dir.join("..").join("..").join("xhs_cookie.txt"));
        }
    }

    for path in candidates {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let cookie = content.trim().to_string();
                if !cookie.is_empty() {
                    tracing::debug!("已从 {:?} 加载小红书 Cookie", path);
                    return cookie;
                }
            }
        }
    }

    // 兼容：如果环境变量里仍有 XHS_COOKIE（例如 Docker compose 注入），则读取
    env::var("XHS_COOKIE").unwrap_or_default()
}
