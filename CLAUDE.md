# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

一个基于 Rust 的企业微信智能机器人 Agent，接入 DeepSeek 大模型实现自动回复，支持自动联网搜索和网页内容抓取。基于企业微信「AI 智能机器人」的 WebSocket 长连接（不是群机器人 Webhook）。

## 常用命令

```bash
cargo build --release   # 编译 release（主要产物）
cargo run               # 本地运行（需配置 .env）
cargo check             # 快速类型检查
cargo doc --open        # 生成并打开文档
```

- 无测试代码（`cargo test` 无测试）。
- 运行前需 `cp .env.example .env` 并填入 `WECHAT_BOT_ID`、`WECHAT_BOT_SECRET`、`DEEPSEEK_API_KEY`。
- **Windows 上编译 release 前，若 `wechatbot-rust.exe` 正在运行，需先 `taskkill //F //IM wechatbot-rust.exe` 再构建**，否则 `cargo build` 会报 `拒绝访问 (os error 5)`。

## 架构（大图）

数据流：企业微信消息 → WebSocket 回调 → `wecom.rs` → `deepseek.rs` → 搜索结果注入 → 流式回复返回

### 模块划分

- **`src/main.rs`** — 入口：加载 `.env`、初始化日志、组装配置并启动 `WecomBot`。
- **`src/wecom.rs`** — 企业微信事件处理。`WecomBot::run()` 注册所有 SDK 回调（连接/认证/断线/文本消息/进入会话），`handle_text_message` 调 DeepSeek 并流式推送回复。关键：**用 `tokio::spawn` 在同步回调里跑异步逻辑**。
- **`src/deepseek.rs`** — DeepSeek 客户端核心。`DeepseekClient` 按 `session_key` 用 `DashMap` 隔离对话历史（单聊用 `userid`，群聊用 `chatid:userid`）。`chat_stream` 是核心方法，内部循环处理「搜索/抓取」意图。
- **`src/search.rs`** — 封装 `rust-websearch`（无需 API Key，基于 DuckDuckGo）：`web_search` 和 `fetch_url_content`，返回格式化文本。
- **`src/config.rs`** — 从环境变量读取配置。
- **`src/error.rs`** — `AppError` 统一错误类型，桥接 `SdkError` 和 `ds_api::error::ApiError`。

### 联网搜索机制（关键设计，改动时务必理解）

DeepSeek 通过**文本标记**触发联网，而非工具调用（tool calling）：

1. `session_history` 在系统提示词里告知 DeepSeek 两种能力：
   - 搜索：输出 `[SEARCH:关键词]`
   - 网页抓取：输出 `[FETCH:完整URL]`
2. `chat_stream` 先**静默**（`stream_silent`，不推给用户）调 DeepSeek，返回后 `extract_intent` 解析回复里的标记。
3. 若命中标记 → 执行搜索/抓取 → 把结果作为 **System 消息**注入历史 → 再次静默调用让 DeepSeek 基于结果回答。
4. `loop` 最多 3 轮，防止死循环；达到上限后 `sanitize_action_marker` 清洗残留标记。
5. 重要约束：**第二轮及以后的调用必须用 `stream_silent`（静默），不能用流式推送**，否则 `[SEARCH:...]` 标记会泄漏给用户（之前修过这个 bug）。搜索/抓取前通过 `on_delta` 推送进度提示（"🔍 正在搜索..."）。

### 本地 patch 依赖（关键）

`Cargo.toml` 的 `[patch.crates-io]` 把两个依赖指向本地 `patches/` 目录，因为上游 crate 有 bug 需要修复：

- **`patches/wecom-aibot-rust-sdk`** — 修复首次连接时心跳帧与认证帧竞争导致断连（用 `timeout::interval_at` 延迟首次心跳）。
- **`patches/ds-api`** — 多处适配：
  - `Model` 从固定枚举改为透明新类型（`deepseek-v4-pro`/`deepseek-v4-flash` 等新模型名）。
  - 默认模型 `basic_query` = `deepseek-v4-flash`。
  - `system_fingerprint` 改为 `Option`（DeepSeek 响应可能不含该字段）。
  - 流式 chunk 的 `ChunkObjectType` 用 `#[serde(rename = "chat.completion.chunk")]` 修正（原 `lowercase` 会生成 `chatcompletionchunk` 导致反序列化失败）。

**改 `patches/` 里的代码时**：这些是复制的上游源码，改动会作用于整个项目。改完记得 `cargo build --release` 验证。

## 环境变量

| 变量 | 必填 | 说明 |
|------|------|------|
| `WECHAT_BOT_ID` | 是 | 企业微信智能机器人 Bot ID |
| `WECHAT_BOT_SECRET` | 是 | 企业微信智能机器人 Secret |
| `DEEPSEEK_API_KEY` | 是 | DeepSeek API Key |
| `DEEPSEEK_SYSTEM_PROMPT` | 否 | 系统提示词，默认"你是一个 helpful 的 AI 助手" |
| `RUST_LOG` | 否 | 日志级别，默认 `info`；调试用 `RUST_LOG=debug` |

## 关键注意点

- 企业微信智能机器人**被动回复用户消息必须用 `msgtype: stream`**（`reply_stream`），不能用 `text`（`text` 仅用于欢迎语），否则报错 40008。
- 企业微信同一 Bot 只允许一个长连接，本地调试时确保没有其他进程/容器占用。
- 单会话历史上限 100 条，超出自动截断最早的对话对。
- 时区注意：日志时间戳为 UTC（`Z` 后缀），本地时间差 8 小时。