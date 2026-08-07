use crate::error::AppError;
use crate::search;
use dashmap::DashMap;
use ds_api::{Message, Request, Role};
use futures::pin_mut;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{info, warn};

/// 用户意图：要么搜索，要么抓取页面，要么不需要任何操作。
enum Intent {
    Search(String),
    Fetch(String),
    None,
}

#[derive(Clone)]
pub struct DeepseekClient {
    token: Arc<String>,
    system_prompt: Arc<String>,
    http: HttpClient,
    sessions: Arc<DashMap<String, Arc<Mutex<Vec<Message>>>>>,
    max_history: usize,
    web_search_enabled: bool,
}

impl DeepseekClient {
    pub fn new(api_key: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            token: Arc::new(api_key.into()),
            system_prompt: Arc::new(system_prompt.into()),
            http: HttpClient::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("构建 HTTP 客户端失败"),
            sessions: Arc::new(DashMap::new()),
            max_history: 100,
            web_search_enabled: true,
        }
    }

    fn session_history(&self, session_key: &str) -> Arc<Mutex<Vec<Message>>> {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| {
                let mut hist = vec![Message::new(Role::System, &self.system_prompt)];
                if self.web_search_enabled {
                    hist.push(Message::new(
                        Role::System,
                        "\n\n[系统能力] 你具备以下能力：\n\n\
                         1. **联网搜索**：当用户提问涉及实时信息、新闻、最新数据、不确定的事实等需要联网获取的内容时，\
                         你必须在回复中严格按照以下格式输出搜索请求：\n[SEARCH:搜索关键词]\n\n\
                         2. **网页内容抓取**：当用户给你一个链接让你查看内容时，\
                         你必须在回复中严格按照以下格式输出抓取请求：\n[FETCH:完整URL]\n\n\
                         注意：\n\
                         - 你的回复只需要包含上述标记，不需要多余的解释。\n\
                         - 当你看到系统注入的「以下是搜索结果」或「以下是页面内容」后，\
                         必须直接基于这些信息给出最终回答，不要再输出标记。\n\
                         - 如果不需要搜索或抓取，直接正常回复即可。",
                    ));
                }
                Arc::new(Mutex::new(hist))
            })
            .value()
            .clone()
    }

    /// 流式聊天（含自动联网搜索和页面抓取）。
    pub async fn chat_stream<F>(
        &self,
        session_key: &str,
        prompt: &str,
        mut on_delta: F,
    ) -> Result<String, AppError>
    where
        F: FnMut(&str),
    {
        let history = self.session_history(session_key);
        let mut hist = history.lock().await;
        hist.push(Message::new(Role::User, prompt));

        let mut final_reply = self.stream_silent(&mut hist).await?;

        const MAX_ROUNDS: usize = 3;
        let mut rounds = 0;

        loop {
            let intent = extract_intent(&final_reply);

            match intent {
                Intent::None => break,
                Intent::Search(query) => {
                    rounds += 1;
                    if rounds > MAX_ROUNDS {
                        warn!("搜索轮次超过上限 {MAX_ROUNDS}，停止");
                        break;
                    }

                    info!("触发联网搜索({rounds}/{MAX_ROUNDS})：{query}");
                    hist.pop(); // 移除含标记的 assistant 消息

                    // 推送搜索进度提示
                    on_delta(&format!("\n🔍 正在搜索「{}」，请稍后...\n", query));

                    let search_results = search::web_search(&query, 5).await;
                    hist.push(Message::new(
                        Role::System,
                        &format!(
                            "你刚才请求了搜索「{}」。以下是搜索结果：\n\n{}",
                            query, search_results
                        ),
                    ));

                    // 第二轮改为静默收集，避免标记泄漏给用户
                    match timeout(
                        Duration::from_secs(30),
                        self.stream_silent(&mut hist),
                    )
                    .await
                    {
                        Ok(Ok(reply)) => final_reply = reply,
                        Ok(Err(e)) => {
                            warn!("搜索后生成回复失败: {e}");
                            let fallback = format!("关于「{}」的搜索结果如下：\n\n{}", query, search_results);
                            hist.push(Message::new(Role::Assistant, &fallback));
                            on_delta(&fallback);
                            Self::truncate_history(&mut hist, self.max_history);
                            return Ok(fallback);
                        }
                        Err(_) => {
                            warn!("搜索后生成回复超时");
                            let fallback = format!(
                                "搜索「{}」完成，但生成回复超时。以下是搜索结果供参考：\n\n{}",
                                query, search_results
                            );
                            hist.push(Message::new(Role::Assistant, &fallback));
                            on_delta(&fallback);
                            Self::truncate_history(&mut hist, self.max_history);
                            return Ok(fallback);
                        }
                    }
                }
                Intent::Fetch(url) => {
                    rounds += 1;
                    if rounds > MAX_ROUNDS {
                        warn!("抓取轮次超过上限 {MAX_ROUNDS}，停止");
                        break;
                    }

                    info!("触发页面抓取({rounds}/{MAX_ROUNDS})：{url}");
                    hist.pop(); // 移除含标记的 assistant 消息

                    // 推送抓取进度提示
                    on_delta(&format!("\n📄 正在抓取页面，请稍后...\n"));

                    let page_content = search::fetch_url_content(&url).await;
                    hist.push(Message::new(
                        Role::System,
                        &format!(
                            "你刚才请求了抓取页面「{}」。以下是页面内容：\n\n{}",
                            url, page_content
                        ),
                    ));

                    // 第二轮改为静默收集
                    match timeout(
                        Duration::from_secs(30),
                        self.stream_silent(&mut hist),
                    )
                    .await
                    {
                        Ok(Ok(reply)) => final_reply = reply,
                        Ok(Err(e)) => {
                            warn!("抓取后生成回复失败: {e}");
                            let fallback = format!("页面「{}」的内容如下：\n\n{}", url, page_content);
                            hist.push(Message::new(Role::Assistant, &fallback));
                            on_delta(&fallback);
                            Self::truncate_history(&mut hist, self.max_history);
                            return Ok(fallback);
                        }
                        Err(_) => {
                            warn!("抓取后生成回复超时");
                            let fallback = format!(
                                "页面「{}」抓取完成，但生成回复超时。以下是页面内容供参考：\n\n{}",
                                url, page_content
                            );
                            hist.push(Message::new(Role::Assistant, &fallback));
                            on_delta(&fallback);
                            Self::truncate_history(&mut hist, self.max_history);
                            return Ok(fallback);
                        }
                    }
                }
            }
        }

        Self::truncate_history(&mut hist, self.max_history);

        // 清洗残留标记（达到轮次上限时）
        if matches!(extract_intent(&final_reply), Intent::Search(_) | Intent::Fetch(_)) {
            warn!("最终回复仍含操作标记，已清洗");
            final_reply = sanitize_action_marker(&final_reply);
        }

        on_delta(&final_reply);
        Ok(final_reply)
    }

    async fn stream_silent(&self, hist: &mut Vec<Message>) -> Result<String, AppError> {
        let request = Request::basic_query(hist.clone());
        let stream = request
            .execute_client_streaming(&self.http, &self.token)
            .await?;
        pin_mut!(stream);

        let mut full = String::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(c) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                full.push_str(c);
            }
        }

        hist.push(Message::new(Role::Assistant, &full));
        Ok(full)
    }

    fn truncate_history(hist: &mut Vec<Message>, max: usize) {
        let msg_count = hist.len().saturating_sub(1);
        if msg_count > max {
            let to_remove = msg_count - max;
            hist.drain(1..1 + to_remove);
        }
    }
}

/// 从回复中提取意图：搜索、抓取页面、或什么都不做。
fn extract_intent(reply: &str) -> Intent {
    // 先检查 [FETCH:URL]
    if let Some(url) = extract_fetch_url(reply) {
        return Intent::Fetch(url);
    }
    // 再检查 [SEARCH:关键词]
    if let Some(query) = extract_search_query(reply) {
        return Intent::Search(query);
    }
    Intent::None
}

fn extract_search_query(reply: &str) -> Option<String> {
    let start = reply.find("[SEARCH:")?;
    let after = start + "[SEARCH:".len();
    let end = reply[after..].find(']')?;
    let q = reply[after..after + end].trim().to_string();
    if q.is_empty() { None } else { Some(q) }
}

fn extract_fetch_url(reply: &str) -> Option<String> {
    let start = reply.find("[FETCH:")?;
    let after = start + "[FETCH:".len();
    let end = reply[after..].find(']')?;
    let url = reply[after..after + end].trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// 清洗回复中的 `[SEARCH:...]` 和 `[FETCH:...]` 标记。
fn sanitize_action_marker(reply: &str) -> String {
    let mut out = reply.to_string();
    for prefix in &["[SEARCH:", "[FETCH:"] {
        while let Some(start) = out.find(prefix) {
            let after = start + prefix.len();
            if let Some(end_rel) = out[after..].find(']') {
                let end = after + end_rel;
                out.replace_range(start..=end, "");
            } else {
                break;
            }
        }
    }
    out.trim().to_string()
}