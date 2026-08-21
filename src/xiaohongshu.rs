//! 小红书搜索与笔记详情获取。
//!
//! - **搜索**：优先使用 camoufox（反指纹 Firefox），回退到 Obscura CDP（headless Chrome）。
//!   camoufox 能绕过小红书的风控检测，参考 xhs-cli 的策略。
//! - **笔记详情**：直接 HTTP 请求 + Cookie，从 SSR `__INITIAL_STATE__` 解析。

use futures::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tracing::{debug, info, warn};

/// 小红书域名，用于 Cookie 注入。
pub const XHS_DOMAIN: &str = ".xiaohongshu.com";

/// CDP 默认地址。
const DEFAULT_OBSCURA_CDP_URL: &str = "http://127.0.0.1:9222";

/// Vue 响应式代理解包 JS 函数。
/// 小红书前端使用 Vue.js，__INITIAL_STATE__ 中的数据被响应式代理包裹，
/// 需要递归解包才能获取原始 JSON 数据。
pub const UNWRAP_JS: &str = r#"
function unwrap(obj, depth) {
    if (depth > 8 || obj === null || obj === undefined) return obj;
    if (typeof obj !== 'object') return obj;
    if ('_value' in obj && 'dep' in obj) return unwrap(obj._value, depth + 1);
    if ('value' in obj && 'dep' in obj) return unwrap(obj.value, depth + 1);
    if ('_rawValue' in obj) return unwrap(obj._rawValue, depth + 1);
    if (Array.isArray(obj)) return obj.map(item => unwrap(item, depth + 1));
    const result = {};
    for (const key of Object.keys(obj)) {
        if (key === 'dep' || key.startsWith('__')) continue;
        try { result[key] = unwrap(obj[key], depth + 1); } catch(e) {}
    }
    return result;
}
"#;

/// 反自动化检测 JS，在每个新文档加载前自动执行。
/// 隐藏 CDP/headless Chrome 的指纹特征，避免被小红书反爬系统识别。
const STEALTH_JS: &str = r#"
Object.defineProperty(navigator, 'webdriver', { get: () => undefined });
Object.defineProperty(navigator, 'plugins', {
    get: () => [
        { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
        { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
        { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' }
    ]
});
Object.defineProperty(navigator, 'languages', { get: () => ['zh-CN', 'zh', 'en'] });
window.chrome = window.chrome || {};
window.chrome.runtime = {
    connect: function() {}, sendMessage: function() {},
    onMessage: { addListener: function() {} }, onConnect: { addListener: function() {} }
};
try { delete navigator.__proto__.webdriver; } catch(e) {}
"#;

// ─── 公共接口 ───

/// 搜索小红书，返回格式化结果文本。
/// 优先使用 camoufox（Unix + feature 启用时），回退到 Obscura CDP。
pub async fn xhs_search(query: &str, max_results: usize, cookie: &str) -> String {
    if cookie.is_empty() {
        return "（小红书 Cookie 未配置，请在 .env 中设置 XHS_COOKIE）".to_string();
    }

    let encoded_query = urlencoding::encode(query);
    let search_url = format!(
        "https://www.xiaohongshu.com/search_result?keyword={encoded_query}&source=web_explore_feed"
    );

    info!("[XHS] 搜索: query={query}");

    // 尝试 camoufox（Unix only, feature-gated）
    #[cfg(all(unix, feature = "xhs-camoufox"))]
    {
        info!("[XHS] 使用 camoufox 反指纹浏览器");
        let cookie_owned = cookie.to_string();
        let url_owned = search_url.clone();

        let result = tokio::task::spawn_blocking(move || {
            crate::camoufox_backend::search_via_camoufox(&url_owned, &cookie_owned, max_results)
        })
        .await;

        match result {
            Ok(Ok(items)) => {
                if items.is_empty() {
                    return format!("（小红书搜索「{query}」未找到结果）");
                }
                info!("[XHS] camoufox 搜索「{query}」获取 {} 条结果", items.len());
                return format_search_results(query, &items);
            }
            Ok(Err(e)) => {
                warn!("[XHS] camoufox 搜索失败: {e}");
                // Fall through to CDP
            }
            Err(e) => {
                warn!("[XHS] camoufox task 失败: {e}");
                // Fall through to CDP
            }
        }
    }

    // 回退到 Obscura CDP
    #[cfg(not(all(unix, feature = "xhs-camoufox")))]
    {
        info!("[XHS] camoufox 未启用，使用 Obscura CDP");
    }
    #[cfg(all(unix, feature = "xhs-camoufox"))]
    {
        info!("[XHS] camoufox 失败，回退到 Obscura CDP");
    }

    let cdp_root = obscura_cdp_url();
    let ws_url = match get_browser_ws_url(&cdp_root).await {
        Ok(u) => u,
        Err(e) => {
            warn!("[XHS] 无法获取 Obscura browser WebSocket 地址: {e}");
            return format!("（小红书搜索失败：无法连接 CDP：{e}）");
        }
    };

    let result = timeout(
        Duration::from_secs(90),
        search_via_cdp(&ws_url, &search_url, cookie, max_results),
    )
    .await;

    match result {
        Ok(Ok(items)) => {
            if items.is_empty() {
                format!("（小红书搜索「{query}」未找到结果，可能 Cookie 已过期或触发了反爬机制）")
            } else {
                info!("[XHS] 搜索「{query}」获取 {} 条结果", items.len());
                format_search_results(query, &items)
            }
        }
        Ok(Err(e)) => {
            warn!("[XHS] 搜索失败: {e}");
            if e.contains("Cookie") || e.contains("登录") || e.contains("未登录") {
                format!("（小红书搜索失败：{e}。请更新 .env 中的 XHS_COOKIE）")
            } else {
                format!("（小红书搜索「{query}」失败：{e}）")
            }
        }
        Err(_) => {
            warn!("[XHS] 搜索超时 (90s)");
            format!("（小红书搜索「{query}」超时，可能页面加载过慢或触发了反爬）")
        }
    }
}

/// 获取小红书笔记详情，返回格式化文本。
pub async fn xhs_note_detail(url: &str, cookie: &str) -> String {
    if cookie.is_empty() {
        return "（小红书 Cookie 未配置，请在 .env 中设置 XHS_COOKIE）".to_string();
    }

    let note_url = normalize_note_url(url);
    info!("[XHS] 获取笔记详情: {note_url}");

    let client = match HttpClient::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("（构建 HTTP 客户端失败：{e}）"),
    };

    let resp = match client
        .get(&note_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36")
        .header("Referer", "https://www.xiaohongshu.com/")
        .header("Cookie", cookie)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("（请求小红书笔记失败：{e}）"),
    };

    let html = match resp.text().await {
        Ok(h) => h,
        Err(e) => return format!("（读取小红书笔记响应失败：{e}）"),
    };

    parse_note_from_html(&html, &note_url)
}

// ─── CDP 消息分发器 ───

type WsMsg = tokio_tungstenite::tungstenite::Message;
type WsSink = futures::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMsg,
>;

struct CdpResponse {
    id: i64,
    value: Value,
}

#[allow(dead_code)]
struct CdpEvent {
    method: String,
    session_id: Option<String>,
    params: Value,
}

/// CDP 会话：封装 WebSocket 消息分发。
struct CdpSession {
    write: WsSink,
    cmd_tx: mpsc::UnboundedSender<CdpResponse>,
    cmd_rx: mpsc::UnboundedReceiver<CdpResponse>,
    event_rx: mpsc::UnboundedReceiver<CdpEvent>,
    next_id: i64,
    session_id: String,
    target_id: String,
}

/// 创建 CDP 会话：连接 WebSocket，spawn reader，创建 target 并 attach。
async fn create_cdp_session(ws_url: &str) -> Result<CdpSession, String> {
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("连接 CDP WebSocket 失败: {e}"))?;
    let (mut write, read) = ws_stream.split();

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<CdpResponse>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<CdpEvent>();
    let cmd_tx_reader = cmd_tx.clone();

    // Spawn reader task：持续读取 WS 消息并分发到对应 channel
    tokio::spawn(async move {
        let mut read = read;
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(WsMsg::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        if let Some(id) = parsed.get("id").and_then(|v| v.as_i64()) {
                            let _ = cmd_tx_reader.send(CdpResponse { id, value: parsed });
                        } else if let Some(method) = parsed.get("method").and_then(|v| v.as_str()) {
                            let session_id = parsed
                                .get("sessionId")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            let params = parsed.get("params").cloned().unwrap_or(Value::Null);
                            let _ = event_tx.send(CdpEvent {
                                method: method.to_string(),
                                session_id,
                                params,
                            });
                        }
                    }
                }
                Ok(WsMsg::Close(_)) => {
                    debug!("[XHS] CDP WebSocket 关闭");
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("[XHS] CDP WebSocket 读取错误: {e}");
                    break;
                }
            }
        }
    });

    let mut next_id: i64 = 1;

    // Target.createTarget
    let create_id = send_raw(
        &mut write,
        &mut next_id,
        "Target.createTarget",
        &json!({ "url": "about:blank" }),
        None,
    )
    .await;
    let target_id = wait_cmd(&mut cmd_rx, &cmd_tx, create_id, 10)
        .await?
        .get("result")
        .and_then(|r| r.get("targetId"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or("Target.createTarget 未返回 targetId")?;
    info!("[XHS] target 创建: {target_id}");

    // Target.attachToTarget
    let attach_id = send_raw(
        &mut write,
        &mut next_id,
        "Target.attachToTarget",
        &json!({ "targetId": target_id, "flatten": true }),
        None,
    )
    .await;
    let session_id = wait_cmd(&mut cmd_rx, &cmd_tx, attach_id, 10)
        .await?
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or("Target.attachToTarget 未返回 sessionId")?;
    info!(
        "[XHS] session 建立: {}...",
        &session_id[..16.min(session_id.len())]
    );

    Ok(CdpSession {
        write,
        cmd_tx,
        cmd_rx,
        event_rx,
        next_id,
        session_id,
        target_id,
    })
}

impl CdpSession {
    /// 发送 CDP 命令并等待响应（短超时，配合轮询使用）。
    async fn send_cmd(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let msg_id = send_raw(
            &mut self.write,
            &mut self.next_id,
            method,
            params,
            Some(&self.session_id),
        )
        .await;
        wait_cmd(&mut self.cmd_rx, &self.cmd_tx, msg_id, 5).await // 5秒超时，不阻塞轮询
    }

    /// 执行 JS 表达式，返回字符串结果。
    async fn evaluate(&mut self, expression: &str) -> Result<String, String> {
        let resp = self
            .send_cmd(
                "Runtime.evaluate",
                &json!({ "expression": expression, "returnByValue": true }),
            )
            .await?;

        if let Some(error) = resp.get("error") {
            return Err(format!("JS 执行错误: {error}"));
        }

        resp.get("result")
            .and_then(|r| r.get("result"))
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| "JS 表达式无返回值".to_string())
    }

    /// 导航到指定 URL（不等待命令响应，靠后续轮询判断页面就绪）。
    async fn navigate(&mut self, url: &str) {
        // 清空事件队列
        while self.event_rx.try_recv().is_ok() {}

        // 发送导航命令，不等待响应（xhs-cli 策略：靠轮询 __INITIAL_STATE__ 判断就绪）
        let msg_id = send_raw(
            &mut self.write,
            &mut self.next_id,
            "Page.navigate",
            &json!({ "url": url }),
            Some(&self.session_id),
        )
        .await;
        info!("[XHS] 导航命令已发送 (id={msg_id}): {url}");

        // 短暂等待让命令到达浏览器
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    /// 轮询等待 JS 条件满足，返回 evaluate 结果。
    async fn poll_for_data(
        &mut self,
        check_js: &str,
        poll_interval_ms: u64,
        timeout_secs: u64,
    ) -> Result<String, String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut attempt = 0u32;

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err("等待数据超时".to_string());
            }

            attempt += 1;
            match self.evaluate(check_js).await {
                Ok(result)
                    if !result.is_empty()
                        && result != "null"
                        && result != "[]"
                        && result != "{}" =>
                {
                    return Ok(result);
                }
                Ok(_) => {
                    if attempt % 3 == 0 {
                        debug!("[XHS] 轮询第 {attempt} 次，数据尚未就绪");
                    }
                }
                Err(e) => {
                    debug!("[XHS] 轮询第 {attempt} 次 evaluate 出错: {e}");
                }
            }

            tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
        }
    }

    /// 关闭 target 释放浏览器资源。
    async fn close(&mut self) {
        send_raw(
            &mut self.write,
            &mut self.next_id,
            "Target.closeTarget",
            &json!({ "targetId": self.target_id }),
            None,
        )
        .await;
    }
}

// ─── CDP 底层辅助函数 ───

async fn send_raw(
    write: &mut WsSink,
    next_id: &mut i64,
    method: &str,
    params: &Value,
    session_id: Option<&str>,
) -> i64 {
    let id = *next_id;
    *next_id += 1;
    let mut msg = json!({ "id": id, "method": method, "params": params });
    if let Some(sid) = session_id {
        msg["sessionId"] = json!(sid);
    }
    if let Err(e) = write.send(WsMsg::Text(msg.to_string().into())).await {
        warn!("[XHS] 发送 CDP 消息失败: {e}");
    }
    id
}

async fn wait_cmd(
    rx: &mut mpsc::UnboundedReceiver<CdpResponse>,
    cmd_tx: &mpsc::UnboundedSender<CdpResponse>,
    expected_id: i64,
    timeout_secs: u64,
) -> Result<Value, String> {
    let mut stash = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let remaining = deadline.duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            for resp in stash {
                let _ = cmd_tx.send(resp);
            }
            return Err(format!("等待 CDP 响应超时 (id={expected_id})"));
        }

        match timeout(remaining, rx.recv()).await {
            Ok(Some(resp)) => {
                if resp.id == expected_id {
                    for r in stash {
                        let _ = cmd_tx.send(r);
                    }
                    if let Some(error) = resp.value.get("error") {
                        return Err(format!("CDP 命令错误: {error}"));
                    }
                    return Ok(resp.value);
                } else {
                    stash.push(resp);
                }
            }
            Ok(None) => {
                for r in stash {
                    let _ = cmd_tx.send(r);
                }
                return Err("CDP 命令通道已关闭".to_string());
            }
            Err(_) => {
                for r in stash {
                    let _ = cmd_tx.send(r);
                }
                return Err(format!("等待 CDP 响应超时 (id={expected_id})"));
            }
        }
    }
}

// ─── 搜索流程 ───

async fn search_via_cdp(
    ws_url: &str,
    search_url: &str,
    cookie: &str,
    max_results: usize,
) -> Result<Vec<XhsSearchItem>, String> {
    let start = std::time::Instant::now();
    info!("[XHS] 开始 CDP 搜索流程");

    let mut cdp = create_cdp_session(ws_url).await?;
    info!("[XHS] CDP 会话就绪 ({}ms)", start.elapsed().as_millis());

    // 1. 启用 Runtime + Page（不启用 Network，避免事件洪泛）
    cdp.send_cmd("Runtime.enable", &json!({})).await?;
    cdp.send_cmd("Page.enable", &json!({})).await?;
    info!(
        "[XHS] Runtime/Page 已启用 ({}ms)",
        start.elapsed().as_millis()
    );

    // 2. 注入反检测脚本（必须在任何导航之前）
    cdp.send_cmd(
        "Page.addScriptToEvaluateOnNewDocument",
        &json!({ "source": STEALTH_JS }),
    )
    .await?;
    info!("[XHS] 反检测脚本已注入");

    // 3. 设置视口尺寸（避免 0x0 暴露 headless）
    cdp.send_cmd(
        "Emulation.setDeviceMetricsOverride",
        &json!({
            "width": 1920,
            "height": 1080,
            "deviceScaleFactor": 1,
            "mobile": false
        }),
    )
    .await?;
    info!("[XHS] 视口已设置为 1920x1080");

    // 4. 设置 User-Agent
    cdp.send_cmd(
        "Network.setUserAgentOverride",
        &json!({
            "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36",
            "platform": "Win32",
            "acceptLanguage": "zh-CN,zh;q=0.9,en;q=0.8"
        }),
    ).await?;

    // 3. 注入 Cookie（Network.setCookies 不需要 Network.enable）
    let cookies = parse_cookie_string(cookie);
    if !cookies.is_empty() {
        let cookie_names: Vec<&str> = cookies.iter().map(|(n, _)| n.as_str()).collect();
        cdp.send_cmd(
            "Network.setCookies",
            &json!({
                "cookies": cookies.iter().map(|(name, value)| {
                    json!({ "name": name, "value": value, "domain": XHS_DOMAIN, "path": "/" })
                }).collect::<Vec<_>>()
            }),
        )
        .await?;
        info!(
            "[XHS] Cookie 注入: {} 个 [{}]",
            cookies.len(),
            cookie_names.join(", ")
        );
    }

    // 4. 导航到首页建立会话（不等待 CDP 响应，靠轮询判断）
    cdp.navigate("https://www.xiaohongshu.com/explore").await;
    info!("[XHS] 首页导航已发送 ({}ms)", start.elapsed().as_millis());

    // 等待首页 Vue 数据就绪
    random_delay_ms(2000, 3000).await;

    // 5. 验证登录状态
    let login_check = cdp
        .evaluate(
            "(() => { \
            try { \
                const s = window.__INITIAL_STATE__; \
                if (!s || !s.user) return JSON.stringify({has_state: false, logged_in: false}); \
                const u = s.user.loggedIn !== undefined ? s.user.loggedIn : \
                          (s.user.userPageData !== undefined); \
                return JSON.stringify({has_state: true, logged_in: !!u, \
                    user_keys: Object.keys(s.user || {}).slice(0, 10).join(',')}); \
            } catch(e) { return JSON.stringify({has_state: false, error: e.message}); } \
        })()",
        )
        .await
        .unwrap_or_default();

    let login_info: Value = serde_json::from_str(&login_check).unwrap_or_default();
    let logged_in = login_info
        .get("logged_in")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_state = login_info
        .get("has_state")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    info!("[XHS] 登录验证: has_state={has_state}, logged_in={logged_in}, raw={login_check}");

    if !logged_in {
        warn!("[XHS] Cookie 验证失败：用户未登录。可能 Cookie 已过期。");
        // 不立即返回，继续尝试搜索（有些搜索结果不需要登录）
    }

    // 6. 导航到搜索页（不等待 CDP 响应，靠轮询判断）
    cdp.navigate(search_url).await;
    info!("[XHS] 搜索页导航已发送 ({}ms)", start.elapsed().as_millis());

    // 初始等待让 JS 有时间发起搜索 API 请求
    random_delay_ms(3000, 4000).await;

    // 7. 轮询等待 Vue 数据填充（每 1.5 秒检查一次，最多 20 秒）
    let wait_js = format!(
        r#"(() => {{
            {UNWRAP_JS}
            const s = window.__INITIAL_STATE__;
            if (!s || !s.search) return null;
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
        }})()"#
    );

    info!("[XHS] 开始轮询等待 Vue 搜索数据...");
    match cdp.poll_for_data(&wait_js, 1500, 20).await {
        Ok(json_str) => {
            info!(
                "[XHS] ✓ 获取到搜索数据 ({}ms), {} bytes",
                start.elapsed().as_millis(),
                json_str.len()
            );
            let items = parse_search_json(&json_str);
            info!("[XHS] 解析到 {} 条搜索结果", items.len());
            cdp.close().await;
            Ok(items.into_iter().take(max_results).collect())
        }
        Err(e) => {
            warn!("[XHS] 轮询等待搜索数据失败: {e}");

            // 最终诊断：检查页面状态
            let diag = cdp.evaluate(
                r#"(() => {
                    const title = document.title || '';
                    const url = window.location.href;
                    const text = document.body ? document.body.innerText.substring(0, 500) : '';
                    const hasLogin = !!document.querySelector('[class*="login"]');
                    const hasCaptcha = !!document.querySelector('[class*="verify"], [class*="captcha"]');
                    const s = window.__INITIAL_STATE__;
                    const stateKeys = s ? Object.keys(s).join(',') : 'no_state';
                    const searchKeys = s && s.search ? Object.keys(s.search).join(',') : 'no_search';
                    return JSON.stringify({title, url: url.substring(0, 200), hasLogin, hasCaptcha, stateKeys, searchKeys, text: text.substring(0, 300)});
                })()"#
            ).await.unwrap_or_else(|_| "{}".to_string());

            info!("[XHS] 诊断: {diag}");
            cdp.close().await;

            let diag_v: Value = serde_json::from_str(&diag).unwrap_or_default();
            if diag_v
                .get("hasLogin")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || diag_v
                    .get("hasCaptcha")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                Err("检测到登录弹窗或验证码，Cookie 可能已失效".to_string())
            } else {
                Err(format!("搜索数据未加载。诊断: {diag}"))
            }
        }
    }
}

// ─── 搜索结果解析 ───

pub fn parse_search_json(json_str: &str) -> Vec<XhsSearchItem> {
    let items: Vec<Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            warn!("[XHS] 解析搜索结果 JSON 失败: {e}");
            return vec![];
        }
    };

    items
        .into_iter()
        .filter_map(|item| {
            let note_id = item.get("note_id").and_then(|v| v.as_str())?;
            if note_id.is_empty() {
                return None;
            }
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("(无标题)").to_string();
            let author = item.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let likes = item.get("likes").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let xsec_token = item.get("xsec_token").and_then(|v| v.as_str()).unwrap_or("");

            let url = if xsec_token.is_empty() {
                format!("https://www.xiaohongshu.com/explore/{note_id}")
            } else {
                format!("https://www.xiaohongshu.com/explore/{note_id}?xsec_token={xsec_token}&xsec_source=pc_search")
            };

            Some(XhsSearchItem { title, author, likes, url })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct XhsSearchItem {
    pub title: String,
    pub author: String,
    pub likes: String,
    pub url: String,
}

// ─── 笔记详情解析 ───

fn parse_note_from_html(html: &str, note_url: &str) -> String {
    let state_start = match html.find("__INITIAL_STATE__=") {
        Some(pos) => pos + "__INITIAL_STATE__=".len(),
        None => return format!("（页面「{note_url}」中没有 __INITIAL_STATE__ 数据）"),
    };
    let state_end = match html[state_start..].find("</script>") {
        Some(pos) => state_start + pos,
        None => return format!("（页面「{note_url}」中 __INITIAL_STATE__ 未闭合）"),
    };

    let raw = &html[state_start..state_end];
    let cleaned = sanitize_js_json(raw);

    let data: Value = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(e) => {
            warn!("解析 __INITIAL_STATE__ JSON 失败: {e}");
            return format!("（解析小红书笔记数据失败：{e}）");
        }
    };

    let note_map = match data.get("note").and_then(|n| n.get("noteDetailMap")) {
        Some(m) => m,
        None => return format!("（页面「{note_url}」中没有笔记数据）"),
    };

    let note_obj = note_map
        .as_object()
        .and_then(|map| map.values().next())
        .and_then(|v| v.get("note"));

    let note = match note_obj {
        Some(n) => n,
        None => return format!("（页面「{note_url}」中笔记对象为空）"),
    };

    let title = note
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(无标题)");
    let desc = note.get("desc").and_then(|v| v.as_str()).unwrap_or("");
    let author = note
        .get("user")
        .and_then(|u| u.get("nickname"))
        .and_then(|v| v.as_str())
        .unwrap_or("未知");
    let note_type = note
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let interact = note.get("interactInfo");
    let likes = interact
        .and_then(|i| i.get("likedCount"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let comments = interact
        .and_then(|i| i.get("commentCount"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let tags: Vec<&str> = note
        .get("tagList")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                .collect()
        })
        .unwrap_or_default();

    let images: Vec<&str> = note
        .get("imageList")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|img| {
                    img.get("urlDefault")
                        .or_else(|| img.get("url"))
                        .and_then(|v| v.as_str())
                })
                .collect()
        })
        .unwrap_or_default();

    let mut buf = String::new();
    buf.push_str(&format!("📕 小红书笔记「{title}」\n"));
    buf.push_str(&format!("作者: {author} | 类型: {note_type}\n"));
    if !likes.is_empty() || !comments.is_empty() {
        buf.push_str(&format!("点赞: {likes} | 评论: {comments}\n"));
    }
    if !tags.is_empty() {
        buf.push_str(&format!("标签: {}\n", tags.join(", ")));
    }
    buf.push_str(&format!("链接: {note_url}\n\n"));
    if !desc.is_empty() {
        buf.push_str("【正文】\n");
        buf.push_str(desc);
        buf.push('\n');
    }
    if !images.is_empty() {
        buf.push_str(&format!("\n【图片（共 {} 张）】\n", images.len()));
        for (i, img) in images.iter().enumerate() {
            buf.push_str(&format!("[{}] {}\n", i + 1, img));
        }
    }
    buf
}

// ─── 辅助函数 ───

fn format_search_results(query: &str, items: &[XhsSearchItem]) -> String {
    let mut buf = String::new();
    buf.push_str(&format!(
        "📕 小红书搜索「{query}」结果（共 {} 条）：\n",
        items.len()
    ));
    for (i, item) in items.iter().enumerate() {
        buf.push_str(&format!("\n[{}.] {}", i + 1, item.title));
        if !item.author.is_empty() {
            buf.push_str(&format!("\n作者: {}", item.author));
        }
        if !item.likes.is_empty() {
            buf.push_str(&format!(" | 点赞: {}", item.likes));
        }
        buf.push_str(&format!("\n链接: {}", item.url));
        buf.push('\n');
    }
    buf
}

fn normalize_note_url(url: &str) -> String {
    if url.starts_with("https://www.xiaohongshu.com/explore/")
        || url.starts_with("https://www.xiaohongshu.com/discovery/item/")
    {
        return url.to_string();
    }
    if url.len() == 24 && url.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("https://www.xiaohongshu.com/explore/{url}");
    }
    url.to_string()
}

fn sanitize_js_json(raw: &str) -> String {
    let mut s = raw.to_string();
    s = replace_word_boundary(&s, "undefined", "null");
    s = replace_word_boundary(&s, "NaN", "null");
    s = replace_word_boundary(&s, "Infinity", "null");
    s = replace_new_constructor(&s, "new Map([", "])", "{}");
    s = replace_new_constructor(&s, "new Set([", "])", "[]");
    s
}

fn replace_word_boundary(input: &str, word: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut search_from = 0;
    let bytes = input.as_bytes();
    while let Some(rel_pos) = input[search_from..].find(word) {
        let pos = search_from + rel_pos;
        let before_ok =
            pos == 0 || (!bytes[pos - 1].is_ascii_alphanumeric() && bytes[pos - 1] != b'_');
        let after_pos = pos + word.len();
        let after_ok = after_pos >= bytes.len()
            || (!bytes[after_pos].is_ascii_alphanumeric() && bytes[after_pos] != b'_');
        result.push_str(&input[search_from..pos]);
        if before_ok && after_ok && !is_inside_json_string(&input[..pos]) {
            result.push_str(replacement);
        } else {
            result.push_str(word);
        }
        search_from = pos + word.len();
    }
    result.push_str(&input[search_from..]);
    result
}

fn is_inside_json_string(prefix: &str) -> bool {
    let mut in_string = false;
    let mut escape = false;
    for ch in prefix.chars() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
        }
    }
    in_string
}

fn replace_new_constructor(input: &str, prefix: &str, suffix: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut search_from = 0;
    while let Some(start) = input[search_from..].find(prefix) {
        let abs_start = search_from + start;
        let after_prefix = abs_start + prefix.len();
        if let Some(end_rel) = input[after_prefix..].find(suffix) {
            let abs_end = after_prefix + end_rel + suffix.len();
            result.push_str(&input[search_from..abs_start]);
            result.push_str(replacement);
            search_from = abs_end;
        } else {
            break;
        }
    }
    result.push_str(&input[search_from..]);
    result
}

pub fn parse_cookie_string(cookie: &str) -> Vec<(String, String)> {
    cookie
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let eq_pos = pair.find('=')?;
            let name = pair[..eq_pos].trim().to_string();
            let value = pair[eq_pos + 1..].trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some((name, value))
            }
        })
        .collect()
}

async fn random_delay_ms(min_ms: u64, max_ms: u64) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let range = max_ms - min_ms;
    let delay = min_ms + (nanos as u64 % range.max(1));
    tokio::time::sleep(Duration::from_millis(delay)).await;
}

async fn get_browser_ws_url(cdp_root: &str) -> Result<String, String> {
    let version_url = format!("{cdp_root}/json/version");
    let body = reqwest::get(&version_url)
        .await
        .map_err(|e| format!("请求 {version_url} 失败: {e}"))?
        .text()
        .await
        .map_err(|e| format!("读取 CDP version 响应失败: {e}"))?;

    let version: Value = serde_json::from_str(&body)
        .map_err(|e| format!("解析 CDP version 失败: {e}（响应: {body}）"))?;

    let ws_url = version
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "CDP version 中没有 webSocketDebuggerUrl".to_string())?;

    let expected_host = cdp_root
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let rewritten = if let Some(scheme_end) = ws_url.find("://") {
        let after_scheme = scheme_end + 3;
        let rest = &ws_url[after_scheme..];
        if let Some(path_offset) = rest.find('/') {
            let path_start = after_scheme + path_offset;
            format!(
                "{}{}{}",
                &ws_url[..after_scheme],
                expected_host,
                &ws_url[path_start..]
            )
        } else {
            ws_url.to_string()
        }
    } else {
        ws_url.to_string()
    };

    if rewritten != ws_url {
        info!("CDP WebSocket 地址重写: {ws_url} → {rewritten}");
    }
    Ok(rewritten)
}

fn obscura_cdp_url() -> String {
    std::env::var("OBSCURA_CDP_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OBSCURA_CDP_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cookie_string() {
        let cookie = "a1=abc123; web_session=xyz789; gid=test";
        let pairs = parse_cookie_string(cookie);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], ("a1".to_string(), "abc123".to_string()));
        assert_eq!(pairs[1], ("web_session".to_string(), "xyz789".to_string()));
        assert_eq!(pairs[2], ("gid".to_string(), "test".to_string()));
    }

    #[test]
    fn test_normalize_note_url() {
        assert_eq!(
            normalize_note_url(
                "https://www.xiaohongshu.com/explore/6a3346880000000021014789?xsec_token=abc"
            ),
            "https://www.xiaohongshu.com/explore/6a3346880000000021014789?xsec_token=abc"
        );
        assert_eq!(
            normalize_note_url("6a3346880000000021014789"),
            "https://www.xiaohongshu.com/explore/6a3346880000000021014789"
        );
    }

    #[test]
    fn test_replace_word_boundary() {
        assert_eq!(
            replace_word_boundary(r#"{"a":undefined,"b":"undefined"}"#, "undefined", "null"),
            r#"{"a":null,"b":"undefined"}"#
        );
    }

    #[test]
    fn test_sanitize_js_json() {
        let raw = r#"{"a":undefined,"b":new Map([]),"c":new Set([])}"#;
        let result = sanitize_js_json(raw);
        assert_eq!(result, r#"{"a":null,"b":{},"c":[]}"#);
    }

    #[test]
    fn test_parse_search_json() {
        let json = r#"[{"title":"测试","author":"作者","likes":"100","note_id":"abc123","xsec_token":"token1"}]"#;
        let items = parse_search_json(json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "测试");
        assert!(items[0].url.contains("abc123"));
    }

    #[test]
    fn test_parse_search_json_empty() {
        assert!(parse_search_json("[]").is_empty());
    }

    #[tokio::test]
    #[ignore = "需要本地 Obscura CDP 服务和有效的 XHS_COOKIE"]
    async fn test_xhs_search_real() {
        let cookie = std::env::var("XHS_COOKIE").expect("需要设置 XHS_COOKIE 环境变量");
        let result = xhs_search("Rust编程", 5, &cookie).await;
        println!("=== 小红书搜索结果 ===\n{result}\n=== END ===");
        assert!(result.contains("小红书搜索"));
    }

    #[tokio::test]
    #[ignore = "需要网络 + 有效的 XHS_COOKIE"]
    async fn test_xhs_note_detail_real() {
        let cookie = std::env::var("XHS_COOKIE").expect("需要设置 XHS_COOKIE 环境变量");
        let url = "https://www.xiaohongshu.com/explore/6a3346880000000021014789?xsec_token=ABtmu-Dp2Mv-pBuHBl4OYCTDR349kYXuBD4mdhXJcjJIQ=&xsec_source=pc_search";
        let result = xhs_note_detail(url, &cookie).await;
        println!("=== 小红书笔记详情 ===\n{result}\n=== END ===");
        assert!(result.contains("星际") || result.contains("笔记"));
    }
}
