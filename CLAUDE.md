# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

一个基于 Rust 的企业微信智能机器人 Agent，接入 DeepSeek 大模型实现自动回复，支持自动联网搜索、网页内容抓取和每日 Rust 资讯定时推送。基于企业微信「AI 智能机器人」的 WebSocket 长连接（不是群机器人 Webhook）。

## 常用命令

```bash
cargo build --release   # 编译 release（主要产物）
cargo run               # 本地运行（需配置 .env）
cargo check             # 快速类型检查
cargo test              # 运行单元测试（main.rs / rss.rs / search.rs 中有少量测试）
cargo test -- --ignored --nocapture  # 运行被 #[ignore] 的测试（需要网络或本地 Obscura CDP）
cargo doc --open        # 生成并打开文档
```

- 运行前需 `cp .env.example .env` 并填入 `WECHAT_BOT_ID`、`WECHAT_BOT_SECRET`、`DEEPSEEK_API_KEY`。
- **Windows 上编译 release 前，若 `wechatbot-rust.exe` 正在运行，需先 `taskkill //F //IM wechatbot-rust.exe` 再构建**，否则 `cargo build` 会报 `拒绝访问 (os error 5)`。

## 架构（大图）

数据流：企业微信消息 → WebSocket 回调 → `wecom.rs` → `deepseek.rs` → Tool Calling 触发搜索/抓取 → 流式回复返回。

### 模块划分

- **`src/main.rs`** — 入口：初始化日志、从多个候选路径加载 `.env`、组装配置并启动 `WecomBot`。
- **`src/wecom.rs`** — 企业微信事件处理。`WecomBot::run()` 注册所有 SDK 回调（连接/认证/断线/文本消息/进入会话），`handle_text_message` 调用 DeepSeek 并将回复分块流式推送给用户。关键：**用 `tokio::spawn` 在同步回调里跑异步逻辑**。
- **`src/deepseek.rs`** — DeepSeek 客户端核心。`DeepseekClient` 按 `session_key` 用 `DashMap` 隔离对话历史（单聊用 `userid`，群聊用 `chatid:userid`）。`chat_stream` 是核心方法，通过 DeepSeek Tool Calling 循环调用 `web_search` / `fetch`。
- **`src/search.rs`** — 封装 `rust-websearch`（无需 API Key，基于 DuckDuckGo）：`web_search` 和 `fetch_url_content`。支持 Obscura headless 浏览器作为反爬 fallback。
- **`src/rss.rs`** — 抓取 [Rust.cc](https://rustcc.cn/) RSS，解析并格式化当天资讯，供每日定时推送。
- **`src/config.rs`** — 从环境变量读取配置。
- **`src/error.rs`** — `AppError` 统一错误类型，桥接 `SdkError` 和 `ds_api::error::ApiError`。

## 联网搜索机制（关键设计，改动时务必理解）

当前已实现为 **DeepSeek Tool Calling**，不再是早期基于文本标记 `[SEARCH:...]` / `[FETCH:...]` 的方案。

1. `build_chat_request` 在请求中注册两个 `function` 类型工具：
   - `web_search` — 需要实时信息、新闻、价格等时使用。
   - `fetch` — 已知具体 URL 需要读取页面详细内容时使用。
2. `chat_stream` 先以**非流式**调用 `call_api_nostreaming`，拿到 assistant 消息后检查 `tool_calls`。
3. 若模型请求工具调用 → 解析参数 → 执行 `search::web_search` 或 `search::fetch_url_content` → 将结果作为 `Role::Tool` 消息注入历史 → 继续下一轮非流式调用。
4. 最多允许 `MAX_TOOL_ROUNDS = 100` 轮工具调用，到达上限后强制追加 system 消息要求模型直接给出最终回答。
5. 当模型不再调用工具时，如果处于上限后的兜底路径，调用 `stream_with_delta` 把最终回复流式返回；否则直接把 `assistant_msg.content` 返回。

### 工具结果注入格式

- `web_search` 返回格式化文本：
  ```
  以下是关于「{query}」的网络搜索结果：

  [1.] {title}
  来源: {url}
  摘要: {snippet}
  ```
- `fetch` 返回格式化文本（普通抓取或 Obscura 抓取）：
  ```
  以下是页面「{url}」的内容：

  {content}
  ```

这些文本会作为 `Role::Tool` 消息追加到对话历史中，`tool_call_id` 与对应工具调用 ID 匹配。

## 本地 patch 依赖（关键）

`Cargo.toml` 的 `[patch.crates-io]` 把两个依赖指向本地 `patches/` 目录，因为上游 crate 有 bug 需要修复：

- **`patches/wecom-aibot-rust-sdk`** — 修复首次连接时心跳帧与认证帧竞争导致断连（用 `timeout::interval_at` 延迟首次心跳）。
- **`patches/ds-api`** — 多处适配：
  - `Model` 从固定枚举改为透明新类型（`deepseek-v4-pro`/`deepseek-v4-flash` 等新模型名）。
  - 默认模型 `basic_query` = `deepseek-v4-pro`。
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
| `DAILY_NEWS_CHAT_ID` | 否 | 定时推送 Rust 日报的目标群聊 chatid，留空则不启用 |
| `RUST_LOG` | 否 | 日志级别，默认 `info`；调试用 `RUST_LOG=debug` |
| `OBSCURA_ENABLED` | 否 | 设为 `true` 时启用 Obscura 抓取 fallback |
| `OBSCURA_CDP_URL` | 否 | Obscura CDP 服务器地址，默认 `http://127.0.0.1:9222` |
| `OBSCURA_FETCH_MODE` | 否 | `always` / `fallback`（默认） / `never` |

## Docker Compose 部署（含 Obscura）

项目根目录的 `docker-compose.yml` 已将 Obscura 集成到同一编排中，通过自定义 bridge 网络 `wechatbot-net` 让 `wechatbot` 服务直接访问 `obscura` 服务，无需依赖宿主机的 `127.0.0.1:9222`。

要求：仓库根目录存在 `obscura/` 源码目录（用于构建 Obscura 镜像）。

```bash
docker compose up -d
docker compose logs -f wechatbot
docker compose down
```

`docker-compose.yml` 中为 `wechatbot` 服务覆盖了以下环境变量：

- `OBSCURA_ENABLED=true`
- `OBSCURA_CDP_URL=http://obscura:9222`
- `OBSCURA_FETCH_MODE=fallback`

因此 `.env` 中即使保留本地默认值 `http://127.0.0.1:9222`，在 compose 环境中也会自动改为容器间通信地址。若单独运行 Obscura（如 `docker run -p 9222:9222`），则需在 `.env` 中显式设置 `OBSCURA_CDP_URL=http://host.docker.internal:9222` 或宿主 IP。

## 关键注意点

- 企业微信智能机器人**被动回复用户消息必须用 `msgtype: stream`**（`reply_stream`），不能用 `text`（`text` 仅用于欢迎语），否则报错 40008。主动推送日报时使用 `msgtype: markdown`。
- 企业微信同一 Bot 只允许一个长连接，本地调试时确保没有其他进程/容器占用。
- 单会话历史上限 100 条，超出自动截断最早的对话对。
- 时区注意：日志时间戳为 UTC（`Z` 后缀），本地时间差 8 小时。
- `.env` 文件会从多个候选路径加载（当前目录、可执行文件目录、项目根目录等），详见 `main.rs::load_dotenv`。
