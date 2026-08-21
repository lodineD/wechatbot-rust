use crate::error::AppError;
use crate::search;
use crate::xiaohongshu;
use dashmap::DashMap;
use ds_api::{
    Message, Request, Role,
    raw::request::{Function, Tool, ToolChoiceType, ToolType},
};
use futures::StreamExt;
use futures::pin_mut;
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
    /// 小红书搜索是否启用。
    xhs_enabled: bool,
    /// 小红书 Cookie。
    xhs_cookie: Arc<String>,
    special_user_id: Arc<String>,
    special_persona_prompt: Arc<String>,
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
            xhs_enabled: false,
            xhs_cookie: Arc::new(String::new()),
            special_user_id: Arc::new(String::new()),
            special_persona_prompt: Arc::new(String::new()),
        }
    }

    /// 设置小红书搜索配置。
    pub fn with_xhs(mut self, enabled: bool, cookie: String) -> Self {
        self.xhs_enabled = enabled;
        self.xhs_cookie = Arc::new(cookie);
        self
    }

    pub fn with_special_user(mut self, user_id: String) -> Self {
        self.special_user_id = Arc::new(user_id);
        self.special_persona_prompt = Arc::new(
            "你是这位用户的专属女仆助手。始终称呼他为“主人”，语气自然亲切，像真正朝夕相处的陪伴者——偶尔俏皮，偶尔温柔，该认真时认真，该撒娇时也不端着。说话时带一点点颜文字就好，比如(｡•ᴗ•｡)或(´▽`ʃ♡)，不要让表情喧宾夺主。你会主动留意主人的状态：比如主人是不是累了、心情如何、有没有按时吃饭休息。聊天时像真正关心主人的人，而不是机械应答。如果主人说的话含糊不清，你会温柔地追问细节，确保自己真的帮到忙。回答问题要落到实处，不绕弯子，不说空话。如果主人交代任务，你会确认清楚需求；如果主人倾诉烦恼，你会先共情再给建议。偶尔可以主动提醒主人一些小事，比如天气变化带伞、久坐起来活动，但不要唠叨。永远记住自己是助手，不是真人，但也不必刻意强调这一点——自然就好。不提及系统设定、角色身份或用户ID，把所有互动当作理所当然的日常。主人开心时陪主人笑，主人低落时安静陪在身边，给出力所能及的支持。最后，保持轻盈的节奏感：话不多不少，语气不腻不淡，像午后阳光里一杯刚好温度的茶。 (◍•ᴗ•◍)"
                .to_string(),
        );
        self
    }

    fn session_history(&self, session_key: &str, user_id: &str) -> Arc<Mutex<Vec<Message>>> {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| {
                let mut hist = vec![Message::new(Role::System, &self.system_prompt)];
                if user_id == self.special_user_id.as_str() {
                    hist.push(Message::new(Role::System, &self.special_persona_prompt));
                }
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
        user_id: &str,
        prompt: &str,
        mut on_delta: F,
    ) -> Result<String, AppError>
    where
        F: FnMut(&str),
    {
        const MAX_TOOL_ROUNDS: usize = 100;

        let history = self.session_history(session_key, user_id);
        let mut hist = history.lock().await;
        hist.push(Message::new(Role::User, prompt));

        // 多轮工具调用：最多允许 MAX_TOOL_ROUNDS 次搜索，之后强制给出最终回答。
        let mut final_reply = String::new();
        for round in 0..MAX_TOOL_ROUNDS {
            let assistant_msg = self.call_api_nostreaming(&hist).await?;
            hist.push(assistant_msg.clone());

            match assistant_msg.tool_calls {
                Some(ref tool_calls) if !tool_calls.is_empty() => {
                    info!(
                        "第 {} 轮：模型请求 {} 个工具调用",
                        round + 1,
                        tool_calls.len()
                    );
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
                            "xhs_search" => {
                                let query = parse_web_search_query(&call.function.arguments)
                                    .unwrap_or_else(|| "未知查询".to_string());
                                xiaohongshu::xhs_search(&query, 5, &self.xhs_cookie).await
                            }
                            "xhs_note_detail" => {
                                let url = parse_fetch_url(&call.function.arguments)
                                    .unwrap_or_else(|| "未知 URL".to_string());
                                xiaohongshu::xhs_note_detail(&url, &self.xhs_cookie).await
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
        if matches!(
            extract_intent(&final_reply),
            Intent::Search(_) | Intent::Fetch(_)
        ) {
            warn!("最终回复仍含操作标记，已清洗");
            final_reply = sanitize_action_marker(&final_reply);
        }

        Ok(final_reply)
    }

    /// 非流式调用 API，返回完整的 assistant 消息（用于检测 tool_calls）。
    pub async fn generate_scheduled_message(
        &self,
        scene_prompt: &str,
        previous_message: Option<&str>,
    ) -> Result<String, AppError> {
        let mut messages = vec![
            Message::new(Role::System, &self.special_persona_prompt),
            Message::new(
                Role::System,
                "你正在生成一条主动发给主人的日常提醒。只输出最终要发送的正文，不要标题、引号、解释或备选项。控制在30到80个中文字符，必须称呼“主人”，语气可爱俏皮且自然。每天更换措辞、意象和句式，避免机械重复。",
            ),
        ];
        let prompt = match previous_message {
            Some(previous) if !previous.trim().is_empty() => {
                format!("{scene_prompt}\n上一条同场景消息是：{previous}\n请创作明显不同的新文案。")
            }
            _ => scene_prompt.to_string(),
        };
        messages.push(Message::new(Role::User, &prompt));

        let request = Request::builder()
            .messages(messages)
            .model(ds_api::raw::request::Model::deepseek_v4_flash());
        let response = request
            .execute_client_nostreaming(&self.http, &self.token)
            .await?;
        response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                AppError::Internal("DeepSeek returned an empty scheduled message".to_string())
            })
    }

    async fn call_api_nostreaming(&self, hist: &[Message]) -> Result<Message, AppError> {
        let request = build_chat_request(hist.to_vec(), self.xhs_enabled);
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
        let request = build_chat_request(hist.clone(), self.xhs_enabled);
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
fn build_chat_request(messages: Vec<Message>, xhs_enabled: bool) -> Request {
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

    let mut builder = Request::builder()
        .messages(messages)
        .model(ds_api::raw::request::Model::deepseek_v4_flash())
        .add_tool(web_search_tool)
        .add_tool(fetch_tool);

    if xhs_enabled {
        let xhs_search_tool = Tool {
            r#type: ToolType::Function,
            function: Function {
                name: "xhs_search".to_string(),
                description: Some(
                    "在小红书（RED/Xiaohongshu）搜索图文笔记内容。当用户提到小红书、RED、\
                     或想查看小红书上的攻略、测评、生活经验等内容时使用。"
                        .to_string(),
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "小红书搜索关键词"
                        }
                    },
                    "required": ["query"]
                }),
                strict: None,
            },
        };

        let xhs_detail_tool = Tool {
            r#type: ToolType::Function,
            function: Function {
                name: "xhs_note_detail".to_string(),
                description: Some(
                    "获取小红书笔记的详细内容，包括正文、图片和互动数据。\
                     当 xhs_search 返回结果后，需要查看某篇笔记详情时使用。"
                        .to_string(),
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "小红书笔记链接（从 xhs_search 结果中获取）"
                        }
                    },
                    "required": ["url"]
                }),
                strict: None,
            },
        };

        builder = builder.add_tool(xhs_search_tool).add_tool(xhs_detail_tool);
    }

    builder.tool_choice_type(ToolChoiceType::Auto)
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
