use crate::error::AppError;
use crate::search;
use dashmap::DashMap;
use ds_api::{Message, Request, Role};
use futures::pin_mut;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// DeepSeek 客户端封装。
///
/// 按 `session_key` 隔离对话历史，支持流式调用。
/// 自动检测模型是否请求联网搜索（`[SEARCH:xxx]`），
/// 若检测到则执行搜索并将结果注入历史后重新生成回答。
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
            http: HttpClient::new(),
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
    ///
    /// 流程：
    /// 1. 追加用户消息 → 静默调用 DeepSeek（不推给用户）。
    /// 2. 检查回复是否含 `[SEARCH:xxx]`。
    ///    - 不含 → 将回复追加到历史，通过 `on_delta` 推给用户。
    ///    - 含 → 移除该 assistant 消息，执行搜索，注入搜索结果，再次静默调用，推最终回复。
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

        // 第一轮：静默收集（不推给用户），检查是否需要搜索
        let first_reply = self.call_stream_silent(&mut hist).await?;

        if let Some(search_query) = extract_search_query(&first_reply) {
            info!("触发联网搜索：{search_query}");

            // 移除刚刚推入的 `[SEARCH:...]` assistant 消息，不在历史中留下痕迹
            hist.pop();

            // 执行搜索
            let search_results = search::web_search(&search_query, 5).await;

            // 注入搜索结果作为 system 消息
            hist.push(Message::new(
                Role::System,
                &format!(
                    "你刚才请求了搜索「{}」。以下是搜索结果：\n\n{}",
                    search_query, search_results
                ),
            ));

            // 第二轮：静默生成最终回复
            let final_reply = self.call_stream_silent(&mut hist).await?;

            // 截断历史
            Self::truncate_history(&mut hist, self.max_history);

            // 一次性推给用户
            on_delta(&final_reply);

            return Ok(final_reply);
        }

        // 无搜索标记，把第一轮回复推给用户
        // 注意：call_stream_silent 已经 push 了 assistant 消息到 hist
        Self::truncate_history(&mut hist, self.max_history);
        on_delta(&first_reply);
        Ok(first_reply)
    }

    /// 静默执行一轮流式调用（不调用 on_delta，不推送消息），
    /// 返回完整回复文本，并自动将回复追加到历史。
    async fn call_stream_silent(&self, hist: &mut Vec<Message>) -> Result<String, AppError> {
        let request = Request::basic_query(hist.clone());
        let stream = request
            .execute_client_streaming(&self.http, &self.token)
            .await?;
        pin_mut!(stream);

        let mut full_reply = String::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                full_reply.push_str(content);
            }
        }

        hist.push(Message::new(Role::Assistant, &full_reply));
        Ok(full_reply)
    }

    fn truncate_history(hist: &mut Vec<Message>, max: usize) {
        let msg_count = hist.len().saturating_sub(1);
        if msg_count > max {
            let to_remove = msg_count - max;
            hist.drain(1..1 + to_remove);
        }
    }
}

/// 从回复文本中提取搜索关键词。
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