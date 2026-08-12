#![allow(dead_code)]

use futures::{SinkExt, StreamExt};
use rust_websearch::{fetch_page, search, FetchConfig, SearchConfig};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

/// 单条搜索结果。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Obscura CDP 服务器地址，可通过环境变量覆盖。
const DEFAULT_OBSCURA_CDP_URL: &str = "http://127.0.0.1:9222";

/// 执行网络搜索，返回格式化后的结果文本（供注入到对话历史）。
pub async fn web_search(query: &str, max_results: usize) -> String {
    let config = SearchConfig::default();
    match search(query, &config).await {
        Ok(results) => {
            let hits: Vec<SearchHit> = results
                .results
                .into_iter()
                .take(max_results)
                .map(|r| SearchHit {
                    title: r.title,
                    url: r.url,
                    snippet: r.snippet,
                })
                .collect();

            if hits.is_empty() {
                warn!("搜索「{query}」无结果");
                return format!("（针对「{query}」的搜索没有返回任何结果。）");
            }

            info!("搜索「{query}」共获取 {} 条结果", hits.len());

            let mut buf = String::new();
            buf.push_str(&format!("以下是关于「{query}」的网络搜索结果：\n"));
            for (i, hit) in hits.iter().enumerate() {
                buf.push_str(&format!(
                    "\n[{}.] {}\n来源: {}\n摘要: {}\n",
                    i + 1,
                    hit.title,
                    hit.url,
                    hit.snippet
                ));
            }
            buf
        }
        Err(e) => {
            warn!("搜索「{query}」失败: {e}");
            format!("（搜索「{query}」出错：{e}）")
        }
    }
}

/// 抓取指定 URL 的页面内容，返回格式化文本（供注入到对话历史）。
///
/// 返回标题 + 正文（纯文本，最多 30KB）。
///
/// 行为受环境变量控制：
/// - `OBSCURA_ENABLED=true` 启用 Obscura。
/// - `OBSCURA_CDP_URL` 可覆盖 CDP 服务器地址，默认 `http://127.0.0.1:9222`。
/// - `OBSCURA_FETCH_MODE` 控制策略：
///   - `always`：始终用 Obscura 抓取（最稳，也最慢）。
///   - `fallback`（默认）：先普通抓取，失败/反爬/内容过差时 fallback 到 Obscura。
///   - `never`：只用普通抓取。
pub async fn fetch_url_content(url: &str) -> String {
    match fetch_mode() {
        FetchMode::Always => {
            info!("Obscura 模式为 always，直接调用 Obscura 抓取");
            return fetch_with_obscura(url).await;
        }
        FetchMode::Never => {
            return fetch_with_default(url).await;
        }
        FetchMode::Fallback => {
            // 先普通抓取；失败或内容异常再走 Obscura
            let first_try = fetch_with_default(url).await;
            if !first_try.starts_with('（') && !first_try.starts_with('(') {
                return first_try;
            }
            warn!("普通抓取异常，尝试 Obscura 重抓");
            fetch_with_obscura(url).await
        }
    }
}

/// 使用默认（rust-websearch）方式抓取页面。
async fn fetch_with_default(url: &str) -> String {
    let config = FetchConfig::default();
    match fetch_page(url, &config).await {
        Ok(page) => {
            info!(
                "抓取页面成功: {} ({} bytes, title={})",
                page.final_url,
                page.content_length,
                page.title
            );

            if page.content.trim().is_empty() {
                return format!("（页面「{url}」内容为空）");
            }

            // 如果正文太短（如只有反爬提示/JS 跳转），尝试 Obscura fallback
            if is_anti_bot_content(&page.content) {
                warn!("页面内容疑似反爬/无正文，标记为异常以便 fallback");
                return format!("（页面「{url}」疑似反爬或无正文，建议用 headless 浏览器重试）");
            }

            let mut buf = format!("以下是页面「{}」的内容：\n\n", page.title);
            if page.truncated {
                // 截断时只取前 30KB
                let truncated = page.content.chars().take(30_000).collect::<String>();
                buf.push_str(&truncated);
                buf.push_str("\n\n（内容较长，已截断）");
            } else {
                buf.push_str(&page.content);
            }
            buf
        }
        Err(e) => {
            warn!("抓取页面失败「{url}」: {e}");
            format!("（抓取页面「{url}」出错：{e}）")
        }
    }
}

/// 检查内容是否像反爬/校验页面而非真实正文。
fn is_anti_bot_content(content: &str) -> bool {
    let lower = content.to_lowercase();

    // 1. 明确反爬关键字
    let markers = [
        "checking your browser",
        "please wait",
        "access denied",
        "403 forbidden",
        "验证码",
        "正在校验",
        "安全验证",
        "anti-bot",
        "robot",
        "captcha",
        "blocked",
        "请稍后",
        "正在跳转",
        "location.href",
        "window.screen",
    ];
    if markers.iter().any(|m| lower.contains(m)) {
        return true;
    }

    // 2. 正文过短，大概率没拿到内容
    let trimmed = content.trim();
    if trimmed.chars().count() < 200 {
        return true;
    }

    // 3. script 标签比例过高（JS 跳转/广告页面）
    let script_count = lower.matches("<script").count();
    let total_chars = content.chars().count().max(1);
    if script_count >= 2 && (script_count * 1000) / total_chars > 1 {
        return true;
    }

    false
}

/// Obscura 抓取策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchMode {
    Always,
    Fallback,
    Never,
}

/// 解析 `OBSCURA_FETCH_MODE` 环境变量。
fn fetch_mode() -> FetchMode {
    if !obscura_enabled() {
        return FetchMode::Never;
    }
    match std::env::var("OBSCURA_FETCH_MODE").map(|v| v.to_lowercase()) {
        Ok(v) if v == "always" => FetchMode::Always,
        Ok(v) if v == "never" => FetchMode::Never,
        _ => FetchMode::Fallback,
    }
}

/// 是否启用 Obscura。
fn obscura_enabled() -> bool {
    std::env::var("OBSCURA_ENABLED")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Obscura CDP 服务器根地址（HTTP）。
fn obscura_cdp_url() -> String {
    std::env::var("OBSCURA_CDP_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OBSCURA_CDP_URL.to_string())
}

/// 使用 Obscura CDP 抓取页面纯文本内容。
async fn fetch_with_obscura(url: &str) -> String {
    info!("尝试使用 Obscura CDP 抓取: {url}");

    let cdp_root = obscura_cdp_url();
    let ws_url = match get_obscura_browser_ws_url(&cdp_root).await {
        Ok(u) => u,
        Err(e) => {
            warn!("无法获取 Obscura browser 的 WebSocket 地址: {e}");
            return format!("（无法连接 Obscura CDP：{e}）");
        }
    };

    let result = timeout(Duration::from_secs(60), fetch_text_via_cdp(&ws_url, url)).await;
    match result {
        Ok(Ok(text)) => {
            if text.trim().is_empty() {
                format!("（Obscura 抓取「{url}」返回空内容）")
            } else {
                format!("以下是页面「{url}」的内容：\n\n{text}")
            }
        }
        Ok(Err(e)) => {
            warn!("Obscura CDP 抓取失败: {e}");
            format!("（Obscura 抓取「{url}」失败：{e}）")
        }
        Err(_) => {
            warn!("Obscura CDP 抓取超时");
            format!("（Obscura 抓取「{url}」超时）")
        }
    }
}

/// 从 Obscura CDP 的 `/json/version` 获取 browser-level WebSocket 调试地址。
async fn get_obscura_browser_ws_url(cdp_root: &str) -> Result<String, String> {
    let version_url = format!("{cdp_root}/json/version");
    let body = reqwest::get(&version_url)
        .await
        .map_err(|e| format!("请求 {version_url} 失败: {e}"))?
        .text()
        .await
        .map_err(|e| format!("读取 CDP version 响应失败: {e}"))?;

    let version: Value = serde_json::from_str(&body)
        .map_err(|e| format!("解析 CDP version 失败: {e}（响应: {body}）"))?;

    version
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "CDP version 中没有 webSocketDebuggerUrl".to_string())
}

/// 通过 CDP WebSocket 抓取页面 body.innerText。
///
/// Obscura 采用 CDP flatten 模式：
/// 1. 连接 browser-level WebSocket。
/// 2. `Target.createTarget` 创建新 tab。
/// 3. `Target.attachToTarget` 获取 sessionId。
/// 4. 后续命令均带 `sessionId` 发送。
async fn fetch_text_via_cdp(ws_url: &str, url: &str) -> Result<String, String> {
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("连接 CDP WebSocket 失败: {e}"))?;
    let (mut write, mut read) = ws_stream.split();

    let mut next_id: i64 = 1;

    // 1. 创建新 target
    let create_id = send_cdp(
        &mut write,
        &mut next_id,
        "Target.createTarget",
        &json!({ "url": "about:blank" }),
        None,
    )
    .await;

    let target_id = read_response_with(&mut read, create_id, |v| {
        v.get("result")
            .and_then(|r| r.get("targetId"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
    .await
    .ok_or("Target.createTarget 未返回 targetId")?;

    // 2. attach，拿到 sessionId
    let attach_id = send_cdp(
        &mut write,
        &mut next_id,
        "Target.attachToTarget",
        &json!({ "targetId": target_id, "flatten": true }),
        None,
    )
    .await;

    let session_id = read_response_with(&mut read, attach_id, |v| {
        v.get("result")
            .and_then(|r| r.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
    .await
    .ok_or("Target.attachToTarget 未返回 sessionId")?;

    // 3. 启用 domain（带 sessionId）
    send_cdp(&mut write, &mut next_id, "Runtime.enable", &json!({}), Some(&session_id)).await;
    send_cdp(&mut write, &mut next_id, "Page.enable", &json!({}), Some(&session_id)).await;

    // 4. 导航到目标页面
    let navigate_id = send_cdp(
        &mut write,
        &mut next_id,
        "Page.navigate",
        &json!({ "url": url }),
        Some(&session_id),
    )
    .await;

    // 等待 Page.domContentEventFired 或 Page.loadEventFired，同时消费 navigate 响应
    let load_wait = wait_for_page_load_event(&mut read, navigate_id, &session_id);
    match timeout(Duration::from_secs(15), load_wait).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            warn!("等待页面加载事件超时，将尝试获取当前已渲染正文");
        }
    }

    // 5. 获取 document.body.innerText
    let eval_id = send_cdp(
        &mut write,
        &mut next_id,
        "Runtime.evaluate",
        &json!({
            "expression": "document.body ? document.body.innerText : ''",
            "returnByValue": true
        }),
        Some(&session_id),
    )
    .await;

    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if parsed.get("id").and_then(|v| v.as_i64()) == Some(eval_id) {
                    if let Some(error) = parsed.get("error") {
                        return Err(format!("Runtime.evaluate 错误: {error}"));
                    }
                    let inner_text = parsed
                        .get("result")
                        .and_then(|r| r.get("result"))
                        .and_then(|r| r.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Ok(inner_text);
                }
            }
            Ok(_) => continue,
            Err(e) => return Err(format!("CDP WebSocket 读取错误: {e}")),
        }
    }

    Err("CDP 连接在收到 evaluate 结果前关闭".to_string())
}

/// 读取指定 id 的响应，并通过 extractor 提取字段。
async fn read_response_with<F, T>(
    read: &mut futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
    expected_id: i64,
    extractor: F,
) -> Option<T>
where
    F: Fn(&Value) -> Option<T>,
{
    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    if parsed.get("id").and_then(|v| v.as_i64()) == Some(expected_id) {
                        return extractor(&parsed);
                    }
                }
            }
            Err(e) => {
                warn!("读取 CDP 响应时出错: {e}");
                return None;
            }
            _ => {}
        }
    }
    None
}

/// 等待页面 `Page.domContentEventFired` 或 `Page.loadEventFired` 事件。
async fn wait_for_page_load_event(
    read: &mut futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
    navigate_id: i64,
    session_id: &str,
) -> Result<(), String> {
    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if parsed.get("id").and_then(|v| v.as_i64()) == Some(navigate_id) {
                    if parsed.get("error").is_some() {
                        return Err(format!("Page.navigate 返回错误: {parsed}"));
                    }
                }

                let method = parsed.get("method").and_then(|v| v.as_str());
                let sid = parsed.get("sessionId").and_then(|v| v.as_str());
                if sid == Some(session_id)
                    && (method == Some("Page.domContentEventFired") || method == Some("Page.loadEventFired"))
                {
                    return Ok(());
                }
            }
            Ok(_) => continue,
            Err(e) => return Err(format!("CDP WebSocket 读取错误: {e}")),
        }
    }
    Err("CDP 连接在页面加载事件前关闭".to_string())
}

/// 发送一条 CDP 消息，返回消息 id。
async fn send_cdp(
    write: &mut futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        tokio_tungstenite::tungstenite::Message,
    >,
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
    if let Err(e) = write
        .send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into()))
        .await
    {
        warn!("发送 CDP 消息失败: {e}");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要本地 Obscura CDP 服务运行在 127.0.0.1:9222"]
    async fn test_obscura_fetch_zol() {
        // 确保启用 Obscura（Rust 2024 中 set_var 为 unsafe）
        unsafe {
            std::env::set_var("OBSCURA_ENABLED", "true");
            std::env::set_var("OBSCURA_FETCH_MODE", "always");
        }

        let url = "https://detail.zol.com.cn/vga/s11071/";
        let result = fetch_with_obscura(url).await;
        println!("=== Obscura 抓取结果 ===\n{result}\n=== 长度: {} ===", result.len());

        assert!(result.contains("RTX 5070") || result.contains("5070") || result.chars().count() > 1000,
            "Obscura 应该能抓到 ZOL 页面的显卡相关内容");
    }
}
