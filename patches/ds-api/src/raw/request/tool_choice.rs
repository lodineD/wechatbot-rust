use serde::{Deserialize, Serialize};

use super::message::ToolType;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    String(ToolChoiceType),

    /// type string REQUIRED
    ///     Possible values: `function`
    ///     tool 的类型。目前，仅支持 function。
    /// function object REQUIRED
    ///     name string REQUIRED
    ///     要调用的函数名称。
    Object(ToolChoiceObject),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceType {
    None,
    Auto,
    Required,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolChoiceObject {
    pub r#type: ToolType,
    pub function: FunctionName,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionName {
    pub name: String,
}
