# wechatbot-rust

一个基于 Rust 的企业微信智能机器人 Agent，接入 DeepSeek 大模型实现自动回复，支持自动联网搜索、网页内容抓取、小红书图文搜索（camoufox 反指纹浏览器）和每日定时推送。

基于企业微信「AI 智能机器人」的 WebSocket 长连接（不是群机器人 Webhook）。

## 功能

- 通过 `wecom-aibot-rust-sdk` 与企业微信智能机器人建立 WebSocket 长连接。
- 接收用户文本消息后，调用 `ds-api` 请求 DeepSeek 生成回复，并通过智能机器人通道**流式返回**。
- **自动联网搜索**：通过 DeepSeek Tool Calling 调用 `web_search` 工具，自动搜索并把结果注入对话。
- **网页内容抓取**：通过 DeepSeek Tool Calling 调用 `fetch` 工具，自动抓取页面内容并返回摘要。
- **小红书搜索**：通过 **camoufox-rs**（反指纹 Firefox）绕过小红书的自动化检测，搜索图文笔记，并可获取笔记详情。
- **专属用户女仆角色**：`SPECIAL_USER_ID` 指定的用户与机器人聊天时使用专属女仆角色并称呼“主人”。
- **每日定时提醒**：按 `Asia/Shanghai` 时区在 09:00 / 12:00 / 18:00 向专属用户生成不固定的 AI 问候或用餐提醒。
- **每日 Rust 资讯推送**：每天上海时间 12:00 向专属用户单聊推送 [Rust.cc](https://rustcc.cn/) 日报（RSS）。

## 项目结构

```
wechatbot-rust/
├── src/
│   ├── main.rs              # 程序入口与命令行工具模式
│   ├── config.rs            # 环境变量与配置加载
│   ├── error.rs             # 全局错误类型
│   ├── deepseek.rs          # DeepSeek 客户端封装（Tool Calling）
│   ├── search.rs            # 联网搜索与网页抓取
│   ├── rss.rs               # Rust.cc RSS 资讯抓取
│   ├── wecom.rs             # 企业微信事件处理与定时任务
│   ├── xiaohongshu.rs       # 小红书搜索 / 笔记详情（HTTP 直连）
│   └── camoufox_backend.rs  # camoufox-rs 反指纹搜索（Unix only, feature 启用）
├── camoufox-rs/             # 本地依赖：camoufox-rs（反指纹浏览器自动化）
├── patches/                 # 本地 patch 依赖（wecom-aibot-rust-sdk、ds-api）
├── Cargo.toml
├── Dockerfile               # 多阶段构建，内嵌 camoufox 二进制
├── docker-compose.yml
├── .env.example
└── README.md
```

## 快速开始

### 1. 准备环境变量

```bash
cp .env.example .env
# 编辑 .env，填入 WECHAT_BOT_ID、WECHAT_BOT_SECRET 和 DEEPSEEK_API_KEY
# 可选：SPECIAL_USER_ID（专属用户）、XHS_ENABLED=true（启用小红书搜索）
```

### 2. 本地运行

> 仅原生 Windows / macOS 本地编译时，camoufox 相关功能（`xhs-camoufox` feature）仅限 Unix 启用；Linux 服务器上直接可用。

```bash
cargo run
```

### 3. 编译 Release

```bash
# 本地（Windows 等非 Unix 平台不带 camoufox feature）
cargo build --release

# Linux 服务器 / Docker 内（启用 camoufox 小红书搜索）
cargo build --release --features xhs-camoufox
# 产物: target/release/wechatbot-rust
```

### 4. Docker 部署（推荐）

Dockerfile 为多阶段构建：

1. 在容器内编译（`--features xhs-camoufox`）；
2. 下载并解压 **camoufox** 反指纹浏览器二进制到 `/opt/camoufox`；
3. 复制编译好的二进制，设置 `CAMOUFOX_BIN=/opt/camoufox`。

```bash
# 1. 配置 .env
cp .env.example .env

# 2. 构建并启动（后台运行）
docker compose up -d --build

# 3. 查看日志
docker compose logs -f wechatbot

# 4. 停止
docker compose down
```

`docker-compose.yml` 中预设：

- `TZ: Asia/Shanghai`（容器时区）
- `SPECIAL_USER_ID: 15771075163`
- `XHS_ENABLED: "true"`
- 挂载 `./xhs_cookie.txt:/app/xhs_cookie.txt:ro`（小红书 Cookie）

> **镜像大小提示**：主镜像内嵌了 camoufox 浏览器，体积较大。部署时可用
> `docker save -o wechatbot-rust-wechatbot.tar wechatbot-rust-wechatbot:latest`
> 打包后 scp 到服务器，再 `docker load -i wechatbot-rust-wechatbot.tar` 加载。

## 专属用户与定时任务

`SPECIAL_USER_ID` 默认 `15771075163`。该用户是机器人的“主人”，享受：

- **女仆角色回复**：与机器人聊天时使用专属女仆角色并称呼“主人”。
- **每日定时提醒**：按上海时间在 09:00 / 12:00 / 18:00 推送不固定的 AI 问候或用餐提醒（Morning / Lunch / Dinner）。
- **每日 RSS 日报**：每天 12:00 推送 [Rust.cc](https://rustcc.cn/) 日报到该用户的**单聊**（不再发到群聊）。

> 定时任务全部基于上海时区 `Asia/Shanghai`（代码内使用 `FixedOffset +8` 计算，与系统时区无关）。

## 命令行工具模式

除常驻运行外，程序还支持以下一键命令（用于验证链路 / 发送消息）：

| 命令 | 说明 |
|------|------|
| `--xhs-probe <关键词>` | 直接用 camoufox 搜索一次小红书，验证全链路 |
| `--xhs-detail-probe <URL>` | 用 HTTP 直连获取小红书笔记详情 |
| `--test-reminder morning/lunch/dinner` | 测试发送对应时段的定时提醒 |
| `--test-rss` | 测试向专属用户发送 Rust.cc 日报 |
| `--send <userid> <消息>` | 主动向指定用户发送一条 markdown 消息 |
| `--send-test <userid>` | 向指定用户发送一条“你好！这是一条测试消息” |

```bash
# 示例：主动发消息
cargo run -- --send 15771075163 "你好，这是一条主动消息"
```

> 主动推送单聊消息必须使用 `msgtype: markdown`（`text` 会报 40008）。

## 小红书搜索（camoufox-rs）

- **搜索链路**：`XHS_ENABLED=true` 时，DeepSeek 可通过 Tool Calling 调用 `xhs_search` / `xhs_note_detail` 搜索小红书图文。
- **反检测**：采用 **camoufox-rs**（反指纹 Firefox，默认隐藏 `navigator.webdriver` 等自动化特征），可绕过小红书的风控检测。仅在 `cfg(unix)` + `feature = "xhs-camoufox"` 时启用（Docker / Linux 部署自动满足）。
- **Cookie**：从项目根目录 `xhs_cookie.txt` 读取（不放入 `.env`，因过长会破坏 dotenvy 解析）。从浏览器开发者工具 → Application → Cookies 复制完整 Cookie 字符串（单行），有效期约 30 天。
- **排错日志**：搜索失败时会在日志中打印页面诊断（是否命中安全检查 / 验证码 / 登录失效），据此更新 Cookie 或更换网络环境。

## 配置说明

| 环境变量 | 必填 | 说明 |
|----------|------|------|
| `WECHAT_BOT_ID` | 是 | 企业微信智能机器人 Bot ID |
| `WECHAT_BOT_SECRET` | 是 | 企业微信智能机器人 Secret |
| `DEEPSEEK_API_KEY` | 是 | DeepSeek API Key |
| `DEEPSEEK_SYSTEM_PROMPT` | 否 | 系统提示词，默认“你是一个 helpful 的 AI 助手”。含空格时请用双引号包裹 |
| `SPECIAL_USER_ID` | 否 | 专属用户 ID，默认 `15771075163`，用于女仆回复、RSS 和定时提醒 |
| `XHS_ENABLED` | 否 | 设为 `true` 时启用小红书搜索工具 |
| `CAMOUFOX_BIN` | 否 | camoufox 二进制路径，Docker 内默认 `/opt/camoufox` |
| `RUST_LOG` | 否 | 日志级别，默认 `info`；调试用 `RUST_LOG=debug` |

## 服务器部署（systemd 可选）

使用 Docker Compose 是最简单的方式。如需用 systemd 托管 Docker 容器，可参考以下单元示例（关键点：设置 `TZ=Asia/Shanghai` 保证容器时区）：

```ini
[Unit]
Description=wechatbot-rust
After=docker.service
Requires=docker.service

[Service]
Restart=always
RestartSec=5
Environment=TZ=Asia/Shanghai
ExecStart=/usr/bin/docker compose -f /opt/wechatbot/docker-compose.yml up
ExecStop=/usr/bin/docker compose -f /opt/wechatbot/docker-compose.yml down

[Install]
WantedBy=multi-user.target
```

> 说明：代码内的定时任务已自行按 `Asia/Shanghai`（+8）计算，不依赖宿主机时区；设置 `TZ` 仅为让日志时间戳也显示为上海时间。

## 注意事项

- 本项目基于企业微信**智能机器人**（WebSocket 长连接），不是群机器人 Webhook。
- 企业微信同一 Bot 只允许一个长连接，本地调试时确保没有其他进程/容器占用。
- 企业微信智能机器人需要在企业微信后台开启“API 模式”并获取 `BotID` 和 `Secret`。
- 请妥善保管 `WECHAT_BOT_SECRET` 和 `DEEPSEEK_API_KEY`，不要硬编码到代码中。
- `.env` 中若值包含空格或特殊字符，建议用双引号包裹，否则可能导致解析失败。

## 许可证

本项目主体代码采用 [MIT](LICENSE) 许可证。
