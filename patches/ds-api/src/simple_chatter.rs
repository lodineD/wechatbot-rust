//! 简化的聊天客户端模块
//!
//! 提供简单易用的聊天客户端实现，内置历史记录管理功能。
//!
//! # 主要特性
//!
//! - **内置历史记录**: 自动管理对话历史记录
//! - **系统提示词**: 支持自定义系统提示词
//! - **简单易用**: 无需手动管理历史记录
//! - **JSON 响应**: 支持 JSON 格式的响应
//! - **异步处理**: 基于 `tokio` 的异步实现
//!
//! # 示例
//!
//! ## 基本使用
//!
//! ```rust,no_run
//! use ds_api::SimpleChatter;
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let system_prompt = "You are a helpful assistant.".to_string();
//!     let mut chatter = SimpleChatter::new(token, system_prompt);
//!
//!     let response = chatter.chat("What is Rust?").await?;
//!     println!("Assistant: {}", response);
//!
//!     // 继续对话，历史记录会自动维护
//!     let response = chatter.chat("Tell me more about it.").await?;
//!     println!("Assistant: {}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 使用 JSON 响应
//!
//! ```rust,no_run
//! use ds_api::SimpleChatter;
//! use serde_json::Value;
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let system_prompt = "You are a helpful assistant that responds in JSON format.".to_string();
//!     let mut chatter = SimpleChatter::new(token, system_prompt);
//!
//!     let json_response = chatter.chat_json("Give me information about Paris in JSON format").await?;
//!     println!("JSON response: {}", serde_json::to_string_pretty(&json_response)?);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 修改系统提示词
//!
//! ```rust,no_run
//! use ds_api::SimpleChatter;
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!     let system_prompt = "You are a helpful assistant.".to_string();
//!     let mut chatter = SimpleChatter::new(token, system_prompt);
//!
//!     // 修改系统提示词
//!     if let Some(prompt) = chatter.system_prompt_mut() {
//!         *prompt = "You are a sarcastic assistant.".to_string();
//!     }
//!
//!     let response = chatter.chat("What is the weather like?").await?;
//!     println!("Assistant: {}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! # 注意事项
//!
//! - 当前实现不支持流式响应
//! - 历史记录会不断增长，需要手动管理或实现自动截断
//! - 系统提示词是历史记录中的第一条消息
//!

use crate::error::Result;

use crate::{normal_chatter::NormalChatter, request::*};

/// 简化的聊天客户端，内置历史记录管理
///
/// 这个结构体提供了简单易用的聊天接口，自动管理对话历史记录，
/// 用户无需关心历史记录的存储和传递。
///
/// # 字段
///
/// - `history`: 对话历史记录，第一条消息是系统提示词
/// - `chatter`: 底层的聊天客户端实例
///
/// # 设计说明
///
/// - 历史记录中的第一条消息是系统提示词（System Prompt）
/// - 后续消息是用户（User）和助手（Assistant）的对话内容
/// - 每次聊天都会自动更新历史记录
/// - 历史记录会不断增长，需要注意上下文长度限制
///
/// # 使用建议
///
/// 对于简单的聊天应用，推荐使用 `SimpleChatter`，因为它提供了最简单易用的接口。
/// 对于需要自定义历史记录管理的复杂应用，请使用 [`NormalChatter`]。
///
pub struct SimpleChatter {
    /// 对话历史记录
    ///
    /// 包含系统提示词和所有用户与助手的对话消息。
    /// 第一条消息通常是系统提示词。
    pub history: Vec<Message>,

    /// 底层的聊天客户端实例
    ///
    /// 用于实际发送请求和处理响应。
    pub chatter: NormalChatter,
}

impl SimpleChatter {
    /// 创建一个新的 `SimpleChatter` 实例
    ///
    /// # 参数
    ///
    /// * `token` - DeepSeek API 访问令牌
    /// * `system_prompt` - 系统提示词，用于定义助手的行为和角色
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ds_api::SimpleChatter;
    ///
    /// let token = "your_deepseek_api_token".to_string();
    /// let system_prompt = "You are a helpful assistant.".to_string();
    /// let chatter = SimpleChatter::new(token, system_prompt);
    /// ```
    pub fn new(token: String, system_prompt: String) -> Self {
        Self {
            history: vec![Message::new(Role::System, &system_prompt)],
            chatter: NormalChatter::new(token),
        }
    }

    /// 发送聊天消息并获取文本响应
    ///
    /// 这个方法会自动将用户消息添加到历史记录中，发送请求到 DeepSeek API，
    /// 然后将助手的响应也添加到历史记录中，最后返回响应文本。
    ///
    /// # 参数
    ///
    /// * `user_message` - 用户消息内容
    ///
    /// # 返回
    ///
    /// 返回助手的响应文本，如果发生错误则返回错误信息。
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use ds_api::SimpleChatter;
    ///
    /// #[tokio::main]
    /// async fn main() -> ds_api::error::Result<()> {
    ///     let token = "your_token".to_string();
    ///     let system_prompt = "You are a helpful assistant.".to_string();
    ///     let mut chatter = SimpleChatter::new(token, system_prompt);
    ///
    ///     let response = chatter.chat("Hello, world!").await?;
    ///     println!("Assistant: {}", response);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn chat<T: AsRef<str>>(&mut self, user_message: T) -> Result<String> {
        self.chatter.chat(user_message, &mut self.history).await
    }

    /// 发送聊天消息并获取 JSON 格式的响应
    ///
    /// 这个方法与 [`SimpleChatter::chat`] 类似，但会启用 JSON 响应模式，并返回解析后的 JSON 值。
    ///
    /// # 参数
    ///
    /// * `user_message` - 用户消息内容
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
    /// use ds_api::SimpleChatter;
    /// use serde_json::Value;
    ///
    /// #[tokio::main]
    /// async fn main() -> ds_api::error::Result<()> {
    ///     let token = "your_token".to_string();
    ///     let system_prompt = "You are a helpful assistant that responds in JSON format.".to_string();
    ///     let mut chatter = SimpleChatter::new(token, system_prompt);
    ///
    ///     let json_response = chatter.chat_json("Give me information about Paris").await?;
    ///     println!("JSON response: {}", serde_json::to_string_pretty(&json_response)?);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn chat_json<T: AsRef<str>>(&mut self, user_message: T) -> Result<serde_json::Value> {
        self.chatter
            .chat_json(user_message, &mut self.history)
            .await
    }

    /// 获取系统提示词的可变引用
    ///
    /// 这个方法允许在运行时修改系统提示词。
    ///
    /// # 返回
    ///
    /// 返回 `Option<&mut String>`：当 `history` 为空或系统提示词不可用时返回 `None`。
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use ds_api::SimpleChatter;
    ///
    /// #[tokio::main]
    /// async fn main() -> ds_api::error::Result<()> {
    ///     let token = "your_token".to_string();
    ///     let system_prompt = "You are a helpful assistant.".to_string();
    ///     let mut chatter = SimpleChatter::new(token, system_prompt);
    ///
    ///     // 修改系统提示词（安全用法）
    ///     if let Some(prompt) = chatter.system_prompt_mut() {
    ///         *prompt = "You are a sarcastic assistant.".to_string();
    ///     }
    ///
    ///     let response = chatter.chat("What is the weather like?").await?;
    ///     println!("Assistant: {}", response);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn system_prompt_mut(&mut self) -> Option<&mut String> {
        self.history.get_mut(0).and_then(|m| m.content.as_mut())
    }
}
