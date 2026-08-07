//! # DeepSeek API Rust Client
//!
//! 一个功能完整的 Rust 客户端库，用于与 DeepSeek API 进行交互。
//!
//! ## 特性
//!
//! - **完整的 API 支持**: 支持 DeepSeek API 的所有功能
//! - **类型安全**: 使用 Rust 的强类型系统确保 API 请求和响应的正确性
//! - **异步支持**: 基于 `tokio` 和 `reqwest` 的异步实现
//! - **流式响应**: 支持 Server-Sent Events (SSE) 流式响应
//! - **工具调用**: 支持函数调用和工具选择
//! - **JSON 模式**: 支持 JSON 格式的响应
//! - **推理模式**: 支持 DeepSeek Reasoner 模型的推理功能
//!
//! ## 快速开始
//!
//! ```no_run
//! use ds_api::{Request, Message, Role, Response};
//!
//! #[tokio::main]
//! async fn main() -> ds_api::error::Result<()> {
//!     let token = "your_deepseek_api_token".to_string();
//!
//!     let request = Request::basic_query(vec![
//!         Message::new(Role::User, "Hello, how are you?")
//!     ]);
//!
//!     let response = request.execute_nostreaming(&token).await?;
//!     // 注意：`Response::content()` 现在返回 `Result<&str, ApiError>`，需处理潜在错误
//!     let content = response.content()?;
//!     println!("Response: {}", content);
//!     Ok(())
//! }
//! ```
//!
//! ## 模块概览
//!
//! - [`request`]
//! - [`response`]
//! - [`normal_chatter`]
//! - [`simple_chatter`]
//! - [`raw`]
//!
//! ## 更多示例
//!
//! 查看各个模块的文档和 `examples/` 目录获取更多使用示例。

pub mod error;
pub mod normal_chatter;
pub mod raw;
pub mod request;
pub mod response;
pub mod simple_chatter;

/// 重新导出常用的类型，方便用户使用
pub use normal_chatter::{History, NormalChatter};
pub use request::Request;
pub use response::Response;
pub use simple_chatter::SimpleChatter;

/// 重新导出原始数据结构
pub use raw::*;
