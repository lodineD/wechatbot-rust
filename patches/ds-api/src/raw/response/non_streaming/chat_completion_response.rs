use serde::Deserialize;

use super::{choice::Choice, object_type::ObjectType, usage::Usage};
use crate::raw::Model;

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub created: u64,
    pub model: Model,
    // DeepSeek 官方响应示例中该字段可能不出现，故改为 Option。
    pub system_fingerprint: Option<String>,
    #[serde(rename = "object")]
    pub object: ObjectType,
    pub usage: Usage,
}
