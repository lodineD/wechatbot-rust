use rust_websearch::{fetch_page, search, FetchConfig, SearchConfig};
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