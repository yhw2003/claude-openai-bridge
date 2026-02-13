# claude-openai-bridge（Rust + Salvo）

让 Claude Code CLI 使用 OpenAI 兼容接口（`/chat/completions`）作为上游。

本服务接收 Claude 兼容请求（主要是 `POST /v1/messages`），完成请求/响应格式转换后转发到 OpenAI 兼容 API。

## 当前功能

- Claude 兼容接口：`POST /v1/messages`
- Claude 流式 SSE 事件转换（`message_start`/`content_block_delta`/`message_stop` 等）
- 工具调用双向转换（Claude `tool_use/tool_result` ↔ OpenAI `tool_calls/tool`）
- 图像输入转换（Claude `base64 image` -> OpenAI `image_url`）
- 模型映射（`haiku` / `sonnet` / 其他 -> `SMALL_MODEL` / `MIDDLE_MODEL` / `BIG_MODEL`）
- 上游原生模型直通（`gpt-*`、`o1-*`、`ep-*`、`doubao-*`、`deepseek-*`）
- 可选客户端 Key 校验（`ANTHROPIC_API_KEY`）
- Token 估算接口：`POST /v1/messages/count_tokens`
- 健康检查和上游连通性检查

## 接口列表

- `POST /v1/messages`
- `POST /v1/messages/count_tokens`
- `GET /health`
- `GET /test-connection`
- `GET /`

## 快速开始

1. 复制配置文件

```bash
cp .env.example .env
```

2. 至少配置 `OPENAI_API_KEY`

3. 启动服务

```bash
cargo run
```

默认监听：`0.0.0.0:8082`

## 与 Claude Code 搭配

```bash
ANTHROPIC_BASE_URL=http://localhost:8082 ANTHROPIC_API_KEY="any-value" claude
```

如果你在代理服务的 `.env` 设置了 `ANTHROPIC_API_KEY`，则客户端传入 key 必须完全一致（支持 `x-api-key` 或 `Authorization: Bearer ...`）。

## 环境变量

参考 `.env.example`。

### 必填

- `OPENAI_API_KEY`

### 常用可选

- `OPENAI_BASE_URL`（默认：`https://api.openai.com/v1`）
- `AZURE_API_VERSION`（设置后会作为 query 参数 `api-version` 附加到上游请求）
- `BIG_MODEL`（默认：`gpt-4o`）
- `MIDDLE_MODEL`（默认：跟随 `BIG_MODEL`，未设置时为 `gpt-4o`）
- `SMALL_MODEL`（默认：`gpt-4o-mini`）
- `HOST`（默认：`0.0.0.0`）
- `PORT`（默认：`8082`）
- `LOG_LEVEL`（默认：`INFO`）
- `REQUEST_TIMEOUT`（默认：`90`，非流式请求超时）
- `STREAM_REQUEST_TIMEOUT`（可选；>0 时生效，流式请求总超时）
- `REQUEST_BODY_MAX_SIZE`（默认：`16777216`，16MB）
- `DEBUG_TOOL_ID_MATCHING`（默认：`false`；为 `true` 时输出更详细的 tool id 匹配诊断日志）
- `CUSTOM_HEADER_*`（可选，自定义上游请求头）

### `CUSTOM_HEADER_*` 说明

例如：

- `CUSTOM_HEADER_X_API_KEY="xxx"` -> 上游头 `X-API-KEY: xxx`
- `CUSTOM_HEADER_ACCEPT="application/json"` -> 上游头 `ACCEPT: application/json`

规则：环境变量前缀 `CUSTOM_HEADER_` 去掉后，`_` 会转换为 `-`。

### 关于请求与 Token 控制

- 本服务不再提供 `max_tokens` 上下限配置。
- `max_tokens` 由下游调用方决定，并原样透传到上游。
- `REQUEST_TIMEOUT` / `STREAM_REQUEST_TIMEOUT` / `REQUEST_BODY_MAX_SIZE` 仅用于请求超时与报文大小保护，不参与 token 裁剪。

## 转换行为说明

### 请求转换（Claude -> OpenAI）

- `system` 文本会转换为 OpenAI `system` 消息
- `stop_sequences` -> `stop`
- `top_p` 透传
- `temperature` 默认 `1.0`
- `max_tokens` 原样透传（由下游控制）
- `tools[].input_schema` -> OpenAI `tools[].function.parameters`
- `tool_choice`：
  - `auto` / `any` -> `auto`
  - `tool` + `name` -> 指定函数调用
- 用户消息中的 `tool_result` 会拆成 OpenAI `tool` 角色消息
- 混合 `tool_result + text` 的用户消息会同时保留工具结果和普通文本

### 响应转换（OpenAI -> Claude）

- `choices[0].message.content` -> Claude `content[type=text]`
- `tool_calls` -> Claude `content[type=tool_use]`
- `finish_reason` 映射：
  - `length` -> `max_tokens`
  - `tool_calls` / `function_call` -> `tool_use`
  - 其他 -> `end_turn`
- `usage.prompt_tokens/completion_tokens` -> Claude `usage.input_tokens/output_tokens`

### 流式 SSE

- 自动向上游开启：`stream=true` + `stream_options.include_usage=true`
- 输出 Claude 风格事件：
  - `message_start`
  - `content_block_start`
  - `content_block_delta`
  - `content_block_stop`
  - `message_delta`
  - `message_stop`
  - `ping`
- `thinking` 兼容转换：支持从上游增量中的 `reasoning_content` / `reasoning` 及常见对象形态提取思考内容，并映射为 Claude `thinking_delta`
- 若下游请求开启 thinking，但上游未返回 reasoning 增量，代理会在流式过程中尽早发送一个空的 `thinking` block（仅状态，不伪造思考文本），避免 Claude 侧完全不显示 thinking 状态
- 触发上述 thinking 兜底时会输出 `INFO` 级日志（`phase=thinking_fallback_start`），包含模型、message_id、索引、stop_reason 与工具调用上下文
- 工具调用参数会累积到完整 JSON 后再发送 `input_json_delta`

## 诊断接口

- `GET /health`：返回服务状态、时间戳、API Key 配置状态等
- `GET /test-connection`：用 `SMALL_MODEL` 发起最小请求，验证上游可用性

## `count_tokens` 说明

`POST /v1/messages/count_tokens` 当前是**估算**逻辑，不调用上游 tokenizer：

- 统计 `system + messages` 文本字符数
- 按 `字符数 / 4` 估算
- 最小返回 `1`

## 开发与校验

修改后建议执行：

```bash
cargo check
cargo test
cargo clippy --all-targets --all-features
```
