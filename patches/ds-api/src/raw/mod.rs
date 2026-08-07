//! 原始 API 数据结构模块
//!
//! 提供与 DeepSeek API 直接对应的原始数据结构，这些结构体直接映射到 API 的 JSON 格式。
//! 这个模块适合需要直接控制 API 请求和响应的用户。
//!
//! # 模块结构
//!
//! - [`request`]
//! - [`response`]
//!
//! # 主要类型
//!
//! ## 请求类型
//!
//! - [`ChatCompletionRequest`]
//! - [`Message`]
//! - [`Model`]
//! - [`Tool`]
//! - [`ResponseFormat`]
//! - [`Thinking`]
//! - [`Stop`]
//! - [`StreamOptions`]
//!
//! ## 响应类型
//!
//! - [`ChatCompletionResponse`]
//! - [`ChatCompletionChunk`]
//! - [`Choice`]
//! - [`Usage`]
//!
//! # 使用场景
//!
//! 使用原始数据结构适合以下场景：
//!
//! 1. **需要完全控制 API 请求**：直接设置所有字段
//! 2. **高级功能使用**：如工具调用、推理模式等
//! 3. **性能优化**：避免构建器的开销
//! 4. **与现有代码集成**：直接使用 serde 序列化/反序列化
//!
//! # 示例
//!
//! ## 基本使用
//!
//! ```rust
//! use ds_api::raw::{ChatCompletionRequest, Message, Model, Role};
//! use serde_json::json;
//!
//! fn main() {
//!     // 示例 1: 基本聊天补全请求
//!     let basic_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message::new(Role::System, "You are a helpful assistant."),
//!             Message::new(Role::User, "What is the capital of France?"),
//!         ],
//!         model: Model::DeepseekChat,
//!         max_tokens: Some(100),
//!         temperature: Some(0.7),
//!         stream: Some(false),
//!         ..Default::default()
//!     };
//!
//!     println!("Basic request JSON:");
//!     let json = serde_json::to_string_pretty(&basic_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // 示例 2: 带工具调用的请求
//!     use ds_api::raw::{Tool, ToolChoice, ToolChoiceType, ToolType, Function};
//!
//!     let tool_request = ChatCompletionRequest {
//!         messages: vec![Message::new(Role::User, "What's the weather like in Tokyo?")],
//!         model: Model::DeepseekChat,
//!         max_tokens: Some(200),
//!         temperature: Some(0.8),
//!         stream: Some(false),
//!         tools: Some(vec![Tool {
//!             r#type: ToolType::Function,
//!             function: Function {
//!                 name: "get_weather".to_string(),
//!                 description: Some("Get the current weather for a location".to_string()),
//!                 parameters: json!({
//!                     "type": "object",
//!                     "properties": {
//!                         "location": {
//!                             "type": "string",
//!                             "description": "The city and country, e.g. Tokyo, Japan"
//!                         }
//!                     },
//!                     "required": ["location"]
//!                 }),
//!                 strict: Some(true),
//!             },
//!         }]),
//!         tool_choice: Some(ToolChoice::String(ToolChoiceType::Auto)),
//!         ..Default::default()
//!     };
//!
//!     println!("Tool request JSON:");
//!     let json = serde_json::to_string_pretty(&tool_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // 示例 3: 使用 DeepSeek Reasoner 模型
//!     use ds_api::raw::{Thinking, ThinkingType};
//!
//!     let reasoner_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message::new(Role::System, "You are a helpful assistant that explains your reasoning."),
//!             Message::new(Role::User, "Solve this math problem: What is 15% of 200?"),
//!         ],
//!         model: Model::DeepseekReasoner,
//!         thinking: Some(Thinking {
//!             r#type: ThinkingType::Enabled,
//!         }),
//!         max_tokens: Some(150),
//!         temperature: Some(0.3),
//!         stream: Some(false),
//!         ..Default::default()
//!     };
//!
//!     println!("Reasoner request JSON:");
//!     let json = serde_json::to_string_pretty(&reasoner_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // 示例 4: JSON 模式响应格式
//!     use ds_api::raw::{ResponseFormat, ResponseFormatType};
//!
//!     let json_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message::new(Role::System, "You are a helpful assistant that always responds in valid JSON format."),
//!             Message::new(Role::User, "Give me information about Paris in JSON format with fields: name, country, population, and landmarks."),
//!         ],
//!         model: Model::DeepseekChat,
//!         max_tokens: Some(200),
//!         temperature: Some(0.5),
//!         stream: Some(false),
//!         response_format: Some(ResponseFormat {
//!             r#type: ResponseFormatType::JsonObject,
//!         }),
//!         ..Default::default()
//!     };
//!
//!     println!("JSON mode request:");
//!     let json = serde_json::to_string_pretty(&json_request).unwrap();
//!     println!("{}\n", json);
//! }
//! ```
//!
//! # 序列化/反序列化
//!
//! 所有结构体都实现了 `Serialize` 和 `Deserialize` trait，可以直接使用 `serde_json` 进行序列化和反序列化：
//!
//! ```rust
//! use ds_api::raw::{ChatCompletionRequest, Message, Model, Role};
//!
//! let request = ChatCompletionRequest {
//!     messages: vec![Message::new(Role::User, "Hello")],
//!     model: Model::DeepseekChat,
//!     ..Default::default()
//! };
//! let json_string = serde_json::to_string(&request).unwrap();
//! let parsed_request: ChatCompletionRequest = serde_json::from_str(&json_string).unwrap();
//! ```
//!
//! # 与高级 API 的关系
//!
//! 高级 API（如 [`Request`](../request/struct.Request.html)）内部使用这些原始数据结构，
//! 但提供了更友好的构建器接口和验证逻辑。如果你需要更简单的使用方式，请
pub mod request;
pub mod response;

pub use request::*;
pub use response::*;
