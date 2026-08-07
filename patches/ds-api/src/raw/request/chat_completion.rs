use serde::{Deserialize, Serialize};

use super::{
    message::Message, model::Model, response_format::ResponseFormat, stop::Stop,
    stream_options::StreamOptions, thinking::Thinking, tool::Tool, tool_choice::ToolChoice,
};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatCompletionRequest {
    /// 对话的消息列表。
    pub messages: Vec<Message>,

    /// 使用的模型的 ID。您可以使用 deepseek-chat 来获得更快的响应速度，或者使用 deepseek-reasoner 来获得更深入的推理能力。
    pub model: Model,

    /// 控制思考模式与非思考模式的转换
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,

    /// Possible values: >= -2 and <= 2
    /// Default value: 0
    /// 介于 -2.0 和 2.0 之间的数字。如果该值为正，那么新 token 会根据其在已有文本中的出现频率受到相应的惩罚，降低模型重复相同内容的可能性。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// 限制一次请求中模型生成 completion 的最大 token 数。输入 token 和输出 token 的总长度受模型的上下文长度的限制。取值范围与默认值详见文档。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Possible values: >= -2 and <= 2
    /// Default value: 0
    /// 介于 -2.0 和 2.0 之间的数字。如果该值为正，那么新 token 会根据其是否已在已有文本中出现受到相应的惩罚，从而增加模型谈论新主题的可能性。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// 一个 object，指定模型必须输出的格式。
    /// 设置为 { "type": "json_object" } 以启用 JSON 模式，该模式保证模型生成的消息是有效的 JSON。
    /// 注意: 使用 JSON 模式时，你还必须通过系统或用户消息指示模型生成 JSON。否则，模型可能会生成不断的空白字符，直到生成达到令牌限制，从而导致请求长时间运行并显得“卡住”。此外，如果 finish_reason="length"，这表示生成超过了 max_tokens 或对话超过了最大上下文长度，消息内容可能会被部分截断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,

    /// 一个 string 或最多包含 16 个 string 的 list，在遇到这些词时，API 将停止生成更多的 token。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Stop>,

    /// 如果设置为 True，将会以 SSE（server-sent events）的形式以流式发送消息增量。消息流以 data: \[DONE\] 结尾。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// 流式输出相关选项。只有在 stream 参数为 true 时，才可设置此参数。
    /// include_usage: boolean
    /// 如果设置为 true，在流式消息最后的 data: \[DONE\] 之前将会传输一个额外的块。此块上的 usage 字段显示整个请求的 token 使用统计信息，而 choices 字段将始终是一个空数组。所有其他块也将包含一个 usage 字段，但其值为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,

    /// Possible values: <= 2
    /// Default value: 1
    /// 采样温度，介于 0 和 2 之间。更高的值，如 0.8，会使输出更随机，而更低的值，如 0.2，会使其更加集中和确定。 我们通常建议可以更改这个值或者更改 top_p，但不建议同时对两者进行修改。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Possible values: <= 1
    /// Default value: 1
    /// 作为调节采样温度的替代方案，模型会考虑前 top_p 概率的 token 的结果。所以 0.1 就意味着只有包括在最高 10% 概率中的 token 会被考虑。 我们通常建议修改这个值或者更改 temperature，但不建议同时对两者进行修改。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// 模型可能会调用的 tool 的列表。目前，仅支持 function 作为工具。使用此参数来提供以 JSON 作为输入参数的 function 列表。最多支持 128 个 function。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// 控制模型调用 tool 的行为。
    /// none 意味着模型不会调用任何 tool，而是生成一条消息。
    /// auto 意味着模型可以选择生成一条消息或调用一个或多个 tool。
    /// required 意味着模型必须调用一个或多个 tool。
    /// 通过 {"type": "function", "function": {"name": "my_function"}} 指定特定 tool，会强制模型调用该 tool。
    /// 当没有 tool 时，默认值为 none。如果有 tool 存在，默认值为 auto。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// logprobs boolean NULLABLE
    /// 是否返回所输出 token 的对数概率。如果为 true，则在 message 的 content 中返回每个输出 token 的对数概率。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,

    /// Possible values: <= 20
    /// 一个介于 0 到 20 之间的整数 N，指定每个输出位置返回输出概率 top N 的 token，且返回这些 token 的对数概率。指定此参数时，logprobs 必须为 true。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::request::message::Role;

    #[test]
    fn test_chat_completion_request_serialization() {
        let request = ChatCompletionRequest {
            messages: vec![Message {
                role: Role::User,
                content: Some("Hello, world!".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                prefix: None,
            }],
            model: Model::deepseek_chat(),
            thinking: None,
            frequency_penalty: Some(0.5),
            max_tokens: Some(100),
            presence_penalty: None,
            response_format: None,
            stop: None,
            stream: Some(false),
            stream_options: None,
            temperature: Some(0.7),
            top_p: None,
            tools: None,
            tool_choice: None,
            logprobs: None,
            top_logprobs: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(
            parsed.messages[0].content.as_ref().unwrap(),
            "Hello, world!"
        );
        assert_eq!(parsed.model.as_str(), "deepseek-chat");
        assert_eq!(parsed.frequency_penalty, Some(0.5));
        assert_eq!(parsed.max_tokens, Some(100));
        assert_eq!(parsed.stream, Some(false));
        assert_eq!(parsed.temperature, Some(0.7));
    }

    #[test]
    fn test_default_chat_completion_request() {
        let request = ChatCompletionRequest::default();

        assert!(request.messages.is_empty());
        assert_eq!(request.model.as_str(), "deepseek-v4-pro");
        assert!(request.thinking.is_none());
        assert!(request.frequency_penalty.is_none());
        assert!(request.max_tokens.is_none());
        assert!(request.presence_penalty.is_none());
        assert!(request.response_format.is_none());
        assert!(request.stop.is_none());
        assert!(request.stream.is_none());
        assert!(request.stream_options.is_none());
        assert!(request.temperature.is_none());
        assert!(request.top_p.is_none());
        assert!(request.tools.is_none());
        assert!(request.tool_choice.is_none());
        assert!(request.logprobs.is_none());
        assert!(request.top_logprobs.is_none());
    }
}
