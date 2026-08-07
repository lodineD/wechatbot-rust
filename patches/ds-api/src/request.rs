//! 高级请求构建器模块
//!
//! 提供类型安全的 API 请求构建器，简化与 DeepSeek API 的交互。
//!
//! # 主要类型
//!
//! - [`Request`]: 主要的请求构建器，提供流畅的 API 来构建聊天补全请求
//!
//! # 示例
//!
//! ## 基本使用
//!
//! ```rust
//! use ds_api::{Request, Message, Role};
//!
//! let request = Request::basic_query(vec![
//!     Message::new(Role::User, "Hello, world!")
//! ]);
//! ```
//!
//! ## 使用构建器模式
//!
//! ```rust
//! use ds_api::{Request, Message, Role};
//!
//! let request = Request::builder()
//!     .add_message(Message::new(Role::System, "You are a helpful assistant."))
//!     .add_message(Message::new(Role::User, "What is Rust?"))
//!     .temperature(0.7)
//!     .max_tokens(100);
//! ```
//!
//! ## 流式响应
//!
//! ```rust,no_run
//! use ds_api::{Request, Message, Role};
//! use futures::StreamExt;
//!
//! # #[tokio::main]
//! # async fn main() -> ds_api::error::Result<()> {
//! let token = "your_token".to_string();
//! let client = reqwest::Client::new();
//!
//! let request = Request::basic_query(vec![
//!     Message::new(Role::User, "Tell me a story.")
//! ]);
//!
//! let stream = request.execute_client_streaming(&client, &token).await?;
//!
//! // 使用 pin_mut! 宏来固定流
//! use futures::pin_mut;
//! pin_mut!(stream);
//!
//! while let Some(chunk_result) = stream.next().await {
//!     match chunk_result {
//!         Ok(chunk) => {
//!             if let Some(content) = chunk.choices[0].delta.content.as_ref() {
//!                 print!("{}", content);
//!             }
//!         }
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{ApiError, Result};
pub use crate::raw::*;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;

/// 默认 API base url（不包含版本前缀，路径将使用 /chat/completions）
const DEFAULT_API_BASE: &str = "https://api.deepseek.com";

/// 一个发送至 Deepseek API 的请求对象，封装了原始请求数据。
/// 该结构体保证请求合法
pub struct Request {
    raw: ChatCompletionRequest,
}

impl Request {
    /// 创建一个基本的聊天请求，使用 DeepseekChat 模型。
    /// 参数 `messages` 是一个消息列表，表示对话的上下文。
    /// example:
    /// ```
    /// use ds_api::request::message::Role;
    /// use ds_api::request::Message;
    /// use ds_api::request::Request;
    /// let request = Request::basic_query(vec![
    ///    Message::new(Role::User, "What is the capital of France?")
    /// ]);
    /// ```
    pub fn basic_query(messages: Vec<Message>) -> Self {
        Self::builder()
            .messages(messages)
            .model(Model::deepseek_v4_flash())
    }

    /// 创建一个基本的聊天请求，使用 DeepseekReasoner 模型。
    /// 参数 `messages` 是一个消息列表，表示对话的上下文。
    /// example:
    /// ```
    /// use ds_api::request::message::Role;
    /// use ds_api::request::Message;
    /// use ds_api::request::Request;
    /// let request = Request::basic_query_reasoner(vec![
    ///    Message::new(Role::User, "What is the capital of France?")
    /// ]);
    /// ```
    pub fn basic_query_reasoner(messages: Vec<Message>) -> Self {
        Self::builder()
            .messages(messages)
            .model(Model::deepseek_v4_flash())
    }

    pub fn builder() -> Self {
        Self {
            raw: ChatCompletionRequest::default(),
        }
    }

    pub fn add_message(mut self, message: Message) -> Self {
        self.raw.messages.push(message);
        self
    }

    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.raw.messages = messages;
        self
    }

    pub fn model(mut self, model: Model) -> Self {
        self.raw.model = model;
        self
    }

    pub fn response_format_type(mut self, response_format_type: ResponseFormatType) -> Self {
        self.raw.response_format = Some(ResponseFormat {
            r#type: response_format_type,
        });
        self
    }

    pub fn json(self) -> Self {
        self.response_format_type(ResponseFormatType::JsonObject)
    }

    pub fn text(self) -> Self {
        self.response_format_type(ResponseFormatType::Text)
    }

    /// Possible values: >= -2 and <= 2
    /// Default value: 0
    /// 介于 -2.0 和 2.0 之间的数字。如果该值为正，那么新 token 会根据其在已有文本中的出现频率受到相应的惩罚，降低模型重复相同内容的可能性。
    pub fn frequency_penalty(mut self, penalty: f32) -> Self {
        self.raw.frequency_penalty = Some(penalty);
        self
    }

    /// Possible values: >= -2 and <= 2
    /// Default value: 0
    /// 介于 -2.0 和 2.0 之间的数字。如果该值为正，那么新 token 会根据其是否已在已有文本中出现受到相应的惩罚，从而增加模型谈论新主题的可能性。
    pub fn presence_penalty(mut self, penalty: f32) -> Self {
        self.raw.presence_penalty = Some(penalty);
        self
    }

    /// 限制一次请求中模型生成 completion 的最大 token 数。输入 token 和输出 token 的总长度受模型的上下文长度的限制。取值范围与默认值详见文档。
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.raw.max_tokens = Some(max_tokens);
        self
    }

    /// Possible values: <= 2
    /// Default value: 1
    /// 采样温度，介于 0 和 2 之间。更高的值，如 0.8，会使输出更随机，而更低的值，如 0.2，会使其更加集中和确定。 我们通常建议可以更改这个值或者更改 top_p，但不建议同时对两者进行修改。
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.raw.temperature = Some(temperature);
        self
    }

    pub fn stop_vec(mut self, stop: Vec<String>) -> Self {
        self.raw.stop = Some(Stop::Array(stop));
        self
    }

    pub fn stop_str(mut self, stop: String) -> Self {
        self.raw.stop = Some(Stop::String(stop));
        self
    }

    /// Possible values: <= 1
    /// Default value: 1
    /// 作为调节采样温度的替代方案，模型会考虑前 top_p 概率的 token 的结果。所以 0.1 就意味着只有包括在最高 10% 概率中的 token 会被考虑。 我们通常建议修改这个值或者更改 temperature，但不建议同时对两者进行修改。
    pub fn top_p(mut self, top_p: f32) -> Self {
        self.raw.top_p = Some(top_p);
        self
    }

    pub fn add_tool(mut self, tool: Tool) -> Self {
        if let Some(tools) = &mut self.raw.tools {
            tools.push(tool);
        } else {
            self.raw.tools = Some(vec![tool]);
        }
        self
    }

    pub fn tool_choice_type(mut self, tool_choice: ToolChoiceType) -> Self {
        self.raw.tool_choice = Some(ToolChoice::String(tool_choice));
        self
    }

    pub fn tool_choice_object(mut self, tool_choice: ToolChoiceObject) -> Self {
        self.raw.tool_choice = Some(ToolChoice::Object(tool_choice));
        self
    }

    /// top_logprobs: 一个介于 0 到 20 之间的整数 N，指定每个输出位置返回输出概率 top N 的 token，且返回这些 token 的对数概率
    pub fn logprobs(mut self, top_logprobs: u32) -> Self {
        self.raw.logprobs = Some(true);
        self.raw.top_logprobs = Some(top_logprobs);
        self
    }

    pub fn raw(&self) -> &ChatCompletionRequest {
        &self.raw
    }

    /// 执行无流式（non-streaming）请求，使用指定的 `base_url`（会自动追加 `/chat/completions` path）。
    /// 接收一个不可变的 `&reqwest::Client`，避免对外部 client 所有权的要求。
    pub async fn execute_client_baseurl_nostreaming(
        self,
        client: &reqwest::Client,
        base_url: &str,
        token: &str,
    ) -> Result<ChatCompletionResponse> {
        // 构建 url（确保不会重复斜杠）
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let resp = client
            .post(&url)
            .bearer_auth(token)
            .json(&self.raw)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            // 尝试读取响应体文本以便诊断，若读取失败则用错误字符串占位
            let text = resp.text().await.unwrap_or_else(|e| e.to_string());
            return Err(ApiError::http_error(status, text));
        }

        let parsed = resp.json::<ChatCompletionResponse>().await?;
        Ok(parsed)
    }

    /// 便捷方法：使用默认 base URL 执行无流式请求（接收不可变的 `&Client`）。
    pub async fn execute_client_nostreaming(
        self,
        client: &reqwest::Client,
        token: &str,
    ) -> Result<ChatCompletionResponse> {
        self.execute_client_baseurl_nostreaming(client, DEFAULT_API_BASE, token)
            .await
    }

    /// 在没有外部 `Client` 的情况下执行无流式请求（使用传入的 `base_url`）。
    pub async fn execute_baseurl_nostreaming(
        self,
        base_url: &str,
        token: &str,
    ) -> Result<ChatCompletionResponse> {
        let client = reqwest::Client::new();
        self.execute_client_baseurl_nostreaming(&client, base_url, token)
            .await
    }

    /// 使用默认 base URL 执行无流式请求（最常用的便捷方法）。
    pub async fn execute_nostreaming(self, token: &str) -> Result<ChatCompletionResponse> {
        self.execute_baseurl_nostreaming(DEFAULT_API_BASE, token)
            .await
    }

    /// 执行流式（SSE）响应请求。使用 `DEFAULT_API_BASE` 构建 URL，并对流中可能出现的解析错误进行更加明确的映射。
    ///
    /// 返回一个 Stream，每个 Item 是 `Result<ChatCompletionChunk, ApiError>`。
    /// Execute a streaming request (SSE) using a custom `base_url`.
    /// 返回一个 Stream，每个 Item 是 `Result<ChatCompletionChunk, ApiError>`。
    pub async fn execute_client_streaming_baseurl(
        mut self,
        client: &reqwest::Client,
        base_url: &str,
        token: &str,
    ) -> Result<impl Stream<Item = std::result::Result<ChatCompletionChunk, ApiError>>> {
        self.raw.stream = Some(true); // 确保请求中包含 stream: true

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let response = client
            .post(&url)
            .bearer_auth(token)
            .json(&self.raw)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|e| e.to_string());
            return Err(ApiError::http_error(status, error_text));
        }

        // 将响应字节流转换为 SSE 事件流
        let event_stream = response.bytes_stream().eventsource();

        // 映射每个事件：
        // - 如果是 Ok(event)，判断 event.data：
        //   - 若 data == "[DONE]"，忽略（返回 None）
        //   - 否则尝试反序列化为 ChatCompletionChunk
        //     - 若解析成功 -> Some(Ok(chunk))
        //     - 若解析失败 -> Some(Err(ApiError::Json(...)))
        // - 如果 eventsource 返回错误 -> Some(Err(ApiError::Other(...)))
        let chunk_stream = event_stream.filter_map(|event_result| async move {
            match event_result {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        None
                    } else {
                        match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
                            Ok(chunk) => Some(Ok(chunk)),
                            Err(e) => Some(Err(ApiError::Json(e))),
                        }
                    }
                }
                Err(e) => Some(Err(ApiError::EventSource(e.to_string()))),
            }
        });

        Ok(chunk_stream)
    }

    /// Execute a streaming request (SSE) using the default API base URL.
    /// Convenience wrapper that delegates to `execute_client_streaming_baseurl`.
    pub async fn execute_client_streaming(
        self,
        client: &reqwest::Client,
        token: &str,
    ) -> Result<impl Stream<Item = std::result::Result<ChatCompletionChunk, ApiError>>> {
        self.execute_client_streaming_baseurl(client, DEFAULT_API_BASE, token)
            .await
    }

    /// # Safety
    /// 该函数允许直接从原始请求数据创建一个 Request 对象，绕过了构建器的合法性检查。调用者必须确保提供的原始数据是合法且符合 API 要求的，否则可能导致请求失败或产生不可预期的行为。
    pub unsafe fn from_raw_unchecked(raw: ChatCompletionRequest) -> Self {
        Self { raw }
    }

    /// # Safety
    /// 该函数返回对原始请求数据的可变引用，允许直接修改请求的各个字段。调用者必须确保在修改过程中保持请求数据的合法性和一致性，以避免产生无效的请求或引发错误。
    pub unsafe fn get_raw_mut(&mut self) -> &mut ChatCompletionRequest {
        &mut self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_world_request() {
        let request = Request::basic_query(vec![Message {
            role: Role::User,
            content: Some("Hello, world!".to_string()),
            ..Default::default()
        }]);

        assert_eq!(request.raw().messages.len(), 1);
        assert_eq!(
            request.raw().messages[0].content.as_ref().unwrap(),
            "Hello, world!"
        );
        assert_eq!(request.raw().model.as_str(), "deepseek-chat");
    }
}
