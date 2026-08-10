# wecom-aibot-rust-sdk

企业微信智能机器人 Rust SDK —— 基于 WebSocket 长连接通道，提供消息收发、流式回复、模板卡片、事件回调、文件下载解密等核心能力。

本项目是企业微信官方 Node.js SDK（`@wecom/aibot-node-sdk`）的 Rust 重写版本，基于 `tokio` 异步运行时。

## 特性

- 🔗 **WebSocket 长连接** — 基于 `wss://openws.work.weixin.qq.com` 内置默认地址，开箱即用
- 🔐 **自动认证** — 连接建立后自动发送认证帧（bot_id + secret）
- 💓 **心跳保活** — 自动维护心跳，连续未收到 ack 时自动判定连接异常
- 🔄 **断线重连** — 指数退避重连策略（1s → 2s → 4s → ... → 30s 上限），支持自定义最大重连次数
- 📨 **消息分发** — 自动解析消息类型并触发对应事件（text / image / mixed / voice / file）
- 🌊 **流式回复** — 内置流式回复方法，支持 Markdown 和图文混排
- 🃏 **模板卡片** — 支持回复模板卡片消息、流式 + 卡片组合回复、更新卡片
- 📤 **主动推送** — 支持向指定会话主动发送 Markdown 或模板卡片消息，无需依赖回调帧
- 📡 **事件回调** — 支持进入会话、模板卡片按钮点击、用户反馈等事件
- 🔑 **文件下载解密** — 内置 AES-256-CBC 文件解密，每个图片/文件消息自带独立的 aeskey
- 🪵 **可插拔日志** — 支持自定义 Logger，内置带时间戳的 DefaultLogger

## 快速开始

```rust
use wecom_aibot_rust_sdk::{WSClient, WSClientOptions};

#[tokio::main]
async fn main() {
    let client = WSClient::new(
        WSClientOptions::new("your-bot-id", "your-bot-secret")
    );

    // 监听连接事件
    client.on_connected(|| {
        println!("WebSocket 已连接");
    });

    // 监听认证成功
    client.on_authenticated(|| {
        println!("认证成功");
    });

    // 连接
    client.connect().await.unwrap();

    // 保持运行
    tokio::signal::ctrl_c().await.ok();

    client.disconnect();
}
```

## 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                        WSClient                            │
│                      (client.rs)                           │
│  核心客户端：提供事件注册、消息收发、上传下载等 API          │
└──────────┬──────────┬──────────────┬───────────────────────-┘
           │          │              │
           ▼          ▼              ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│ WsConnectionMgr  │ │  MessageHandler  │ │  WeComApiClient  │
│  (ws.rs)         │ │(message_handler) │ │   (api.rs)       │
│ 连接管理          │ │ 消息解析与分发    │ │ HTTP 文件下载     │
│ 心跳/重连/认证    │ │                  │ │                  │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```

## 模块结构

```
src/
├── lib.rs              # 库入口文件，统一导出
├── client.rs           # WSClient 核心客户端
├── ws.rs               # WebSocket 长连接管理器（连接/心跳/重连/认证/上传）
├── message_handler.rs  # 消息解析与事件分发
├── api.rs              # HTTP API 客户端（文件下载、response_url 回复）
├── crypto_utils.rs     # AES-256-CBC 文件解密
├── logger.rs           # 默认日志实现
├── utils.rs            # 工具方法（generate_req_id 等）
└── types.rs            # 类型定义（枚举、结构体、常量）
```

### 各模块职责

| 模块 | 文件 | 职责 |
|------|------|------|
| `types.rs` | 276 行 | 定义 `SdkError`、`Logger`、`WSClientOptions`、`WsCmd`、`MessageType`、`WsFrame` 等基础类型 |
| `ws.rs` | 472 行 | `WsConnectionManager`：连接建立、认证、心跳、断线重连、消息发送、素材上传 |
| `client.rs` | 735 行 | `WSClient`：用户主入口，事件注册、回复方法、文件操作 |
| `message_handler.rs` | 193 行 | `MessageHandler`：把 `WsFrame` 解析为具体的 `MessageEvent` |
| `api.rs` | 168 行 | `WeComApiClient`：用 `reqwest` 实现 HTTP 文件下载 |
| `crypto_utils.rs` | 174 行 | `decrypt_file`：AES-256-CBC 解密 |
| `logger.rs` | 68 行 | `DefaultLogger`：带时间戳的日志实现 |
| `utils.rs` | 57 行 | `generate_req_id`：生成唯一请求 ID |

## 核心类型

### `WSClientOptions`

构建 `WSClient` 的配置项：

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `bot_id` | `String` | ✅ | — | 机器人 ID（企业微信后台获取） |
| `secret` | `String` | ✅ | — | 机器人 Secret（企业微信后台获取） |
| `reconnect_interval` | `u64` | — | `1000` | 重连基础延迟（毫秒），指数退避递增 |
| `max_reconnect_attempts` | `i32` | — | `10` | 最大重连次数（`-1` 表示无限重连） |
| `heartbeat_interval` | `u64` | — | `30000` | 心跳间隔（毫秒） |
| `request_timeout` | `u64` | — | `10000` | HTTP 请求超时（毫秒） |
| `ws_url` | `Option<String>` | — | `None` | 自定义 WebSocket 地址 |
| `logger` | `Option<Arc<dyn Logger>>` | — | `None` | 自定义日志实例 |

### `WsFrame`

WebSocket 帧结构（`body` 为原始 JSON，不解析具体字段）：

```rust
pub struct WsFrame {
    pub cmd: Option<String>,          // 命令类型
    pub headers: WsFrameHeaders,      // 包含 req_id
    pub body: Option<serde_json::Value>, // 消息体原始 JSON
    pub errcode: Option<i32>,         // 错误码
    pub errmsg: Option<String>,       // 错误信息
}
```

### `WsCmd` 常量

| 方向 | 命令 | 说明 |
|------|------|------|
| 开发者 → 企业微信 | `aibot_subscribe` | 订阅认证 |
| 开发者 → 企业微信 | `ping` | 心跳 |
| 开发者 → 企业微信 | `aibot_respond_msg` | 回复消息 |
| 开发者 → 企业微信 | `aibot_respond_welcome_msg` | 回复欢迎语 |
| 开发者 → 企业微信 | `aibot_respond_update_msg` | 更新模板卡片 |
| 开发者 → 企业微信 | `aibot_send_msg` | 主动推送 |
| 企业微信 → 开发者 | `aibot_msg_callback` | 消息回调 |
| 企业微信 → 开发者 | `aibot_event_callback` | 事件回调 |

## 消息类型

| 类型 | 值 | 说明 |
|------|-----|------|
| `MessageType::Text` | `"text"` | 文本消息 |
| `MessageType::Image` | `"image"` | 图片消息 |
| `MessageType::Mixed` | `"mixed"` | 图文混排消息 |
| `MessageType::Voice` | `"voice"` | 语音消息 |
| `MessageType::File` | `"file"` | 文件消息 |

## 事件类型

| 类型 | 值 | 说明 |
|------|-----|------|
| `EventType::EnterChat` | `"enter_chat"` | 进入会话事件 |
| `EventType::TemplateCardEvent` | `"template_card_event"` | 模板卡片事件 |
| `EventType::FeedbackEvent` | `"feedback_event"` | 用户反馈事件 |

## 事件注册

所有事件均通过 `on_*` 方法注册（均为 `async` 方法，需 `.await`）：

```rust
// 连接生命周期
client.on_connected(|| { ... }).await;
client.on_authenticated(|| { ... }).await;
client.on_disconnected(|reason| { ... }).await;
client.on_reconnecting(|attempt| { ... }).await;
client.on_error(|err| { ... }).await;

// 消息
client.on_message(|frame| { ... }).await;
client.on_message_text(|frame| { ... }).await;
client.on_message_image(|frame| { ... }).await;
client.on_message_mixed(|frame| { ... }).await;
client.on_message_voice(|frame| { ... }).await;
client.on_message_file(|frame| { ... }).await;

// 事件
client.on_event(|frame| { ... }).await;
client.on_event_enter_chat(|frame| { ... }).await;
client.on_event_template_card(|frame| { ... }).await;
client.on_event_feedback(|frame| { ... }).await;
```

> **注意**：回调闭包签名是 `Fn(&WsFrame) + Send + Sync + 'static`，**内部不能直接 `.await`**。需要在回调里用 `tokio::spawn` 派生异步任务。

## 回复消息

### 流式回复（`reply_stream`）

企业微信智能机器人**被动回复用户消息必须使用 `msgtype: stream`**（`text` 仅用于欢迎语）：

```rust
let stream_id = generate_req_id("stream");

// 发送中间内容（finish=false）
client.reply_stream(&frame, &stream_id, "正在思考...", false, None, None).await?;

// 发送最终内容（finish=true）
client.reply_stream(&frame, &stream_id, "最终回复", true, None, None).await?;
```

### 主动推送（`send_message`）

向指定会话主动推送消息，无需依赖收到的回调帧：

```rust
let body = json!({
    "msgtype": "markdown",
    "markdown": { "content": "**推送内容**" }
});
client.send_message(&chatid, body).await?;
```

### 欢迎语（`reply_welcome`）

收到 `enter_chat` 事件后需在 **5 秒内**调用，超时将无法发送：

```rust
client.on_event_enter_chat(move |frame| {
    let client = client.clone();
    let frame = frame.clone();
    tokio::spawn(async move {
        let body = json!({
            "msgtype": "text",
            "text": { "content": "您好！有什么可以帮您的吗？" }
        });
        let _ = client.reply_welcome(&frame, body).await;
    });
}).await;
```

## 文件操作

### 下载文件并解密

每个图片/文件消息自带独立 `aeskey`，需用于解密：

```rust
let page = frame.body.as_ref().unwrap();
let url = page["file"]["url"].as_str().unwrap();
let aes_key = page["file"]["aeskey"].as_str();
let (data, filename) = client.download_file(url, aes_key).await?;
```

### 上传临时素材

支持 image/voice/video/file 类型，`media_id` 有效期 3 天：

```rust
let result = client.upload_media("image", &file_data, "photo.png").await?;
```

**文件大小限制**：

| 类型 | 大小限制 | 格式 |
|------|---------|------|
| image | ≤10MB | JPG, PNG |
| voice | ≤2MB | AMR (≤60s) |
| video | ≤10MB | MP4 |
| file | ≤10MB | - |

## 自定义日志

实现 `Logger` trait 即可自定义日志输出：

```rust
use wecom_aibot_rust_sdk::Logger;

struct MyLogger;

impl Logger for MyLogger {
    fn debug(&self, msg: &str) { /* ... */ }
    fn info(&self, msg: &str) { /* ... */ }
    fn warn(&self, msg: &str) { /* ... */ }
    fn error(&self, msg: &str) { /* ... */ }
}
```

## 关键技术点

1. **两阶段事件分发**：`ws.rs` 的 `_receive_loop` 管理连接生命周期（心跳、重连），`client.rs` 的 `_receive_loop` 处理业务消息。两者通过 `mpsc` 通道解耦。

2. **req_id 透传**：回复消息时必须使用原始消息帧的 `req_id`，企业微信通过这个 id 关联请求和回复。

3. **不解析消息体**：SDK 把 `body` 作为 `serde_json::Value` 透传，用户自行提取字段（如 `text.content`、`from.userid`），避免版本耦合。

4. **回调闭包不能 await**：回调是同步闭包，异步逻辑需用 `tokio::spawn` 派生。

## License

MIT