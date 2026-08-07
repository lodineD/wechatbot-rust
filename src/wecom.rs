use crate::deepseek::DeepseekClient;
use crate::error::AppError;
use serde_json::json;
use tracing::{error, info, warn};
use wecom_aibot_rust_sdk::{generate_req_id, SdkError, WSClient, WSClientOptions, WsFrame};

/// 企业微信智能机器人客户端封装。
pub struct WecomBot {
    client: WSClient,
    deepseek: DeepseekClient,
}

impl WecomBot {
    /// 创建并初始化企业微信机器人客户端。
    pub fn new(bot_id: impl Into<String>, bot_secret: impl Into<String>, deepseek: DeepseekClient) -> Self {
        let options = WSClientOptions::new(bot_id.into(), bot_secret.into());
        let client = WSClient::new(options);

        Self { client, deepseek }
    }

    /// 注册事件处理器并启动连接。
    pub async fn run(&self) -> Result<(), AppError> {
        let client = self.client.clone();

        self.client
            .on_connected(move || {
                info!("企业微信 WebSocket 已连接");
            })
            .await;

        self.client
            .on_authenticated(move || {
                info!("企业微信机器人认证成功");
            })
            .await;

        self.client
            .on_disconnected(move |reason| {
                warn!("企业微信 WebSocket 已断开: {reason}");
            })
            .await;

        self.client
            .on_reconnecting(move |attempt| {
                info!("企业微信 WebSocket 第 {attempt} 次重连...");
            })
            .await;

        self.client
            .on_error(move |err| {
                if is_not_subscribed_error(err) {
                    error!("企业微信返回错误 846609：智能机器人未建立长连接订阅。");
                    error!("请检查：1) 企业微信后台是否已开启该机器人的 API 模式/长连接；2) Bot ID 和 Secret 是否正确；3) 是否有其他进程占用了同一个 Bot 的长连接。");
                } else {
                    error!("企业微信 SDK 错误: {err}");
                }
            })
            .await;

        // 处理进入会话事件：发送欢迎语
        let client_enter = client.clone();
        self.client
            .on_event_enter_chat(move |frame| {
                let frame = frame.clone();
                let client_enter = client_enter.clone();

                tokio::spawn(async move {
                    if let Err(e) = send_welcome(&client_enter, &frame).await {
                        error!("发送欢迎语失败: {e}");
                    }
                });
            })
            .await;

        // 处理文本消息
        let deepseek = self.deepseek.clone();
        self.client
            .on_message_text(move |frame| {
                let frame = frame.clone();
                let deepseek = deepseek.clone();
                let client = client.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_text_message(&client, &deepseek, &frame).await {
                        error!("处理文本消息失败: {e}");
                    }
                });
            })
            .await;

        info!("正在连接企业微信智能机器人...");
        if let Err(e) = self.client.connect().await {
            if is_not_subscribed_error(&e) {
                error!("连接失败：企业微信返回错误 846609，智能机器人未建立长连接订阅。");
                error!("请按以下步骤排查：");
                error!("  1. 登录企业微信管理后台，进入【应用管理】-> 目标智能机器人；");
                error!("  2. 确认已开启「API 模式」并选择「长连接」接入方式；");
                error!("  3. 确认 .env 中的 WECHAT_BOT_ID 和 WECHAT_BOT_SECRET 与后台一致；");
                error!("  4. 确认没有其他地方（包括本机其他终端/容器）占用同一个 Bot 的长连接；");
                error!("  5. 确认机器人已发布，且当前成员在机器人的可见范围内。");
            }
            return Err(e.into());
        }

        // 保持运行直到收到 Ctrl+C
        tokio::signal::ctrl_c().await.ok();
        info!("收到退出信号，正在断开连接...");
        self.client.disconnect();

        Ok(())
    }
}

/// 判断 SDK 错误是否为企业微信 846609（机器人未订阅/未建立长连接订阅）。
fn is_not_subscribed_error(err: &SdkError) -> bool {
    let msg = err.to_string();
    msg.contains("846609") || msg.contains("aibot websocket not subscribed")
}

/// 从消息帧中提取文本内容。
fn extract_text_content(frame: &WsFrame) -> Option<String> {
    frame
        .body
        .as_ref()?
        .get("text")?
        .get("content")?
        .as_str()
        .map(|s| s.trim().to_string())
}

/// 从消息帧中提取会话标识，用于隔离多会话历史。
///
/// 格式：`chatid:userid`，若取不到则回退到 `"default"`。
fn extract_session_key(frame: &WsFrame) -> String {
    let body = frame.body.as_ref();
    let chatid = body
        .and_then(|b| b.get("chatid"))
        .and_then(|v| v.as_str());
    let userid = body
        .and_then(|b| b.get("from"))
        .and_then(|v| v.get("userid"))
        .and_then(|v| v.as_str());
    match (chatid, userid) {
        (Some(c), Some(u)) => format!("{c}:{u}"),
        _ => "default".to_string(),
    }
}

/// 发送欢迎语。
async fn send_welcome(client: &WSClient, frame: &WsFrame) -> Result<(), AppError> {
    let body = json!({
        "msgtype": "text",
        "text": {
            "content": "您好！我是智能助手，有什么可以帮您的吗？"
        }
    });

    client.reply_welcome(frame, body).await?;
    info!("已发送欢迎语");
    Ok(())
}

/// 处理文本消息：调用 DeepSeek（流式）并将结果分片推送给用户。
async fn handle_text_message(
    client: &WSClient,
    deepseek: &DeepseekClient,
    frame: &WsFrame,
) -> Result<(), AppError> {
    let content = extract_text_content(frame).ok_or("无法从消息帧中提取文本内容")?;
    if content.is_empty() {
        info!("收到空文本消息，忽略");
        return Ok(());
    }

    let session_key = extract_session_key(frame);
    info!("收到消息(session={session_key}): {content}");

    let stream_id = generate_req_id("stream");

    // 先发送占位提示
    if let Err(e) = client
        .reply_stream(frame, &stream_id, "正在思考...", false, None, None)
        .await
    {
        error!("发送占位消息失败: {e}");
        // 占位失败不阻断后续流程
    }

    // 批量推送：累积一定字符后再推一次，避免过于频繁
    let mut accumulated = String::new();
    let mut last_flush_len = 0usize;
    const FLUSH_THRESHOLD: usize = 15; // 累计新增 15 个字符后推一次

    let result = deepseek
        .chat_stream(&session_key, &content, |delta| {
            accumulated.push_str(delta);
            // 累计足够增量时推一次
            if accumulated.len() - last_flush_len >= FLUSH_THRESHOLD {
                // 不阻塞回调，直接推
                let _ = client.reply_stream(
                    frame,
                    &stream_id,
                    &accumulated,
                    false,
                    None,
                    None,
                );
                last_flush_len = accumulated.len();
            }
        })
        .await;

    match result {
        Ok(reply) => {
            info!("DeepSeek 回复(session={session_key}): {reply}");
            // 最终推送完整内容，finish=true
            client
                .reply_stream(frame, &stream_id, &reply, true, None, None)
                .await?;
        }
        Err(e) => {
            error!("调用 DeepSeek API 失败(session={session_key}): {e}");
            // 如果已有部分内容，先把已收到的推完（finish=true）
            if !accumulated.is_empty() {
                client
                    .reply_stream(frame, &stream_id, &accumulated, true, None, None)
                    .await?;
            } else {
                client
                    .reply_stream(
                        frame,
                        &stream_id,
                        "抱歉，我现在有点问题，请稍后再试。",
                        true,
                        None,
                        None,
                    )
                    .await?;
            }
        }
    }

    Ok(())
}