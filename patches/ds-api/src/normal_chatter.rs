//! 支持自定义历史记录管理的聊天客户端模块
//!
//! 提供灵活的聊天客户端实现，允许用户自定义对话历史记录的存储和管理方式。
//!
//! # 主要特性
//!
//! - **自定义历史记录**: 通过实现 [`History`] trait 来自定义历史记录存储
//! - **聊天功能**: 支持基本的聊天交互
//! - **JSON 响应**: 支持 JSON 格式的响应
//! - **异步处理**: 基于 `tokio` 的异步实现
//!
//! # 示例
//!
//! ## 基本使用
//!
//! ```rust,no_run
//! use ds_api::{NormalChatter, History, Message, Role};
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let mut chatter = NormalChatter::new(token);
//!     let mut history: Vec<Message> = vec![];
//!
//!     let response = chatter.chat("Hello, how are you?", &mut history).await?;
//!     println!("Assistant: {}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 自定义历史记录实现
//!
//! ```rust,no_run
//! use ds_api::{NormalChatter, History, Message, Role};
//!
//! struct LimitedHistory {
//!     messages: Vec<Message>,
//!     max_messages: usize,
//! }
//!
//! impl History for LimitedHistory {
//!     fn add_message(&mut self, message: Message) {
//!         self.messages.push(message);
//!         if self.messages.len() > self.max_messages {
//!             self.messages.remove(0); // 移除最旧的消息
//!         }
//!     }
//!
//!     fn get_history(&self) -> Vec<Message> {
//!         self.messages.clone()
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let mut chatter = NormalChatter::new(token);
//!     let mut history = LimitedHistory {
//!         messages: vec![],
//!         max_messages: 10,
//!     };
//!
//!     let response = chatter.chat("What is Rust?", &mut history).await?;
//!     println!("Assistant: {}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 使用 JSON 响应
//!
//! ```rust,no_run
//! use ds_api::{NormalChatter, History, Message, Role};
//! use serde_json::Value;
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let mut chatter = NormalChatter::new(token);
//!     let mut history: Vec<Message> = vec![
//!         Message::new(Role::System, "You are a helpful assistant that responds in JSON format.")
//!     ];
//!
//!     let json_response = chatter.chat_json("Give me information about Paris in JSON format", &mut history).await?;
//!     println!("JSON response: {}", serde_json::to_string_pretty(&json_response)?);
//!
//!     Ok(())
//! }
//! ```
//!
//! # 注意事项
//!
//! - 当前实现不支持流式响应
//! - 需要手动管理上下文长度，避免超过模型限制
//! - 历史记录中的第一条消息通常是系统提示词
//!

/// 历史记录管理 trait
///
/// 定义对话历史记录的存储和管理接口，允许用户自定义历史记录的存储方式。
///
/// # 实现要求
///
/// 实现者需要提供：
/// - 消息添加功能
/// - 历史记录获取功能
///
/// # 示例
///
/// 使用 `Vec<Message>` 作为历史记录存储：
///
/// ```rust
/// use ds_api::{History, Message, Role};
///
/// let mut history: Vec<Message> = vec![];
/// history.add_message(Message::new(Role::User, "Hello"));
///
/// let messages = history.get_history();
/// assert_eq!(messages.len(), 1);
/// ```
pub trait History {
    /// 添加一条消息到历史记录中
    ///
    /// # 参数
    ///
    /// * `message` - 要添加的消息
    fn add_message(&mut self, message: Message);

    /// 获取完整的历史记录
    ///
    /// 返回历史记录中所有消息的副本。由于需要发送给 API，
    /// 这里直接返回 `Vec<Message>` 而不是迭代器，这样不会带来性能损失。
    ///
    /// # 返回
    ///
    /// 历史记录中所有消息的向量
    fn get_history(&self) -> Vec<Message>;
}

impl History for Vec<Message> {
    fn add_message(&mut self, message: Message) {
        self.push(message);
    }

    fn get_history(&self) -> Vec<Message> {
        self.clone()
    }
}

use crate::error::Result;

use crate::request::*;
use crate::response::Response;
use reqwest::Client;
use serde_json::Value;

/// 支持自定义历史记录管理的聊天客户端
///
/// 这个结构体提供了与 DeepSeek API 交互的基本功能，同时允许用户
/// 通过实现 [`History`] trait 来自定义历史记录的存储和管理方式。
///
/// # 字段
///
/// - `token`: DeepSeek API 访问令牌
/// - `client`: HTTP 客户端，用于发送请求
///
/// # 注意事项
///
/// - 历史记录中的第一条消息通常是系统提示词（System Prompt）
/// - 后续消息是用户（User）或助手（Assistant）的对话内容
/// - 需要手动管理上下文长度，避免超过模型限制
pub struct NormalChatter {
    /// DeepSeek API 访问令牌
    pub token: String,
    /// HTTP 客户端实例
    pub client: Client,
}

impl NormalChatter {
    /// 创建一个新的 `NormalChatter` 实例
    ///
    /// # 参数
    ///
    /// * `token` - DeepSeek API 访问令牌
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ds_api::NormalChatter;
    ///
    /// let token = "your_deepseek_api_token".to_string();
    /// let chatter = NormalChatter::new(token);
    /// ```
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: Client::new(),
        }
    }

    /// 发送聊天消息并获取文本响应
    ///
    /// 这个方法会将用户消息添加到历史记录中，发送请求到 DeepSeek API，
    /// 然后将助手的响应也添加到历史记录中，最后返回响应文本。
    ///
    /// # 参数
    ///
    /// * `user_message` - 用户消息内容
    /// * `history` - 实现了 [`History`] trait 的历史记录管理器
    ///
    /// # 返回
    ///
    /// 返回助手的响应文本，如果发生错误则返回错误信息。
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use ds_api::{NormalChatter, History, Message, Role};
    ///
    /// #[tokio::main]
    /// async fn main() -> ds_api::error::Result<()> {
    ///     let token = "your_token".to_string();
    ///     let mut chatter = NormalChatter::new(token);
    ///     let mut history: Vec<Message> = vec![];
    ///
    ///     let response = chatter.chat("Hello, world!", &mut history).await?;
    ///     println!("Assistant: {}", response);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn chat<T: AsRef<str>>(
        &mut self,
        user_message: T,
        history: &mut impl History,
    ) -> Result<String> {
        let user_message = Message::new(Role::User, user_message.as_ref());
        history.add_message(user_message);

        let response = Request::basic_query(history.get_history())
            .execute_nostreaming(&self.token)
            .await?;

        let assistant_message = response
            .choices
            .get(0)
            .map(|c| c.message.clone())
            .ok_or_else(|| {
                crate::error::ApiError::Other("missing choice or content in response".to_string())
            })?;
        history.add_message(assistant_message);

        Ok(response.content()?.to_string())
    }

    /// 发送聊天消息并获取 JSON 格式的响应
    ///
    /// 这个方法与 [`NormalChatter::chat`] 类似，但会启用 JSON 响应模式，并返回解析后的 JSON 值。
    ///
    /// # 参数
    ///
    /// * `user_message` - 用户消息内容
    /// * `history` - 实现了 [`History`] trait 的历史记录管理器
    ///
    /// # 返回
    ///
    /// 返回解析后的 JSON 值，如果发生错误则返回错误信息。
    ///
    /// # 注意事项
    ///
    /// 使用此方法前，确保在系统提示词中指示模型返回 JSON 格式的响应。
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use ds_api::{NormalChatter, History, Message, Role};
    /// use serde_json::Value;
    ///
    /// #[tokio::main]
    /// async fn main() -> ds_api::error::Result<()> {
    ///     let token = "your_token".to_string();
    ///     let mut chatter = NormalChatter::new(token);
    ///     let mut history: Vec<Message> = vec![
    ///         Message::new(Role::System, "You are a helpful assistant that responds in JSON format.")
    ///     ];
    ///
    ///     let json_response = chatter.chat_json("Give me information about Paris", &mut history).await?;
    ///     println!("JSON response: {}", serde_json::to_string_pretty(&json_response)?);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn chat_json<T: AsRef<str>>(
        &mut self,
        user_message: T,
        history: &mut impl History,
    ) -> Result<Value> {
        let user_message = Message::new(Role::User, user_message.as_ref());
        history.add_message(user_message);

        let response = Request::basic_query(history.get_history())
            .json()
            .execute_nostreaming(&self.token)
            .await?;

        let assistant_message = response
            .choices
            .get(0)
            .map(|c| c.message.clone())
            .ok_or_else(|| {
                crate::error::ApiError::Other("missing choice or content in response".to_string())
            })?;
        history.add_message(assistant_message);

        let value = serde_json::from_str(response.content()?)?;

        Ok(value)
    }
}
