# 第一阶段：编译
FROM rust:1.80-bookworm AS builder

WORKDIR /app
COPY . .

# 编译 release 版本
RUN cargo build --release

# 第二阶段：运行
FROM debian:bookworm-slim

WORKDIR /app

# 从构建阶段复制编译好的二进制文件
COPY --from=builder /app/target/release/wechatbot-rust /app/wechatbot-rust

# 环境变量通过运行时注入，不将 .env 打包进镜像
CMD ["./wechatbot-rust"]
