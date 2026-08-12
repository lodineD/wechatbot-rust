#![allow(dead_code)]

use rust_websearch::{fetch_page, search, FetchConfig, SearchConfig};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

/// 单条搜索结果。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

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
/// 如果普通抓取失败且环境变量 `OBSCURA_ENABLED=true`，会 fallback 到 Obscura headless 浏览器。
pub async fn fetch_url_content(url: &str) -> String {
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

            // 如果正文太短（如只有反爬提示），也尝试 Obscura fallback
            if is_anti_bot_content(&page.content) && obscura_enabled() {
                warn!("页面内容疑似反爬校验页，尝试 Obscura 重抓");
                return fetch_with_obscura(url).await;
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
            if obscura_enabled() {
                fetch_with_obscura(url).await
            } else {
                format!("（抓取页面「{url}」出错：{e}）")
            }
        }
    }
}

/// 检查内容是否像反爬/校验页面而非真实正文。
fn is_anti_bot_content(content: &str) -> bool {
    let lower = content.to_lowercase();
    let markers = [
        "checking your browser",
        "please wait",
        "access denied",
        "403 forbidden",
        "验证码",
        "正在校验",
        "安全验证",
        "anti-bot",
    ];
    markers.iter().any(|m| lower.contains(m))
}

/// 是否启用 Obscura fallback。
fn obscura_enabled() -> bool {
    std::env::var("OBSCURA_ENABLED")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// 使用 Obscura headless 浏览器抓取页面纯文本内容。
async fn fetch_with_obscura(url: &str) -> String {
    info!("尝试使用 Obscura 抓取: {url}");

    let cmd_future = Command::new("obscura")
        .args(["fetch", url, "--dump", "text"])
        .output();

    let result = match timeout(Duration::from_secs(60), cmd_future).await {
        Ok(res) => res,
        Err(_) => {
            warn!("Obscura 抓取超时");
            return format!("（Obscura 抓取「{url}」超时）");
        }
    };

    match result {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            info!(
                "Obscura 抓取成功: {} ({} bytes)",
                url,
                text.len()
            );
            if text.is_empty() {
                format!("（Obscura 抓取「{url}」返回空内容）")
            } else {
                format!("以下是页面「{url}」的内容：\n\n{text}")
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "未知".to_string());
            warn!("Obscura 抓取失败 (exit={code}): {stderr}");
            format!("（Obscura 抓取「{url}」失败，退出码 {code}：{stderr}）")
        }
        Err(e) => {
            warn!("无法启动 Obscura: {e}");
            format!("（无法启动 Obscura 抓取「{url}」：{e}）")
        }
    }
}
