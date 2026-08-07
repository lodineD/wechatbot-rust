use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Thinking {
    /// 如果设为 enabled，则使用思考模式。如果设为 disabled，则使用非思考模式
    pub r#type: ThinkingType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingType {
    Disabled,
    Enabled,
}
