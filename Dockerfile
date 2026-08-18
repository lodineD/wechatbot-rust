# 第一阶段：编译
FROM rust:1.97.1-bookworm AS builder

WORKDIR /app
COPY . .

# 安装编译依赖（openssl-sys 需要）
RUN apt-get update && \
    apt-get install -y libssl-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# 编译 release 版本（启用 camoufox feature）
RUN cargo build --release --features xhs-camoufox

# 第二阶段：运行
FROM debian:bookworm-slim

WORKDIR /app

# 安装运行时依赖（HTTPS、SSL、时区、camoufox 需要的 X11 库）
RUN apt-get update && \
    apt-get install -y \
      ca-certificates \
      libssl3 \
      tzdata \
      libgtk-3-0 \
      libdbus-glib-1-2 \
      libasound2 \
      libx11-xcb1 \
      libxt6 \
      wget \
      unzip \
      && \
    rm -rf /var/lib/apt/lists/*

# 下载并安装 camoufox (x86_64 Linux)，保留原始解压目录结构（含运行时库）
RUN wget -q -O /tmp/camoufox.zip \
      https://github.com/daijro/camoufox/releases/download/v152.0.4-beta.28/camoufox-152.0.4-beta.28-lin.x86_64.zip && \
    mkdir -p /opt && \
    unzip -q /tmp/camoufox.zip -d /opt && \
    rm /tmp/camoufox.zip && \
    CAMOUFOX_BIN_LOC=$(find /opt -type f -name "camoufox" | head -1) && \
    echo "Found binary at: $CAMOUFOX_BIN_LOC" && \
    CAMOUFOX_DIR=$(dirname "$CAMOUFOX_BIN_LOC") && \
    echo "camoufox dir: $CAMOUFOX_DIR" && \
    chmod +x "$CAMOUFOX_BIN_LOC"

# 从构建阶段复制编译好的二进制文件
COPY --from=builder /app/target/release/wechatbot-rust /app/wechatbot-rust

# 设置 camoufox 环境变量（指向原始解压路径，保证运行时库可用）
ENV CAMOUFOX_BIN=/opt/camoufox

# 环境变量通过运行时注入，不将 .env 打包进镜像
CMD ["/app/wechatbot-rust"]
