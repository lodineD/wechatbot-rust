use crate::error::AppError;
use crate::search;
use dashmap::DashMap;
use ds_api::{Message, Request, Role};
use futures::pin_mut;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::sync::Arc;
use tokio::sync::Mutex;

/// DeepSeek 客户端封装。
///
/// 按 `session_key` 隔离对话历史，支持流式调用。
/// 自动检测 DeepSeek 是否请求联网搜索，若检测到 `[SEARCH:xxx]` 标记，
/// 则执行搜索并将结果注入历史后重新生成回复。
#[derive(Clone)]
pub struct DeepseekClient {
    token: Arc<String>,
    system_prompt: Arc<String>,
    http: HttpClient,
    /// 会话历史表：key = "chatid:userid"，value = 并发安全的对话历史。
    sessions: Arc<DashMap<String, Arc<Mutex<Vec<Message>>>>>,
    /// 单会话历史最大消息数，超出后截断最早的 user/assistant 对。
    max_history: usize,
    /// 是否启用自动联网搜索。
    web_search_enabled: bool,
}

impl DeepseekClient {
    /// 创建新的 DeepSeek 客户端。
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

    /// 获取（或创建）指定会话的对话历史。
    fn session_history(&self, session_key: &str) -> Arc<Mutex<Vec<Message>>> {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| {
                let mut hist = vec![
                    Message::new(Role::System, &self.system_prompt),
                ];
                // 如果启用联网搜索，追加搜索能力说明
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
    /// 1. 追加用户消息 → 调用 DeepSeek，通过 `on_delta` 推送增量。
    /// 2. 检查回复是否包含 `[SEARCH:xxx]`。
    /// 3. 若包含 → 执行搜索 → 将搜索结果作为 system 消息注入历史 → 再次静默调用 DeepSeek → 一次性推送最终回复。
    /// 4. 返回最终回复文本。
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

        // 追加用户消息
        hist.push(Message::new(Role::User, prompt));

        // 第一轮：调用 DeepSeek（流式推送）
        let first_reply = self.call_stream(&mut hist, &mut on_delta).await?;

        // 检查是否包含搜索标记
        if let Some(search_query) = extract_search_query(&first_reply) {
            // 执行搜索
            let search_results = search::web_search(&search_query, 5).await;

            // 注入搜索结果的 system 消息
            hist.push(Message::new(
                Role::System,
                &format!(
                    "你刚才请求了搜索「{}」。以下是搜索结果：\n\n{}",
                    search_query, search_results
                ),
            ));

            // 第二阶段：静默生成（不推送 delta），基于搜索结果重新回答
            let final_reply = self.call_stream_silent(&mut hist).await?;

            // 截断历史
            Self::truncate_history(&mut hist, self.max_history);

            // 一次性推送最终回复给用户
            on_delta(&final_reply);

            return Ok(final_reply);
        }

        // 无搜索标记，直接返回第一轮回复
        Self::truncate_history(&mut hist, self.max_history);
        Ok(first_reply)
    }

    /// 执行一轮流式调用，将增量内容通过 `on_delta` 推送出去，返回完整回复文本。
    async fn call_stream<F>(
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

        let mut full_reply = String::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                full_reply.push_str(content);
                on_delta(content);
            }
        }

        hist.push(Message::new(Role::Assistant, &full_reply));
        Ok(full_reply)
    }

    /// 执行一轮流式调用，但不推送 delta（静默生成），返回完整回复文本。
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

    /// 截断历史：保留 system 提示词，超出上限时移除最早的 user/assistant 对。
    fn truncate_history(hist: &mut Vec<Message>, max: usize) {
        let msg_count = hist.len().saturating_sub(1);
        if msg_count > max {
            let to_remove = msg_count - max;
            let drain_end = 1 + to_remove;
            hist.drain(1..drain_end);
        }
    }
}

/// 从回复文本中提取搜索关键词。
///
/// 匹配格式：`[SEARCH:关键词]`，返回关键词部分（去除首尾空白）。
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