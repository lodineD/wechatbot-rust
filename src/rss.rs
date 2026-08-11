use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// 单条 RSS 文章。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssItem {
    pub title: String,
    pub link: String,
    pub pub_date: String,
    pub description: String,
}

/// 从 rustcc.cn 获取日报/资讯列表，返回格式化文本。
pub async fn fetch_rust_news(http: &HttpClient) -> String {
    let rss_url = "https://rustcc.cn/rss";
    match http.get(rss_url).send().await {
        Ok(response) => match response.text().await {
            Ok(body) => parse_and_format(&body),
            Err(e) => {
                warn!("读取 RSS 响应失败: {e}");
                format!("（获取 Rust.cc 资讯失败：{e}）")
            }
        },
        Err(e) => {
            warn!("请求 RSS 失败: {e}");
            format!("（获取 Rust.cc 资讯失败：{e}）")
        }
    }
}

/// 解析 RSS XML 并格式化为可读文本。
fn parse_and_format(xml: &str) -> String {
    // 简单提取 <item> 块，避免引入 xml 解析依赖
    let mut items: Vec<RssItem> = Vec::new();

    for item_block in extract_item_blocks(xml) {
        let title = extract_tag(&item_block, "title").unwrap_or_else(|| "(无标题)".to_string());
        let link = extract_tag(&item_block, "link").unwrap_or_default();
        let pub_date = extract_tag(&item_block, "pubDate").unwrap_or_default();
        let description = extract_tag(&item_block, "description").unwrap_or_default();

        items.push(RssItem {
            title,
            link,
            pub_date,
            description,
        });
    }

    if items.is_empty() {
        warn!("RSS 解析无结果");
        return "（今日 Rust.cc 没有新资讯）".to_string();
    }

    info!("RSS 共解析 {} 条资讯", items.len());

    let mut buf = String::new();
    buf.push_str(&format!("📰 今日 Rust.cc 资讯（共 {} 条）\n\n", items.len()));

    for (i, item) in items.iter().enumerate() {
        buf.push_str(&format!(
            "[{}] {}\n{}\n{}\n",
            i + 1,
            item.title,
            item.link,
            item.pub_date,
        ));
        if !item.description.is_empty() {
            // 去除 HTML 标签，保留纯文本摘要（最多 200 字符）
            let plain = strip_html(&item.description);
            let truncated: String = plain.chars().take(200).collect();
            if !truncated.is_empty() {
                buf.push_str(&format!("{truncated}\n"));
            }
        }
        buf.push('\n');
    }

    buf
}

/// 从 XML 中提取所有 <item>...</item> 块。
fn extract_item_blocks(xml: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;

    for (i, ch) in xml.char_indices() {
        if ch == '<' {
            let suffix = &xml[i..];
            if suffix.starts_with("<item>") || suffix.starts_with("<item ") {
                if start.is_none() {
                    start = Some(i);
                    depth = 1;
                    continue;
                }
            } else if suffix.starts_with("</item>") {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start.take() {
                            blocks.push(xml[s..i + "</item>".len()].to_string());
                        }
                    }
                }
                continue;
            } else if suffix.starts_with("<item") && !suffix.starts_with("<item>") && !suffix.starts_with("<item ") {
                // 自闭合或其他变体，跳过
            }
        }
        // 嵌套检测：在 item 内部遇到子 tag 时增加 depth
        if let Some(_) = &start {
            if ch == '<' {
                let suffix = &xml[i..];
                if !suffix.starts_with("</") && !suffix.starts_with("<?") && !suffix.starts_with("<!--") {
                    // 简单判断：如果不是结束标签，可能是嵌套开始
                    // 但我们需要区分 <item> 本身和内部的 <title> 等
                    let after_angle = &xml[i+1..];
                    if !after_angle.starts_with("item") && !after_angle.starts_with("/item") {
                        depth += 1;
                    }
                }
            }
        }
    }

    blocks
}

/// 从 XML 片段中提取指定标签的内容。
fn extract_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)?;
    Some(block[start..start + end].to_string())
}

/// 简单去除 HTML 标签。
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut inside = false;
    for ch in html.chars() {
        if ch == '<' {
            inside = true;
        } else if ch == '>' {
            inside = false;
        } else if !inside {
            result.push(ch);
        }
    }
    // 压缩空白
    let collapsed = result.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html() {
        let html = "<p>Hello <b>world</b></p>";
        assert_eq!(strip_html(html), "Hello world");
    }

    #[test]
    fn test_extract_tag() {
        let block = "<item><title>Test Title</title><link>http://example.com</link></item>";
        assert_eq!(extract_tag(block, "title"), Some("Test Title".to_string()));
        assert_eq!(extract_tag(block, "link"), Some("http://example.com".to_string()));
    }
}
