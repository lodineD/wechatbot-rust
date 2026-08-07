use serde::{Deserialize, Serialize};

/// DeepSeek 模型名。
///
/// DeepSeek 的官方模型名在持续演进（`deepseek-v4-pro` / `deepseek-v4-flash` 等），
/// 这里使用透明新类型（`#[serde(transparent)]`）包装 `String`，以便：
/// - 接受当前 DeepSeek 返回的任意模型名（解码不会因枚举不匹配而失败）；
/// - 仍保留常用模型名的常量/构造方法，便于用户代码书写。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Model(pub String);

impl Default for Model {
    fn default() -> Self {
        // DeepSeek 当前官方推荐的默认模型。
        Self::deepseek_v4_pro()
    }
}


impl Model {
    /// DeepSeek-V4-Pro（当前官方推荐的对话模型）。
    pub fn deepseek_v4_pro() -> Self {
        Self("deepseek-v4-pro".to_string())
    }

    /// DeepSeek-V4-Flash（当前官方推荐，更快更便宜）。
    pub fn deepseek_v4_flash() -> Self {
        Self("deepseek-v4-flash".to_string())
    }

    /// 兼容旧名 `deepseek-chat`（如后端仍支持）。
    pub fn deepseek_chat() -> Self {
        Self("deepseek-chat".to_string())
    }

    /// 兼容旧名 `deepseek-reasoner`（如后端仍支持）。
    pub fn deepseek_reasoner() -> Self {
        Self("deepseek-reasoner".to_string())
    }

    /// 使用任意模型名创建。
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// 返回模型名字符串切片。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Model {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Model {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}
