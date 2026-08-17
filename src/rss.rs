use chrono::{Local, NaiveDate};
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
/// 只保留发布日期为当天的资讯。
pub async fn fetch_rust_news(http: &HttpClient) -> String {
    let rss_url = "https://rustcc.cn/rss";
    match http.get(rss_url).send().await {
        Ok(response) => match response.text().await {
            Ok(body) => {
                let today = Local::now().date_naive();
                let items = parse_items(&body);
                let today_items: Vec<_> = items
                    .into_iter()
                    .filter(|item| parse_pub_date(&item.pub_date).map(|d| d == today).unwrap_or(false))
                    .collect();
                format_items(&today_items)
            }
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

/// 解析 RSS XML 为资讯列表。
fn parse_items(xml: &str) -> Vec<RssItem> {
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

    items
}

/// 解析 pubDate 字符串为 NaiveDate。
/// rustcc.cn 的格式为 "2026-08-12 01:05:12"。
fn parse_pub_date(date_str: &str) -> Option<NaiveDate> {
    // 先按空格截取日期部分，再用 chrono 解析
    let date_part = date_str.split_whitespace().next().unwrap_or(date_str);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

/// 将资讯列表格式化为可读文本。
fn format_items(items: &[RssItem]) -> String {
    if items.is_empty() {
        warn!("RSS 解析无结果");
        return "今天没有日报，好好休息吧，主人(◍•ᴗ•)ﾉ♡".to_string();
    }

    info!("RSS 共解析 {} 条今日资讯", items.len());

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
            // 去除 HTML 标签，保留纯文本摘要
            let plain = strip_html(&item.description);
            let summary = smart_truncate(&plain, 500);
            if !summary.is_empty() {
                buf.push_str(&format!("{summary}\n"));
            }
        }
        buf.push('\n');
    }

    buf
}

/// 从 XML 中提取所有 <item>...</item> 块。
fn extract_item_blocks(xml: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut search_from = 0;

    while let Some(start) = xml[search_from..].find("<item>") {
        let absolute_start = search_from + start;
        if let Some(end) = xml[absolute_start..].find("</item>") {
            let absolute_end = absolute_start + end + "</item>".len();
            blocks.push(xml[absolute_start..absolute_end].to_string());
            search_from = absolute_end;
        } else {
            break;
        }
    }

    blocks
}

/// 从 XML 片段中提取指定标签的内容（支持 CDATA）。
fn extract_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)?;
    let mut value = block[start..start + end].to_string();

    // 去除 CDATA 包装
    if value.starts_with("<![CDATA[") && value.ends_with("]]>") {
        value = value["<![CDATA[".len()..value.len() - "]]>".len()].to_string();
    }

    Some(value)
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

/// 智能截断文本：按字符数限制截断，但优先在单词/标点边界处切断，
/// 避免中文句子被截在半句话，并以 "..." 提示后续还有内容。
fn smart_truncate(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    // 先尝试在 max_chars 范围内找最后一个句末标点
    let boundary_chars = ['。', '．', '.', '！', '!', '？', '?', '\n'];
    let mut last_boundary: Option<usize> = None;
    let mut current = 0;
    for (idx, ch) in text.char_indices() {
        if current >= max_chars {
            break;
        }
        if boundary_chars.contains(&ch) {
            last_boundary = Some(idx + ch.len_utf8());
        }
        current += 1;
    }

    if let Some(pos) = last_boundary {
        let truncated = &text[..pos];
        return format!("{truncated} ...");
    }

    // 没有标点，退化为在 max_chars 处截断并补 "..."
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated} ...")
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

    #[test]
    fn test_extract_tag_cdata() {
        let block = "<item><description><![CDATA[<p>Hello <b>world</b></p>]]></description></item>";
        assert_eq!(extract_tag(block, "description"), Some("<p>Hello <b>world</b></p>".to_string()));
    }

    #[test]
    fn test_parse_and_format_real_rss() {
        let xml = r#"<rss version="2.0"><channel><item><title>【Rust日报】2026-08-12 Bevy 六周年</title><link>https://rustcc.cn/article?id=c3f5a647-2f22-4fa0-9b7d-067fb6635c59</link><description><![CDATA[<h2>Thermite SIMD 0.2.0</h2><p>面向单机高性能计算。</p>]]></description><pubDate>2026-08-12 01:05:12</pubDate></item><item><title>RMQTT 版本更新</title><link>https://rustcc.cn/article?id=0a1ec7c5</link><description><![CDATA[<p>主要更新内容...</p>]]></description><pubDate>2026-08-11 14:32:35</pubDate></item></channel></rss>"#;

        let today = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
        let items = parse_items(xml);
        let today_items: Vec<_> = items
            .into_iter()
            .filter(|item| parse_pub_date(&item.pub_date).map(|d| d == today).unwrap_or(false))
            .collect();
        let output = format_items(&today_items);
        println!("{}", output);

        assert!(output.contains("Rust.cc 资讯（共 1 条）"), "应该只解析出 1 条今日资讯");
        assert!(output.contains("Bevy 六周年"), "应包含今日标题");
        assert!(!output.contains("RMQTT 版本更新"), "不应包含昨日资讯");
    }

    #[tokio::test]
    #[ignore = "需要网络，手动执行: cargo test rss -- --ignored --nocapture"]
    async fn test_fetch_rust_news_real() {
        let client = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        let content = fetch_rust_news(&client).await;
        println!("=== 真实 RSS 抓取结果 ===");
        println!("{}", content);
        println!("=== 长度: {} 字符 ===", content.chars().count());

        assert!(
            !content.contains("今天没有日报，好好休息吧，主人"),
            "真实 RSS 应该有内容，但解析为空"
        );
        assert!(
            content.contains("Rust.cc 资讯"),
            "输出应包含标题"
        );
    }
}
