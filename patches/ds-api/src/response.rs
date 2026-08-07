//! 响应 trait 模块
//!
//! 提供统一的响应处理接口，简化对 DeepSeek API 响应的访问。
//!
//! # 主要特性
//!
//! - [`Response`] trait: 定义响应对象的通用接口
//! - 为原始响应类型提供标准实现
//!
//! # 示例
//!
//! ```rust
//! use ds_api::{Response, ChatCompletionResponse};
//! use std::time::SystemTime;
//!
//! // 假设有一个 ChatCompletionResponse 实例
//! # let response = ChatCompletionResponse {
//! #     id: "test".to_string(),
//! #     object: ds_api::ObjectType::ChatCompletion,
//! #     created: 1234567890,
//! #     model: ds_api::Model::DeepseekChat,
//! #     system_fingerprint: "test_fingerprint".to_string(),
//! #     choices: vec![ds_api::Choice {
//! #         index: 0,
//! #         message: ds_api::Message {
//! #             role: ds_api::Role::Assistant,
//! #             content: Some("Hello!".to_string()),
//! #             ..Default::default()
//! #         },
//! #         finish_reason: ds_api::FinishReason::Stop,
//! #         logprobs: None,
//! #     }],
//! #     usage: ds_api::Usage {
//! #         prompt_tokens: 10,
//! #         completion_tokens: 5,
//! #         total_tokens: 15,
//! #         prompt_cache_hit_tokens: None,
//! #         prompt_cache_miss_tokens: None,
//! #         completion_tokens_details: None,
//! #     },
//! # };
//!
//! // 获取响应内容
//! let content = response.content().unwrap();
//! println!("Response content: {}", content);
//!
//! // 获取响应创建时间
//! let created_time: SystemTime = response.created();
//! println!("Response created at: {:?}", created_time);
//! ```
//!
//! # 实现说明
//!
//! [`Response`] trait 提供了对 API 响应的统一访问方式，无论底层响应结构如何变化，
//! 用户都可以通过相同的接口获取响应内容和创建时间。

use std::{
    ops::Add,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    error::{ApiError, Result},
    raw::ChatCompletionResponse,
};

/// 响应 trait，为 DeepSeek API 响应提供统一的访问接口
///
/// 这个 trait 定义了所有响应类型都应该实现的基本操作，
/// 使得用户可以以一致的方式处理不同类型的响应。
pub trait Response {
    /// 获取响应的文本内容
    ///
    /// # 返回
    ///
    /// 返回响应内容的字符串切片。对于聊天补全响应，这通常是助手的回复文本。
    fn content(&self) -> Result<&str>;

    /// 获取响应的创建时间
    ///
    /// # 返回
    ///
    /// 返回响应创建的系统时间，可以用于日志记录、缓存控制等场景。
    fn created(&self) -> SystemTime;
}

impl Response for ChatCompletionResponse {
    fn content(&self) -> Result<&str> {
        self.choices
            .get(0)
            .and_then(|c| c.message.content.as_deref())
            .ok_or_else(|| ApiError::Other("empty choices or missing content".to_string()))
    }

    fn created(&self) -> SystemTime {
        UNIX_EPOCH.add(Duration::from_secs(self.created))
    }
}
