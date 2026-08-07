use serde::{Deserialize, Serialize};

// 统一的消息结构体
// 该结构体同时用于请求中的 messages 数组和响应中的 message 字段。
// 所有字段均为可选，以覆盖不同角色和场景的需求。
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Message {
    /// default role is User
    pub role: Role,

    /// content 在 assistant 消息可能为 null（仅 tool_calls 时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// 用于标识用户/函数名称（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 当 role = "tool" 时必须提供，关联之前的工具调用 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// 当 role = "assistant" 且模型请求调用工具时包含此字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// 模型推理过程的内容（仅在响应中可能包含）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    /// Beta 功能：设置此参数为 true，来强制模型在其回答中以此 assistant 消息中提供的前缀内容开始
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<bool>,
}

impl Message {
    pub fn new(role: Role, message: &str) -> Self {
        Self {
            role,
            content: Some(message.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            prefix: None,
        }
    }
}

// 角色枚举（包含 Tool 变体）
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

// 工具调用结构体（请求和响应中复用）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub r#type: ToolType,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ToolType {
    Function,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON 字符串
}
