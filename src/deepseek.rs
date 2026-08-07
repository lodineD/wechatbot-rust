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
use tracing::info;

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
                        "\n\n[系统能力] 你具备联网搜索能力。当用户提问涉及实时信息、新闻、最新数据、不确定的事实等需要联网获取的内容时，\
                         你必须在回复中严格按照以下格式输出搜索请求：\n[SEARCH:搜索关键词]\n\n\
                         你的回复只需要包含搜索关键词，不需要多余的解释。搜索完成后，你会看到搜索结果，\
                         再基于搜索结果给出最终回答。如果不需要搜索，直接正常回复即可。",
                    ));
                }
                Arc::new(Mutex::new(hist))
            })
            .value()
            .clone()
    }

    /// 流式聊天（含自动联网搜索）。
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

        // 第一轮：静默收集，检查是否需要搜索
        let first_reply = self.stream_silent(&mut hist).await?;

        if let Some(search_query) = extract_search_query(&first_reply) {
            info!("触发联网搜索：{search_query}");

            // 移除含 [SEARCH:...] 的 assistant 消息
            hist.pop();

            // 执行搜索
            let search_results = search::web_search(&search_query, 5).await;

            // 注入搜索结果
            hist.push(Message::new(
                Role::System,
                &format!(
                    "你刚才请求了搜索「{}」。以下是搜索结果：\n\n{}",
                    search_query, search_results
                ),
            ));

            // 第二轮：流式推送（带超时）
            match timeout(
                Duration::from_secs(30),
                self.stream_with_delta(&mut hist, &mut on_delta),
            )
            .await
            {
                Ok(Ok(final_reply)) => {
                    Self::truncate_history(&mut hist, self.max_history);
                    return Ok(final_reply);
                }
                Ok(Err(_e)) => {
                    // 第二轮调用失败，把搜索结果直接返回作为兜底
                    let fallback = format!(
                        "关于「{}」的搜索结果如下：\n\n{}",
                        search_query, search_results
                    );
                    hist.push(Message::new(Role::Assistant, &fallback));
                    Self::truncate_history(&mut hist, self.max_history);
                    on_delta(&fallback);
                    return Ok(fallback);
                }
                Err(_) => {
                    // 超时
                    let fallback = format!(
                        "搜索「{}」完成，但生成回复超时。以下是搜索结果供参考：\n\n{}",
                        search_query, search_results
                    );
                    hist.push(Message::new(Role::Assistant, &fallback));
                    Self::truncate_history(&mut hist, self.max_history);
                    on_delta(&fallback);
                    return Ok(fallback);
                }
            }
        }

        // 无搜索标记：直接推送第一轮回复
        Self::truncate_history(&mut hist, self.max_history);
        on_delta(&first_reply);
        Ok(first_reply)
    }

    /// 静默执行一轮流式调用（不调用 on_delta），返回完整回复并自动追加到历史。
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

    /// 执行一轮流式调用，通过 `on_delta` 推送增量，返回完整回复并追加到历史。
    async fn stream_with_delta<F>(
        &self,
        hist: &mut Vec<Message>,
        on_delta: &mut F,
    ) -> Result<String, AppError>
    where
        F: FnMut(&str),
    {
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
                on_delta(c);
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

fn extract_search_query(reply: &str) -> Option<String> {
    let start = reply.find("[SEARCH:")?;
    let after_start = start + "[SEARCH:".len();
    let end = reply[after_start..].find(']')?;
    let query = reply[after_start..after_start + end].trim().to_string();
    if query.is_empty() {
        return None;
    }
    Some(query)
}