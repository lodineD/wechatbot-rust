use crate::error::AppError;
use dashmap::DashMap;
use ds_api::{Message, Request, Role};
use futures::pin_mut;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use std::sync::Arc;
use tokio::sync::Mutex;

/// DeepSeek 客户端封装。
///
/// 按 `session_key` 隔离对话历史，支持流式和非流式调用。
/// 每个会话维护独立的 `Vec<Message>`，第一条为 system 提示词。
#[derive(Clone)]
pub struct DeepseekClient {
    token: Arc<String>,
    system_prompt: Arc<String>,
    http: HttpClient,
    /// 会话历史表：key = "chatid:userid"，value = 并发安全的对话历史。
    sessions: Arc<DashMap<String, Arc<Mutex<Vec<Message>>>>>,
    /// 单会话历史最大消息数，超出后截断最早的 user/assistant 对。
    max_history: usize,
}

impl DeepseekClient {
    /// 创建新的 DeepSeek 客户端。
    pub fn new(api_key: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            token: Arc::new(api_key.into()),
            system_prompt: Arc::new(system_prompt.into()),
            http: HttpClient::new(),
            sessions: Arc::new(DashMap::new()),
            max_history: 20,
        }
    }

    /// 获取（或创建）指定会话的对话历史。
    fn session_history(&self, session_key: &str) -> Arc<Mutex<Vec<Message>>> {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| {
                Arc::new(Mutex::new(vec![Message::new(
                    Role::System,
                    &self.system_prompt,
                )]))
            })
            .value()
            .clone()
    }

    /// 流式聊天。
    ///
    /// - `session_key`：会话标识，用于隔离历史。
    /// - `prompt`：用户消息。
    /// - `on_delta`：每次收到增量内容时调用。
    ///
    /// 返回完整回复文本。流结束后自动更新历史。
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

        // 构建流式请求
        let request = Request::basic_query(hist.clone());

        let stream =
            request.execute_client_streaming(&self.http, &self.token).await?;

        // 返回的 Stream 是 !Unpin 的，需要 pin 到栈上再 .next()
        pin_mut!(stream);

        let mut full_reply = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                full_reply.push_str(content);
                on_delta(content);
            }
        }

        // 追加助手回复
        hist.push(Message::new(Role::Assistant, &full_reply));

        // 截断历史
        Self::truncate_history(&mut hist, self.max_history);

        Ok(full_reply)
    }

    /// 截断历史：保留 system 提示词，超出上限时移除最早的 user/assistant 对。
    fn truncate_history(hist: &mut Vec<Message>, max: usize) {
        // 第一条是 system，不计入上限
        let msg_count = hist.len().saturating_sub(1);
        if msg_count > max {
            let to_remove = msg_count - max;
            // 跳过头部的 system 消息，移除最早的消息对
            let drain_end = 1 + to_remove; // 保留 system（索引0），移除 to_remove 条
            hist.drain(1..drain_end);
        }
    }
}