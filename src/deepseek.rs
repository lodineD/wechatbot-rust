use crate::error::AppError;
use ds_api::SimpleChatter;
use std::sync::Arc;
use tokio::sync::Mutex;

/// DeepSeek 客户端封装。
///
/// 内部使用 `ds_api::SimpleChatter` 维护对话历史，并通过 `tokio::sync::Mutex`
/// 保证在多个并发消息回调中可以安全共享。
#[derive(Clone)]
pub struct DeepseekClient {
    chatter: Arc<Mutex<SimpleChatter>>,
}

impl DeepseekClient {
    /// 创建新的 DeepSeek 客户端。
    ///
    /// 注意：`ds-api 0.2.0` 的 `SimpleChatter::new` 需要两个参数：
    /// `(api_key, system_prompt)`。
    pub fn new(api_key: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        let chatter = SimpleChatter::new(api_key.into(), system_prompt.into());
        Self {
            chatter: Arc::new(Mutex::new(chatter)),
        }
    }

    /// 发送单条用户消息并返回 DeepSeek 的文本回复。
    pub async fn chat(&self, prompt: &str) -> Result<String, AppError> {
        let mut chatter = self.chatter.lock().await;
        let reply = chatter.chat(prompt).await?;
        Ok(reply)
    }
}
