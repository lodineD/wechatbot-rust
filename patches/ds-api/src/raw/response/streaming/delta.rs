use serde::Deserialize;

use crate::raw::request::message::{Role, ToolType};

// Delta 与 Message 类似，但 tool_calls 是增量形式（带 index）
#[derive(Debug, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct DeltaToolCall {
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub r#type: Option<ToolType>,
    #[serde(default)]
    pub function: Option<DeltaFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct DeltaFunctionCall {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}
