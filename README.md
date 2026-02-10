# claude-openai-bridge（Rust + Salvo）

让claude code cli可以使用openai api格式的llm api。

它接收 Claude 兼容请求（主要是 `/v1/messages`），并转发到 OpenAI 兼容上游接口（`/chat/completions`）。

## 功能特性

- Claude 兼容接口：`POST /v1/messages`
- Claude 流式 SSE 事件格式转换
- 工具调用（function/tool call）双向转换
- 图像输入转换（Claude `base64 image` -> OpenAI `image_url`）
- 模型映射（`haiku` / `sonnet` / `opus` -> 你配置的上游模型）
- 可选的客户端 Anthropic API Key 校验
- Token 估算接口：`POST /v1/messages/count_tokens`
- 健康检查与连通性测试接口

## 接口列表

- `POST /v1/messages`
- `POST /v1/messages/count_tokens`
- `GET /health`
- `GET /test-connection`
- `GET /`

## 快速开始

1. 复制配置文件：

```bash
cp .env.example .env
```

2. 至少配置：

- `OPENAI_API_KEY`

3. 启动服务：

```bash
cargo run
```

默认监听地址：`0.0.0.0:8082`

## 与 Claude Code 搭配使用

```bash
ANTHROPIC_BASE_URL=http://localhost:8082 ANTHROPIC_API_KEY="any-value" claude
```

如果你在代理 `.env` 中设置了 `ANTHROPIC_API_KEY`，则客户端传入的 key 必须完全一致。

## 环境变量说明

可参考 `.env.example`：

- 必填：`OPENAI_API_KEY`
- 常用可选：
  - `OPENAI_BASE_URL`
  - `BIG_MODEL`
  - `MIDDLE_MODEL`
  - `SMALL_MODEL`
  - `HOST`
  - `PORT`
  - `LOG_LEVEL`
  - `MAX_TOKENS_LIMIT`
  - `MIN_TOKENS_LIMIT`
  - `REQUEST_TIMEOUT`
  - `REQUEST_BODY_MAX_SIZE`
  - `CUSTOM_HEADER_*`

## 开发与验证

根据仓库约束，修改后应执行并通过：

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
```

## 说明

- 默认上游地址为 `https://api.openai.com/v1`。
- 支持通过 `CUSTOM_HEADER_*` 注入上游请求头。
- 流式响应事件遵循 Claude SSE 事件结构（如 `message_start`、`content_block_delta` 等）。
