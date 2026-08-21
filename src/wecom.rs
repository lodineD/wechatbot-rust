use crate::deepseek::DeepseekClient;
use crate::error::AppError;
use crate::rss;
use chrono::{Duration as ChronoDuration, FixedOffset, NaiveTime, Utc};
use serde_json::json;
use tokio::time::Duration;
use tracing::{error, info, warn};
use wecom_aibot_rust_sdk::{SdkError, WSClient, WSClientOptions, WsFrame, generate_req_id};

/// 企业微信智能机器人客户端封装。
pub struct WecomBot {
    client: WSClient,
    deepseek: DeepseekClient,
}

impl WecomBot {
    pub fn new(
        bot_id: impl Into<String>,
        bot_secret: impl Into<String>,
        deepseek: DeepseekClient,
    ) -> Self {
        let options = WSClientOptions::new(bot_id.into(), bot_secret.into());
        let client = WSClient::new(options);
        Self { client, deepseek }
    }

    pub async fn test_reminder(
        &self,
        target_user_id: &str,
        scene: ReminderScene,
    ) -> Result<String, AppError> {
        self.client.connect().await?;
        let result =
            generate_and_send_reminder(&self.client, &self.deepseek, target_user_id, scene, None)
                .await;
        self.client.disconnect_async().await;
        result
    }

    pub async fn test_rss(&self, target_user_id: &str) -> Result<(), AppError> {
        self.client.connect().await?;
        let http = reqwest::Client::new();
        let result = fetch_and_push_news(&self.client, target_user_id, &http).await;
        self.client.disconnect_async().await;
        result
    }

    pub async fn run(&self, target_user_id: String) -> Result<(), AppError> {
        let client = self.client.clone();

        self.client
            .on_connected(move || info!("企业微信 WebSocket 已连接"))
            .await;
        self.client
            .on_authenticated(move || info!("企业微信机器人认证成功"))
            .await;
        self.client
            .on_disconnected(move |reason| warn!("企业微信 WebSocket 已断开: {reason}"))
            .await;
        self.client
            .on_reconnecting(move |attempt| info!("企业微信 WebSocket 第 {attempt} 次重连..."))
            .await;
        self.client
            .on_error(move |err| {
                if is_not_subscribed_error(err) {
                    error!("企业微信返回错误 846609：智能机器人未建立长连接订阅。请检查后台配置。");
                } else {
                    error!("企业微信 SDK 错误: {err}");
                }
            })
            .await;

        // 进入会话 → 欢迎语
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

        // 文本消息 → DeepSeek
        let deepseek = self.deepseek.clone();
        let client_for_msg = client.clone();
        self.client
            .on_message_text(move |frame| {
                let frame = frame.clone();
                let deepseek = deepseek.clone();
                let client = client_for_msg.clone();
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
                error!("连接失败：企业微信返回错误 846609。请检查后台配置。");
            }
            return Err(e.into());
        }

        info!("企业微信智能机器人已就绪");

        // 启动定时日报推送到专属用户
        let http_client = reqwest::Client::new();
        let chat_id = target_user_id.clone();
        let client_for_news = client.clone();

        // 在后台启动日报任务
        tokio::spawn(async move {
            if let Err(e) = schedule_daily_news(client_for_news, chat_id, &http_client).await {
                error!("定时日报推送失败: {e}");
            }
        });

        // 阻塞等待 Ctrl+C
        let client_for_reminders = client.clone();
        let deepseek_for_reminders = self.deepseek.clone();
        tokio::spawn(async move {
            if let Err(e) = schedule_personal_reminders(
                client_for_reminders,
                deepseek_for_reminders,
                target_user_id,
            )
            .await
            {
                error!("personal reminder scheduler failed: {e}");
            }
        });

        tokio::signal::ctrl_c().await.ok();
        info!("收到退出信号，正在断开连接...");

        self.client.disconnect();
        Ok(())
    }
}

/// 计算距离下一个 12:00（Asia/Shanghai）还有多少秒。
fn seconds_until_next_rss_noon() -> u64 {
    seconds_until_next(12, 0)
}

fn shanghai_now() -> chrono::DateTime<FixedOffset> {
    Utc::now().with_timezone(&FixedOffset::east_opt(8 * 60 * 60).unwrap())
}

fn seconds_until_next(hour: u32, minute: u32) -> u64 {
    seconds_until_next_from(shanghai_now(), hour, minute)
}

fn seconds_until_next_from(now: chrono::DateTime<FixedOffset>, hour: u32, minute: u32) -> u64 {
    let target_time = NaiveTime::from_hms_opt(hour, minute, 0).unwrap();
    let today_target = now.date_naive().and_time(target_time);

    let target = if now.naive_local() <= today_target {
        today_target
    } else {
        today_target + ChronoDuration::days(1)
    };

    let duration = target.signed_duration_since(now.naive_local());
    duration.num_seconds().max(0) as u64
}

/// 定时日报推送：等待到下一个上海时间 12:00，推送后循环。
async fn schedule_daily_news(
    client: WSClient,
    chat_id: String,
    http: &reqwest::Client,
) -> Result<(), AppError> {
    let cid = &chat_id;
    if chat_id.trim().is_empty() {
        info!("未配置专属用户 ID，跳过定时日报推送");
        return Ok(());
    };

    loop {
        let seconds = seconds_until_next_rss_noon();
        let delay = Duration::from_secs(seconds);
        info!("下次日报推送将在 {:?} 后 (12:00 Asia/Shanghai)", delay);
        tokio::time::sleep(delay).await;

        info!("开始推送 Rust.cc 日报...");
        match fetch_and_push_news(&client, cid, http).await {
            Ok(_) => info!("日报推送成功"),
            Err(e) => error!("日报推送失败: {e}"),
        }

        // 等 5 分钟再算下一个 12:00，避免同一分钟内重复触发
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

/// 获取 RSS 资讯并推送到指定用户单聊。
async fn fetch_and_push_news(
    client: &WSClient,
    chat_id: &str,
    http: &reqwest::Client,
) -> Result<(), AppError> {
    let content = rss::fetch_rust_news(http).await;

    // 主动推送只能用 msgtype: markdown（text 会报 40008）
    let body = json!({
        "msgtype": "markdown",
        "markdown": { "content": content }
    });

    client.send_message(chat_id, body).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn shanghai_schedule_uses_same_day_before_target() {
        let tz = FixedOffset::east_opt(8 * 60 * 60).unwrap();
        let now = tz.with_ymd_and_hms(2026, 8, 20, 8, 30, 0).unwrap();
        assert_eq!(seconds_until_next_from(now, 9, 0), 30 * 60);
    }

    #[test]
    fn shanghai_schedule_rolls_to_next_day_after_target() {
        let tz = FixedOffset::east_opt(8 * 60 * 60).unwrap();
        let now = tz.with_ymd_and_hms(2026, 8, 20, 18, 1, 0).unwrap();
        assert_eq!(seconds_until_next_from(now, 18, 0), 23 * 60 * 60 + 59 * 60);
    }

    #[test]
    fn reminder_prompts_match_requested_scenes() {
        assert_eq!(ReminderScene::Morning.time(), (9, 0));
        assert_eq!(ReminderScene::Lunch.time(), (12, 0));
        assert_eq!(ReminderScene::Dinner.time(), (18, 0));
        assert!(ReminderScene::Dinner.prompt().contains("下班"));
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReminderScene {
    Morning,
    Lunch,
    Dinner,
}

impl ReminderScene {
    fn time(self) -> (u32, u32) {
        match self {
            Self::Morning => (9, 0),
            Self::Lunch => (12, 0),
            Self::Dinner => (18, 0),
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            Self::Morning => {
                "现在是早上9点。请给主人写一条元气早安问候，关心今天的状态，并带一个轻松自然的小问题。"
            }
            Self::Lunch => {
                "现在是中午12点。请提醒主人按时吃午餐、稍作休息，并俏皮地问问主人准备吃什么。"
            }
            Self::Dinner => {
                "现在是晚上6点。请提醒主人吃晚饭、别加班太晚、早点下班，并用温柔俏皮的方式问候今天过得如何。"
            }
        }
    }
}

async fn schedule_personal_reminders(
    client: WSClient,
    deepseek: DeepseekClient,
    user_id: String,
) -> Result<(), AppError> {
    let morning = schedule_reminder(
        client.clone(),
        deepseek.clone(),
        user_id.clone(),
        ReminderScene::Morning,
    );
    let lunch = schedule_reminder(
        client.clone(),
        deepseek.clone(),
        user_id.clone(),
        ReminderScene::Lunch,
    );
    let dinner = schedule_reminder(client, deepseek, user_id, ReminderScene::Dinner);
    tokio::try_join!(morning, lunch, dinner)?;
    Ok(())
}

async fn schedule_reminder(
    client: WSClient,
    deepseek: DeepseekClient,
    user_id: String,
    scene: ReminderScene,
) -> Result<(), AppError> {
    let mut previous_message: Option<String> = None;
    loop {
        let (hour, minute) = scene.time();
        let delay = Duration::from_secs(seconds_until_next(hour, minute));
        info!("next {:?} reminder in {:?} (Asia/Shanghai)", scene, delay);
        tokio::time::sleep(delay).await;

        match generate_and_send_reminder(
            &client,
            &deepseek,
            &user_id,
            scene,
            previous_message.as_deref(),
        )
        .await
        {
            Ok(message) => {
                info!("sent {:?} reminder to {}: {}", scene, user_id, message);
                previous_message = Some(message);
            }
            Err(e) => error!("failed to send {:?} reminder: {e}", scene),
        }

        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn generate_and_send_reminder(
    client: &WSClient,
    deepseek: &DeepseekClient,
    user_id: &str,
    scene: ReminderScene,
    previous_message: Option<&str>,
) -> Result<String, AppError> {
    let date = shanghai_now().format("%Y-%m-%d %A");
    let prompt = format!("日期是 {date}（中国上海时间）。{}", scene.prompt());
    let mut message = deepseek
        .generate_scheduled_message(&prompt, previous_message)
        .await?;

    if previous_message.is_some_and(|previous| previous.trim() == message.trim()) {
        message = deepseek
            .generate_scheduled_message(
                &format!("{prompt}\n必须使用和上一条完全不同的开头与句式。"),
                previous_message,
            )
            .await?;
    }

    let body = json!({
        "msgtype": "markdown",
        "markdown": { "content": message }
    });
    client.send_message(user_id, body).await?;
    Ok(message)
}

fn is_not_subscribed_error(err: &SdkError) -> bool {
    let msg = err.to_string();
    msg.contains("846609") || msg.contains("aibot websocket not subscribed")
}

fn extract_text_content(frame: &WsFrame) -> Option<String> {
    frame
        .body
        .as_ref()?
        .get("text")?
        .get("content")?
        .as_str()
        .map(|s| s.trim().to_string())
}

fn extract_sender_user_id(frame: &WsFrame) -> String {
    frame
        .body
        .as_ref()
        .and_then(|body| body.get("from"))
        .and_then(|from| from.get("userid"))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn extract_session_key(frame: &WsFrame) -> String {
    let body = frame.body.as_ref();
    let ct = body
        .and_then(|b| b.get("chattype"))
        .and_then(|v| v.as_str());
    let uid = body
        .and_then(|b| b.get("from"))
        .and_then(|v| v.get("userid"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let cid = body.and_then(|b| b.get("chatid")).and_then(|v| v.as_str());
    match (ct, cid) {
        (Some("group"), Some(c)) => format!("{c}:{uid}"),
        _ => uid.to_string(),
    }
}

async fn send_welcome(client: &WSClient, frame: &WsFrame) -> Result<(), AppError> {
    let body =
        json!({"msgtype":"text","text":{"content":"您好！我是智能助手，有什么可以帮您的吗？"}});
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
    let sender_user_id = extract_sender_user_id(frame);
    info!("收到消息(session={session_key}): {content}");

    // 打印消息帧中的所有 ID 字段（用于调试/识别会话 ID）
    if let Some(body) = &frame.body {
        let chat_id = body.get("chatid").and_then(|v| v.as_str()).unwrap_or("-");
        let chat_type = body.get("chattype").and_then(|v| v.as_str()).unwrap_or("-");
        let userid = body
            .get("from")
            .and_then(|v| v.get("userid"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let msg_id = body.get("msgid").and_then(|v| v.as_str()).unwrap_or("-");
        let req_id = body.get("reqid").and_then(|v| v.as_str()).unwrap_or("-");
        info!(
            "消息 ID 汇总: chatid={chat_id} | chattype={chat_type} | userid={userid} | msgid={msg_id} | reqid={req_id}"
        );
        // 打印完整 JSON 便于发现其他有用字段
        info!(
            "消息帧完整 body: {}",
            serde_json::to_string(body).unwrap_or_default()
        );
    }

    let stream_id = generate_req_id("stream");

    // 发送占位提示
    let _ = client
        .reply_stream(frame, &stream_id, "正在思考...", false, None, None)
        .await;

    // 流式收集 + 批量推送
    let mut buf = String::new();

    let result = deepseek
        .chat_stream(&session_key, &sender_user_id, &content, |delta| {
            buf.push_str(delta);
        })
        .await;

    match result {
        Ok(reply) => {
            info!("DeepSeek 回复(session={session_key}): {reply}");
            // 模拟流式推送：把完整回复分块累积发送，最后 finish=true
            const CHUNK_SIZE: usize = 15;
            let chars: Vec<char> = buf.chars().collect();
            let mut pos = 0usize;
            while pos < chars.len() {
                pos = (pos + CHUNK_SIZE).min(chars.len());
                let text: String = chars[..pos].iter().collect();
                let is_last = pos == chars.len();
                client
                    .reply_stream(frame, &stream_id, &text, is_last, None, None)
                    .await?;
                if !is_last {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
            }
        }
        Err(e) => {
            error!("调用 DeepSeek API 失败(session={session_key}): {e}");
            if !buf.is_empty() {
                client
                    .reply_stream(frame, &stream_id, &buf, true, None, None)
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
