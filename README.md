# wechatbot-rust

一个基于 Rust 的企业微信智能机器人 Agent，接入 DeepSeek 大模型实现自动回复。

## 功能

- 通过 `wecom-aibot-rust-sdk` 与企业微信智能机器人建立 WebSocket 长连接。
- 接收用户文本消息后，调用 `ds-api` 请求 DeepSeek 生成回复。
- 将 DeepSeek 的回复通过企业微信智能机器人通道返回给用户。

## 项目结构

```
wechatbot-rust/
├── src/
│   ├── main.rs       # 程序入口
│   ├── config.rs     # 环境变量与配置加载
│   ├── error.rs      # 全局错误类型
│   ├── deepseek.rs   # DeepSeek 客户端封装
│   └── wecom.rs      # 企业微信事件处理与生命周期管理
├── Cargo.toml
├── Dockerfile
├── .env.example
└── README.md
```

## 快速开始

### 1. 准备环境变量

```bash
cp .env.example .env
# 编辑 .env，填入 WECHAT_BOT_ID、WECHAT_BOT_SECRET 和 DEEPSEEK_API_KEY
```

### 2. 本地运行

```bash
cargo run
```

### 3. 编译 Release

```bash
cargo build --release
# 产物: target/release/wechatbot-rust
```

### 4. Docker 构建与运行

```bash
docker build -t wechatbot-rust .
docker run --env-file .env wechatbot-rust
```

## 配置说明

| 环境变量 | 必填 | 说明 |
|----------|------|------|
| `WECHAT_BOT_ID` | 是 | 企业微信智能机器人 Bot ID |
| `WECHAT_BOT_SECRET` | 是 | 企业微信智能机器人 Secret |
| `DEEPSEEK_API_KEY` | 是 | DeepSeek API Key |
| `DEEPSEEK_SYSTEM_PROMPT` | 否 | 系统提示词，默认“你是一个 helpful 的 AI 助手” |
| `RUST_LOG` | 否 | 日志级别，默认 `info` |

## 注意事项

- 本项目基于企业微信**智能机器人**（WebSocket 长连接），不是群机器人 Webhook。
- 企业微信智能机器人需要在企业微信后台开启“API 模式”并获取 `BotID` 和 `Secret`。
- 请妥善保管 `WECHAT_BOT_SECRET` 和 `DEEPSEEK_API_KEY`，不要硬编码到代码中。

## 许可证

MIT
