//! camoufox backend for XHS search (Unix only).
//!
//! Uses camoufox-rs (anti-fingerprint Firefox) to bypass XHS's bot detection.

use camoufox::api::{Browser, BrowserOptions, ContextOptions, CookieOptions};
use camoufox::config::LaunchConfig;
use camoufox::process;
use camoufox::protocol::client::Connection;
use camoufox::transport::pipe::PipeTransport;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

use crate::xiaohongshu::{self, UNWRAP_JS, XhsSearchItem};

/// Search XHS using camoufox.
pub fn search_via_camoufox(
    search_url: &str,
    cookie: &str,
    max_results: usize,
) -> Result<Vec<XhsSearchItem>, String> {
    let start = std::time::Instant::now();
    info!("[XHS/camoufox] 开始搜索流程");

    // Launch camoufox process
    let camoufox_bin = std::env::var("CAMOUFOX_BIN")
        .ok()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/camoufox"));

    if !camoufox_bin.exists() {
        return Err(format!(
            "camoufox 二进制未找到: {}（请设置 CAMOUFOX_BIN 环境变量）",
            camoufox_bin.display()
        ));
    }
    info!("[XHS/camoufox] 使用二进制: {}", camoufox_bin.display());

    let profile_dir = std::env::temp_dir().join(format!("camoufox-xhs-{}", std::process::id()));
    std::fs::create_dir_all(&profile_dir).map_err(|e| format!("创建 profile 目录失败: {e}"))?;

    let config = LaunchConfig {
        executable: camoufox_bin,
        profile_dir: Some(profile_dir.clone()),
        headless: true,
        ..Default::default()
    };

    info!("[XHS/camoufox] 启动 camoufox 进程");
    let mut launched =
        process::unix::spawn(&config).map_err(|e| format!("启动 camoufox 失败: {e}"))?;

    process::readiness::wait_for_ready(&mut launched.child, config.timeout)
        .map_err(|e| format!("等待 camoufox 就绪失败: {e}"))?;

    info!(
        "[XHS/camoufox] camoufox 已就绪 ({}ms)",
        start.elapsed().as_millis()
    );

    // Connect to camoufox via Juggler protocol
    let transport = PipeTransport::new(launched.command_pipe, launched.response_pipe);
    let conn = Connection::new(Box::new(transport));
    let root = conn.root_session();

    let browser = Browser::connect(conn, root, BrowserOptions::default())
        .map_err(|e| format!("连接 camoufox 失败: {e}"))?;

    info!(
        "[XHS/camoufox] 浏览器连接成功 ({}ms)",
        start.elapsed().as_millis()
    );

    // Create browser context
    let context = browser
        .new_context(ContextOptions::default())
        .map_err(|e| format!("创建 context 失败: {e}"))?;

    // Inject cookies
    let cookies = crate::xiaohongshu::parse_cookie_string(cookie);
    if !cookies.is_empty() {
        // cookie_names 用 owned String，避免 borrow 与 into_iter move 冲突
        let cookie_names: Vec<String> = cookies.iter().map(|(n, _)| n.clone()).collect();
        let cookie_options: Vec<CookieOptions> = cookies
            .into_iter()
            .map(|(name, value)| CookieOptions {
                name,
                value,
                domain: Some(crate::xiaohongshu::XHS_DOMAIN.to_string()),
                path: Some("/".to_string()),
                secure: Some(true),
                http_only: None,
                expires: None,
                url: None,
                same_site: None,
            })
            .collect();

        context
            .set_cookies(&cookie_options)
            .map_err(|e| format!("设置 Cookie 失败: {e}"))?;

        info!(
            "[XHS/camoufox] Cookie 注入: {} 个 [{}]",
            cookie_options.len(),
            cookie_names.join(", ")
        );
    }

    // Create page (MainFrame)
    let main_frame = context
        .new_main_frame()
        .map_err(|e| format!("创建页面失败: {e}"))?;

    info!(
        "[XHS/camoufox] 页面已创建 ({}ms)",
        start.elapsed().as_millis()
    );

    // Navigate to XHS homepage first (establish domain context)
    info!("[XHS/camoufox] 导航到首页建立会话上下文");
    let _ = main_frame.navigate(
        "https://www.xiaohongshu.com/explore",
        camoufox::api::NavigateOptions {
            wait_until: Some("domcontentloaded".to_string()),
            ..Default::default()
        },
        Duration::from_secs(15),
    );

    std::thread::sleep(Duration::from_millis(2000));

    // 验证 Cookie 是否在浏览器中生效（检查 document.cookie 是否含关键字段）
    if let Ok(cval) = main_frame.evaluate(
        r#"JSON.stringify({
            cookieHasSession: document.cookie.indexOf('web_session') !== -1,
            cookieHasA1: document.cookie.indexOf('a1=') !== -1,
            cookieLen: document.cookie.length,
            readyState: document.readyState,
            title: document.title,
            url: location.href
        })"#,
        Duration::from_secs(5),
    ) {
        let cstr = cval
            .get("result")
            .and_then(|r| r.get("value"))
            .or_else(|| cval.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("(解析失败)");
        info!("[XHS/camoufox] Cookie 生效检查: {cstr}");
    }

    // Navigate to search URL（带 Referer，模拟从首页进入）
    info!("[XHS/camoufox] 导航到搜索页");
    let _ = main_frame.navigate(
        search_url,
        camoufox::api::NavigateOptions {
            wait_until: Some("domcontentloaded".to_string()),
            referer: Some("https://www.xiaohongshu.com/".to_string()),
        },
        Duration::from_secs(15),
    );

    info!(
        "[XHS/camoufox] 页面加载完成 ({}ms)",
        start.elapsed().as_millis()
    );

    // 调试：打印 camoufox 的指纹特征，判断是否被小红书识别
    if let Ok(finger) = main_frame.evaluate(
        r#"JSON.stringify({
            ua: navigator.userAgent,
            webdriver: navigator.webdriver,
            platform: navigator.platform,
            vendor: navigator.vendor,
            languages: navigator.languages
        })"#,
        Duration::from_secs(5),
    ) {
        let fstr = finger
            .get("result")
            .and_then(|r| r.get("value"))
            .or_else(|| finger.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("(解析失败)");
        info!("[XHS/camoufox] 指纹: {fstr}");
    }

    // 轮询等待搜索数据...
    let wait_js = format!(
        r#"(() => {{
            {}
            const s = window.__INITIAL_STATE__;
            if (!s || !s.search || !s.search.feeds) return null;
            const feeds = unwrap(s.search.feeds, 0);
            if (!Array.isArray(feeds) || feeds.length === 0) return null;
            const items = feeds.map(f => {{
                const nc = f.note_card || f.noteCard || {{}};
                const user = nc.user || {{}};
                const interact = nc.interact_info || nc.interactInfo || {{}};
                return {{
                    title: nc.display_title || nc.title || '',
                    author: user.nickname || user.nick_name || '',
                    likes: String(interact.liked_count || interact.likedCount || ''),
                    note_id: f.id || f.note_id || '',
                    xsec_token: f.xsec_token || ''
                }};
            }}).filter(r => r.note_id);
            if (items.length === 0) return null;
            return JSON.stringify(items);
        }})()"#,
        UNWRAP_JS
    );

    info!("[XHS/camoufox] 轮询等待搜索数据...");
    let mut attempts = 0;
    let max_attempts = 13; // 13 * 1.5s = ~20s
    let mut result_json = String::new();

    while attempts < max_attempts {
        attempts += 1;

        // evaluate 返回 serde_json::Value：JS 的 JSON.stringify(...) 得到一个字符串 value。
        // 需要从 result 里取 value 字符串。
        match main_frame.evaluate(&wait_js, Duration::from_secs(5)) {
            Ok(value) => {
                let str_val = value
                    .get("result")
                    .and_then(|r| r.get("value"))
                    .or_else(|| value.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !str_val.is_empty() && str_val != "null" {
                    result_json = str_val.to_string();
                    info!(
                        "[XHS/camoufox] ✓ 获取到搜索数据 (第 {} 次尝试, {}ms)",
                        attempts,
                        start.elapsed().as_millis()
                    );
                    break;
                }
            }
            Err(e) => {
                if attempts % 3 == 0 {
                    warn!(
                        "[XHS/camoufox] evaluate 失败 (尝试 {}/{}): {}",
                        attempts, max_attempts, e
                    );
                }
            }
        }

        std::thread::sleep(Duration::from_millis(1500));
    }

    // 超时后诊断页面实际状态，帮助定位问题并返回明确错误
    let mut fail_reason = String::from("搜索数据未加载（20 秒超时）");
    if result_json.is_empty() {
        warn!("[XHS/camoufox] 搜索数据超时未加载，诊断页面状态...");
        if let Ok(diag_val) = main_frame.evaluate(
            r#"(() => {
                const title = document.title || '';
                const url = window.location.href;
                const body = document.body ? document.body.innerText.substring(0, 500) : '';
                const s = window.__INITIAL_STATE__;
                const hasState = !!s;
                return JSON.stringify({title, url: url.substring(0, 300), hasState, bodyText: body});
            })()"#,
            Duration::from_secs(5),
        ) {
            let diag_str = diag_val
                .get("result")
                .and_then(|r| r.get("value"))
                .or_else(|| diag_val.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            info!("[XHS/camoufox] 诊断: {diag_str}");

            // 分类失败原因：账号风控/验证码 或 Cookie 失效
            let lower = diag_str.to_lowercase();
            if lower.contains("security verification")
                || lower.contains("captcha")
                || lower.contains("account security")
                || diag_str.contains("安全验证")
                || diag_str.contains("扫码验证")
            {
                fail_reason = "小红书要求扫码安全验证（账号风控），可能是当前网络环境/IP 未被信任。请尝试在真实浏览器中搜索一次解除风控，或更换账号。".to_string();
            } else if diag_str.contains("登录") || lower.contains("login") {
                fail_reason = "小红书 Cookie 已失效或未生效，请更新 xhs_cookie.txt 中的 Cookie。".to_string();
            }
        }
    }

    // Cleanup
    let _ = context.close();
    let _ = browser.close();
    let _ = std::fs::remove_dir_all(&profile_dir);

    if result_json.is_empty() {
        return Err(fail_reason);
    }

    // Parse results
    let items = crate::xiaohongshu::parse_search_json(&result_json);
    info!("[XHS/camoufox] 解析到 {} 条搜索结果", items.len());

    Ok(items.into_iter().take(max_results).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立的 camoufox 搜索探测（不依赖企业微信）。
    ///
    /// 运行方式（在 Linux/Docker 容器内）：
    /// ```bash
    /// cargo run --features xhs-camoufox -- --xhs-probe
    /// ```
    /// 或直接运行携带 xhs_cookie.txt 的集成测试。
    #[test]
    #[ignore = "需要 camoufox 二进制 + xhs_cookie.txt 文件"]
    fn probe_xhs_search_camoufox() {
        use crate::xiaohongshu;

        // 从 xhs_cookie.txt 读取 Cookie
        let cookie = std::fs::read_to_string("xhs_cookie.txt")
            .expect("需要 xhs_cookie.txt 文件")
            .trim()
            .to_string();
        assert!(!cookie.is_empty(), "xhs_cookie.txt 为空");

        // 搜索关键词
        let encoded = urlencoding::encode("星际争霸");
        let url = format!(
            "https://www.xiaohongshu.com/search_result?keyword={encoded}&source=web_explore_feed"
        );

        println!("=== 开始 camoufox 搜索探测 ===");
        let result = search_via_camoufox(&url, &cookie, 5);
        match result {
            Ok(items) => {
                println!("✓ 搜索成功，获取 {} 条结果:", items.len());
                for (i, item) in items.iter().enumerate() {
                    println!("  [{}.] {}", i + 1, item.title);
                    println!("       作者: {} | 点赞: {}", item.author, item.likes);
                    println!("       链接: {}", item.url);
                }
                assert!(!items.is_empty(), "应至少返回 1 条结果");
            }
            Err(e) => {
                panic!("✗ camoufox 搜索失败: {e}");
            }
        }
        println!("=== camoufox 搜索探测完成 ===");
    }

    /// 验证 camoufox 二进制可执行。
    #[test]
    #[ignore = "只在容器/Linux 下有意义"]
    fn probe_camoufox_binary_exists() {
        let bin = std::env::var("CAMOUFOX_BIN").unwrap_or_else(|_| "/opt/camoufox".to_string());
        println!("CAMOUFOX_BIN = {bin}");
        assert!(
            std::path::Path::new(&bin).exists(),
            "camoufox 二进制不存在: {bin}"
        );
    }
}
