# wechatbot-rust

一个基于 Rust 的企业微信智能机器人 Agent，接入 DeepSeek 大模型实现自动回复，支持自动联网搜索、网页内容抓取和每日 Rust 资讯定时推送。

## 功能

- 通过 `wecom-aibot-rust-sdk` 与企业微信智能机器人建立 WebSocket 长连接。
- 接收用户文本消息后，调用 `ds-api` 请求 DeepSeek 生成回复。
- **自动联网搜索**：通过 DeepSeek Tool Calling 调用 `web_search` 工具，自动搜索并把结果注入对话。
- **网页内容抓取**：通过 DeepSeek Tool Calling 调用 `fetch` 工具，自动抓取页面内容并返回摘要。
- **Obscura 反爬 fallback**：当普通抓取遇到反爬页面时，可启用 [Obscura](https://github.com/h4ckf0r0day/obscura) headless 浏览器重新抓取。
- **每日 Rust 资讯推送**：每天本地时间 09:10 向指定群聊推送 [Rust.cc](https://rustcc.cn/) 日报（RSS）。
- 将 DeepSeek 的回复通过企业微信智能机器人通道流式返回给用户。

## 项目结构

```
wechatbot-rust/
├── src/
│   ├── main.rs       # 程序入口
│   ├── config.rs     # 环境变量与配置加载
│   ├── error.rs      # 全局错误类型
│   ├── deepseek.rs   # DeepSeek 客户端封装
│   ├── search.rs     # 联网搜索与网页抓取
│   ├── rss.rs        # Rust.cc RSS 资讯抓取
│   └── wecom.rs      # 企业微信事件处理与生命周期管理
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
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

### 5. Docker Compose 运行（推荐，含 Obscura）

项目已将 [Obscura](https://github.com/h4ckf0r0day/obscura) headless 浏览器集成到 `docker-compose.yml`，通过 Docker bridge 网络与本项目通信，无需在宿主机暴露 `127.0.0.1:9222` 也能使用 Obscura fallback。

> **许可说明**：`obscura/` 目录为 [Obscura](https://github.com/h4ckf0r0day/obscura) 源码，采用 [Apache-2.0](https://github.com/h4ckf0r0day/obscura/blob/main/LICENSE) 许可证，与本项目主体（MIT）相互独立。

要求：仓库根目录存在 `obscura/` 源码目录（用于构建 Obscura 镜像）。

```bash
# 1. 确保 .env 已配置
cp .env.example .env

# 2. 构建并启动（后台运行）
docker compose up -d

# 3. 查看日志
docker compose logs -f wechatbot
docker compose logs -f obscura

# 4. 停止
docker compose down
```

`docker-compose.yml` 中已预设：

- `OBSCURA_ENABLED=true`
- `OBSCURA_CDP_URL=http://obscura:9222`（通过服务名在桥接网络内访问）
- `OBSCURA_FETCH_MODE=fallback`

因此 `.env` 中即使保留默认的 `http://127.0.0.1:9222`，在 compose 环境中也会被覆盖为容器间通信地址。

## 配置说明

| 环境变量 | 必填 | 说明 |
|----------|------|------|
| `WECHAT_BOT_ID` | 是 | 企业微信智能机器人 Bot ID |
| `WECHAT_BOT_SECRET` | 是 | 企业微信智能机器人 Secret |
| `DEEPSEEK_API_KEY` | 是 | DeepSeek API Key |
| `DEEPSEEK_SYSTEM_PROMPT` | 否 | 系统提示词，默认“你是一个 helpful 的 AI 助手”。含空格时请用双引号包裹 |
| `DAILY_NEWS_CHAT_ID` | 否 | 定时推送 Rust 日报的目标群聊 chatid，留空则不启用 |
| `RUST_LOG` | 否 | 日志级别，默认 `info` |
| `OBSCURA_ENABLED` | 否 | 设为 `true` 时启用 Obscura 抓取 fallback。使用 Docker Compose 时已默认开启 |
| `OBSCURA_CDP_URL` | 否 | Obscura CDP 服务器地址。Docker Compose 中默认 `http://obscura:9222`；本地独立运行 Obscura 时使用 `http://127.0.0.1:9222` |
| `OBSCURA_FETCH_MODE` | 否 | `always` 始终用 Obscura 抓取；`fallback`（默认）普通抓取失败/反爬时 fallback；`never` 禁用 |

## 每日 Rust 资讯推送

开启步骤：

1. 在企业微信后台获取目标群聊的 `chatid`。
2. 在 `.env` 中设置：
   ```env
   DAILY_NEWS_CHAT_ID=your_chat_id_here
   ```
3. 重新启动程序。

程序会在每天本地时间 09:10 自动抓取 [Rust.cc RSS](https://rustcc.cn/rss) 并推送到指定群聊。

## 注意事项

- 本项目基于企业微信**智能机器人**（WebSocket 长连接），不是群机器人 Webhook。
- 企业微信智能机器人需要在企业微信后台开启“API 模式”并获取 `BotID` 和 `Secret`。
- 请妥善保管 `WECHAT_BOT_SECRET` 和 `DEEPSEEK_API_KEY`，不要硬编码到代码中。
- `.env` 中若值包含空格或特殊字符，建议用双引号包裹，否则可能导致解析失败。

## 许可证

本项目主体代码采用 [MIT](LICENSE) 许可证。

`obscura/` 目录包含的 [Obscura](https://github.com/h4ckf0r0day/obscura) 浏览器源码采用 [Apache-2.0](https://github.com/h4ckf0r0day/obscura/blob/main/LICENSE) 许可证，版权归原作者所有。
