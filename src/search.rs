use rust_websearch::{search, SearchConfig};
use tracing::{info, warn};

/// 单条搜索结果。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 执行网络搜索，返回格式化后的结果文本（供注入到对话历史）。
///
/// - `query`：搜索关键词。
/// - `max_results`：最多返回的结果条数。
///
/// 返回格式化文本；若搜索失败或结果为 0，返回一个提示性占位文本，
/// 避免空结果导致二次生成出错。
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