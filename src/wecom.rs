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
    pub fn new(bot_id: impl Into<String>, bot_secret: impl Into<String>, deepseek: DeepseekClient) -> Self {
        let options = WSClientOptions::new(bot_id.into(), bot_secret.into());
        let client = WSClient::new(options);
        Self { client, deepseek }
    }

    pub async fn run(&self) -> Result<(), AppError> {
        let client = self.client.clone();

        self.client.on_connected(move || info!("企业微信 WebSocket 已连接")).await;
        self.client.on_authenticated(move || info!("企业微信机器人认证成功")).await;
        self.client.on_disconnected(move |reason| warn!("企业微信 WebSocket 已断开: {reason}")).await;
        self.client.on_reconnecting(move |attempt| info!("企业微信 WebSocket 第 {attempt} 次重连...")).await;
        self.client.on_error(move |err| {
            if is_not_subscribed_error(err) {
                error!("企业微信返回错误 846609：智能机器人未建立长连接订阅。请检查后台配置。");
            } else {
                error!("企业微信 SDK 错误: {err}");
            }
        }).await;

        // 进入会话 → 欢迎语
        let client_enter = client.clone();
        self.client.on_event_enter_chat(move |frame| {
            let frame = frame.clone();
            let client_enter = client_enter.clone();
            tokio::spawn(async move {
                if let Err(e) = send_welcome(&client_enter, &frame).await {
                    error!("发送欢迎语失败: {e}");
                }
            });
        }).await;

        // 文本消息 → DeepSeek
        let deepseek = self.deepseek.clone();
        self.client.on_message_text(move |frame| {
            let frame = frame.clone();
            let deepseek = deepseek.clone();
            let client = client.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_text_message(&client, &deepseek, &frame).await {
                    error!("处理文本消息失败: {e}");
                }
            });
        }).await;

        info!("正在连接企业微信智能机器人...");
        if let Err(e) = self.client.connect().await {
            if is_not_subscribed_error(&e) {
                error!("连接失败：企业微信返回错误 846609。请检查后台配置。");
            }
            return Err(e.into());
        }

        tokio::signal::ctrl_c().await.ok();
        info!("收到退出信号，正在断开连接...");
        self.client.disconnect();
        Ok(())
    }
}

fn is_not_subscribed_error(err: &SdkError) -> bool {
    let msg = err.to_string();
    msg.contains("846609") || msg.contains("aibot websocket not subscribed")
}

fn extract_text_content(frame: &WsFrame) -> Option<String> {
    frame.body.as_ref()?.get("text")?.get("content")?.as_str().map(|s| s.trim().to_string())
}

fn extract_session_key(frame: &WsFrame) -> String {
    let body = frame.body.as_ref();
    let ct = body.and_then(|b| b.get("chattype")).and_then(|v| v.as_str());
    let uid = body.and_then(|b| b.get("from")).and_then(|v| v.get("userid")).and_then(|v| v.as_str()).unwrap_or("unknown");
    let cid = body.and_then(|b| b.get("chatid")).and_then(|v| v.as_str());
    match (ct, cid) {
        (Some("group"), Some(c)) => format!("{c}:{uid}"),
        _ => uid.to_string(),
    }
}

async fn send_welcome(client: &WSClient, frame: &WsFrame) -> Result<(), AppError> {
    let body = json!({"msgtype":"text","text":{"content":"您好！我是智能助手，有什么可以帮您的吗？"}});
    client.reply_welcome(frame, body).await?;
    info!("已发送欢迎语");
    Ok(())
}

/// 处理文本消息：调用 DeepSeek（含自动联网搜索）并将结果推送给用户。
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

    // 发送占位提示
    let _ = client
        .reply_stream(frame, &stream_id, "正在思考...", false, None, None)
        .await;

    // 流式收集 + 批量推送
    let mut buf = String::new();
    let mut last_flush = 0usize;
    const FLUSH_AT: usize = 15;

    let client_delta = client.clone();
    let frame_delta = frame.clone();
    let sid_delta = stream_id.clone();

    let result = deepseek
        .chat_stream(&session_key, &content, |delta| {
            buf.push_str(delta);
            if buf.len() - last_flush >= FLUSH_AT {
                let c = client_delta.clone();
                let f = frame_delta.clone();
                let s = sid_delta.clone();
                let b = buf.clone();
                tokio::spawn(async move {
                    let _ = c.reply_stream(&f, &s, &b, false, None, None).await;
                });
                last_flush = buf.len();
            }
        })
        .await;

    match result {
        Ok(reply) => {
            info!("DeepSeek 回复(session={session_key}): {reply}");
            // 最终推送 finish=true
            let output = if buf.len() == reply.len() { &buf } else { &reply };
            client.reply_stream(frame, &stream_id, output, true, None, None).await?;
        }
        Err(e) => {
            error!("调用 DeepSeek API 失败(session={session_key}): {e}");
            if !buf.is_empty() {
                client.reply_stream(frame, &stream_id, &buf, true, None, None).await?;
            } else {
                client.reply_stream(frame, &stream_id, "抱歉，我现在有点问题，请稍后再试。", true, None, None).await?;
            }
        }
    }

    Ok(())
}