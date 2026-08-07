use serde::{Deserialize, Serialize};

use super::message::ToolType;

#[derive(Debug, Serialize, Deserialize)]
pub struct Tool {
    /// tool 的类型。目前仅支持 function。
    pub r#type: ToolType,
    pub function: Function,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Function {
    /// 要调用的 function 名称。必须由 a-z、A-Z、0-9 字符组成，或包含下划线和连字符，最大长度为 64 个字符。
    pub name: String,

    /// function 的功能描述，供模型理解何时以及如何调用该 function。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// function 的输入参数，以 JSON Schema 对象描述。请参阅Tool Calls 指南获取示例，并参阅JSON Schema 参考了解有关格式的文档。省略 parameters 会定义一个参数列表为空的 function。
    pub parameters: serde_json::Value,

    /// Beta 功能：如果设置为 true，API 将在函数调用中使用 strict 模式
    /// 如果设置为 true，API 将在函数调用中使用 strict 模式，以确保输出始终符合函数的 JSON schema 定义。该功能为 Beta 功能，详细使用方式请参阅Tool Calls 指南
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}
