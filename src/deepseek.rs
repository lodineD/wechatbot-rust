use crate::error::AppError;
use crate::search;
use dashmap::DashMap;
use ds_api::{
    raw::request::{Function, Tool, ToolChoiceType, ToolType},
    Message, Request, Role,
};
use futures::pin_mut;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// 用户意图：要么搜索，要么抓取页面，要么不需要任何操作。
///
/// 旧版 marker-based 搜索逻辑保留，但当前已通过 DeepSeek Tool Calling 实现联网搜索，
/// 因此该枚举暂时不被使用。
#[allow(dead_code)]
enum Intent {
    Search(String),
    Fetch(String),
    None,
}

#[derive(Clone)]
pub struct DeepseekClient {
    token: Arc<String>,
    system_prompt: Arc<String>,
    http: HttpClient,
    sessions: Arc<DashMap<String, Arc<Mutex<Vec<Message>>>>>,
    max_history: usize,
    /// 是否注入旧版 [SEARCH:]/[FETCH:] marker 提示词。
    /// 当前使用 Tool Calling，不再依赖 marker，因此默认关闭。
    web_search_enabled: bool,
}

impl DeepseekClient {
    pub fn new(api_key: impl Into<String>, system_prompt: impl Into<String>) -> Self {
        Self {
            token: Arc::new(api_key.into()),
            system_prompt: Arc::new(system_prompt.into()),
            http: HttpClient::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("构建 HTTP 客户端失败"),
            sessions: Arc::new(DashMap::new()),
            max_history: 100,
            web_search_enabled: false,
        }
    }

    fn session_history(&self, session_key: &str) -> Arc<Mutex<Vec<Message>>> {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| {
                let mut hist = vec![Message::new(Role::System, &self.system_prompt)];
                hist.push(Message::new(
                    Role::System,
                    "\n\n[工具使用规则] 你具备 web_search 和 fetch 两种工具。\n\
                     - web_search：当用户询问实时信息、新闻、价格、数据等需要联网的内容时使用。\n\
                     - fetch：当你已经知道一个具体 URL，并且需要读取该页面详细内容时使用。\n\
                     - 如果现有搜索结果或页面内容已足够回答问题，请直接给出最终回答，不要反复搜索/抓取。\n\
                     - 最多连续使用 100 次工具；到达上限后必须基于已有信息给出最终回答。\n\
                     - 不要只说「我再搜索/抓取一下」，而要尽快给出实质性回答。",
                ));
                if self.web_search_enabled {
                    hist.push(Message::new(
                        Role::System,
                        "\n\n[系统能力] 你具备以下能力：\n\n\
                         1. **联网搜索**：当用户提问涉及实时信息、新闻、最新数据、不确定的事实等需要联网获取的内容时，\
                         你必须在回复中严格按照以下格式输出搜索请求：\n[SEARCH:搜索关键词]\n\n\
                         2. **网页内容抓取**：当用户给你一个链接让你查看内容时，\
                         你必须在回复中严格按照以下格式输出抓取请求：\n[FETCH:完整URL]\n\n\
                         注意：\n\
                         - 你的回复只需要包含上述标记，不需要多余的解释。\n\
                         - 当你看到系统注入的「以下是搜索结果」或「以下是页面内容」后，\
                         必须直接基于这些信息给出最终回答，不要再输出标记。\n\
                         - 如果不需要搜索或抓取，直接正常回复即可。",
                    ));
                }
                Arc::new(Mutex::new(hist))
            })
            .value()
            .clone()
    }

    /// 流式聊天（支持 DeepSeek Tool Calling 联网搜索）。
    pub async fn chat_stream<F>(
        &self,
        session_key: &str,
        prompt: &str,
        mut on_delta: F,
    ) -> Result<String, AppError>
    where
        F: FnMut(&str),
    {
        const MAX_TOOL_ROUNDS: usize = 100;

        let history = self.session_history(session_key);
        let mut hist = history.lock().await;
        hist.push(Message::new(Role::User, prompt));

        // 多轮工具调用：最多允许 MAX_TOOL_ROUNDS 次搜索，之后强制给出最终回答。
        let mut final_reply = String::new();
        for round in 0..MAX_TOOL_ROUNDS {
            let assistant_msg = self.call_api_nostreaming(&hist).await?;
            hist.push(assistant_msg.clone());

            match assistant_msg.tool_calls {
                Some(ref tool_calls) if !tool_calls.is_empty() => {
                    info!("第 {} 轮：模型请求 {} 个工具调用", round + 1, tool_calls.len());
                    for call in tool_calls {
                        info!(
                            "tool_call: id={}, name={}, args={}",
                            call.id, call.function.name, call.function.arguments
                        );

                        let tool_result = match call.function.name.as_str() {
                            "web_search" => {
                                let query = parse_web_search_query(&call.function.arguments)
                                    .unwrap_or_else(|| "未知查询".to_string());
                                search::web_search(&query, 5).await
                            }
                            "fetch" => {
                                let url = parse_fetch_url(&call.function.arguments)
                                    .unwrap_or_else(|| "未知 URL".to_string());
                                search::fetch_url_content(&url).await
                            }
                            _ => format!("不支持的工具调用：{}", call.function.name),
                        };

                        hist.push(Message {
                            role: Role::Tool,
                            content: Some(tool_result),
                            name: None,
                            tool_call_id: Some(call.id.clone()),
                            tool_calls: None,
                            reasoning_content: None,
                            prefix: None,
                        });
                    }

                    // 到达最大轮次：强制模型基于已有信息回答
                    if round == MAX_TOOL_ROUNDS - 1 {
                        warn!("搜索轮次达到上限 {}，强制给出最终回答", MAX_TOOL_ROUNDS);
                        hist.push(Message::new(
                            Role::System,
                            "你已经进行了多次搜索。请基于当前搜索结果直接给出最终回答，不要再调用任何工具。",
                        ));
                        final_reply = self.stream_with_delta(&mut hist, &mut on_delta).await?;
                        break;
                    }

                    // 否则继续下一轮，让模型基于搜索结果决定回答或再搜索
                    continue;
                }
                _ => {
                    // 没有工具调用：直接返回当前回答
                    final_reply = assistant_msg.content.unwrap_or_default();
                    on_delta(&final_reply);
                    break;
                }
            }
        }

        Self::truncate_history(&mut hist, self.max_history);

        // 兜底：如果模型仍输出旧版 marker，则清洗
        if matches!(extract_intent(&final_reply), Intent::Search(_) | Intent::Fetch(_)) {
            warn!("最终回复仍含操作标记，已清洗");
            final_reply = sanitize_action_marker(&final_reply);
        }

        Ok(final_reply)
    }

    /// 非流式调用 API，返回完整的 assistant 消息（用于检测 tool_calls）。
    async fn call_api_nostreaming(&self, hist: &[Message]) -> Result<Message, AppError> {
        let request = build_chat_request(hist.to_vec());
        let response = request
            .execute_client_nostreaming(&self.http, &self.token)
            .await?;

        response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| AppError::Internal("DeepSeek 返回空 choices".to_string()))
    }

    /// 流式调用 API，把每个 content delta 通过 on_delta 回调返回给上层。
    /// 返回完整的 assistant 文本，并自动把 assistant 消息追加到 hist。
    async fn stream_with_delta<F>(
        &self,
        hist: &mut Vec<Message>,
        on_delta: &mut F,
    ) -> Result<String, AppError>
    where
        F: FnMut(&str),
    {
        let request = build_chat_request(hist.clone());
        let stream = request
            .execute_client_streaming(&self.http, &self.token)
            .await?;
        pin_mut!(stream);

        let mut full = String::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(choice) = chunk.choices.get(0) {
                if let Some(c) = choice.delta.content.as_ref() {
                    full.push_str(c);
                    on_delta(c);
                }
            }
        }

        hist.push(Message::new(Role::Assistant, &full));
        Ok(full)
    }

    fn truncate_history(hist: &mut Vec<Message>, max: usize) {
        let msg_count = hist.len().saturating_sub(1);
        if msg_count > max {
            let to_remove = msg_count - max;
            hist.drain(1..1 + to_remove);
        }
    }
}

/// 构造带联网搜索工具的聊天请求。
fn build_chat_request(messages: Vec<Message>) -> Request {
    let web_search_tool = Tool {
        r#type: ToolType::Function,
        function: Function {
            name: "web_search".to_string(),
            description: Some("联网搜索工具，用于获取实时信息、新闻、价格等".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词"
                    }
                },
                "required": ["query"]
            }),
            strict: None,
        },
    };

    let fetch_tool = Tool {
        r#type: ToolType::Function,
        function: Function {
            name: "fetch".to_string(),
            description: Some("抓取指定 URL 页面的详细内容，用于读取网页正文。".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "要抓取的完整网页 URL"
                    }
                },
                "required": ["url"]
            }),
            strict: None,
        },
    };

    Request::builder()
        .messages(messages)
        .model(ds_api::raw::request::Model::deepseek_v4_flash())
        .add_tool(web_search_tool)
        .add_tool(fetch_tool)
        .tool_choice_type(ToolChoiceType::Auto)
}

/// 解析 web_search 工具的查询参数。
fn parse_web_search_query(args: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(args).ok()?;
    value
        .get("query")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析 fetch 工具的 URL 参数。
fn parse_fetch_url(args: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(args).ok()?;
    value
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 从回复中提取意图：搜索、抓取页面、或什么都不做。
#[allow(dead_code)]
fn extract_intent(reply: &str) -> Intent {
    if let Some(url) = extract_fetch_url(reply) {
        return Intent::Fetch(url);
    }
    if let Some(query) = extract_search_query(reply) {
        return Intent::Search(query);
    }
    Intent::None
}

#[allow(dead_code)]
fn extract_search_query(reply: &str) -> Option<String> {
    let start = reply.find("[SEARCH:")?;
    let after = start + "[SEARCH:".len();
    let end = reply[after..].find(']')?;
    let q = reply[after..after + end].trim().to_string();
    if q.is_empty() { None } else { Some(q) }
}

#[allow(dead_code)]
fn extract_fetch_url(reply: &str) -> Option<String> {
    let start = reply.find("[FETCH:")?;
    let after = start + "[FETCH:".len();
    let end = reply[after..].find(']')?;
    let url = reply[after..after + end].trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// 清洗回复中的 `[SEARCH:...]` 和 `[FETCH:...]` 标记。
#[allow(dead_code)]
fn sanitize_action_marker(reply: &str) -> String {
    let mut out = reply.to_string();
    for prefix in &["[SEARCH:", "[FETCH:"] {
        while let Some(start) = out.find(prefix) {
            let after = start + prefix.len();
            if let Some(end_rel) = out[after..].find(']') {
                let end = after + end_rel;
                out.replace_range(start..=end, "");
            } else {
                break;
            }
        }
    }
    out.trim().to_string()
}
