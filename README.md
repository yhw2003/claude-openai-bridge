# claude-openai-bridge（Rust + Salvo）

让 Claude Code CLI 使用 OpenAI 兼容接口（`/chat/completions` 或 `/responses`）作为上游。

本服务接收 Claude 兼容请求（主要是 `POST /v1/messages`），完成请求/响应格式转换后转发到 OpenAI 兼容 API。

## 项目状态

在多次迭代后，它终于来到了一个基本可用状态，目前这个代理基本功能完整，支持`chat`和`responses`两种`wire_api`，测试中`claude code`在多个长达数小时的工作里运行正常，可以完成任务。对于中转api提供了良好的优化，使得在使用中转api时仍然可以命中上游模型缓存（具体也需要取决于上游中转站的实现，在我这里它可以命中）。

开发计划：
  - [ ] 更严肃的session_id路由规则
  - [ ] 支持多种下游api
  - [ ] 实现接口在运行时修改配置
  - [ ] 实现多用户支持

## 当前功能

- Claude 兼容接口：`POST /v1/messages`
- Claude 流式 SSE 事件转换（`message_start`/`content_block_delta`/`message_stop` 等）
- 工具调用双向转换（Claude `tool_use/tool_result` ↔ OpenAI `tool_calls/tool`）
- 图像输入转换（Claude `base64 image` -> OpenAI `image_url`）
- 模型映射（`haiku` / `sonnet` / 其他 -> `SMALL_MODEL` / `MIDDLE_MODEL` / `BIG_MODEL`）
- 上游原生模型直通（`gpt-*`、`o1-*`、`ep-*`、`doubao-*`、`deepseek-*`）
- 会话粘性 session_id（按请求身份复用，提升中转 API 网关路由缓存命中）
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

1. 复制推荐配置文件

```bash
cp config.toml.example config.toml
```

2. 至少配置 `openai_api_key`

3. 启动服务

```bash
cargo run
```

默认监听：`0.0.0.0:8082`

## 与 Claude Code 搭配

```bash
ANTHROPIC_BASE_URL=http://localhost:8082 ANTHROPIC_API_KEY="any-value" claude
```

> 作用域说明：`ANTHROPIC_BASE_URL` 是 **Claude Code CLI 侧环境变量**，用于让 Claude Code 把请求发到本代理地址；它不是代理服务端配置项，代理进程不会在 `src/config.rs` 中读取该变量。

如果你在代理服务的 `config.toml` 或环境变量中设置了 `anthropic_api_key` / `ANTHROPIC_API_KEY`，则客户端传入 key 必须完全一致（支持 `x-api-key` 或 `Authorization: Bearer ...`）。

## 推荐配置方式（`config.toml`）

推荐使用 `config.toml` 作为主配置，参考 `config.toml.example`。

配置优先级：**环境变量 > `config.toml` > 代码默认值**。

### 配置映射对照（env ↔ toml）

| 环境变量 | `config.toml` 键 | 默认值 / 说明 |
|---|---|---|
| `OPENAI_API_KEY` | `openai_api_key` | **必填** |
| `ANTHROPIC_API_KEY` | `anthropic_api_key` | 可选；用于校验客户端请求 key |
| `OPENAI_BASE_URL` | `openai_base_url` | `https://api.openai.com/v1` |
| `AZURE_API_VERSION` | `azure_api_version` | 可选；附加为 query 参数 `api-version` |
| `WIRE_API` | `wire_api` | `chat`（可选：`chat` / `responses`） |
| `MIN_THINKING_LEVEL` | `min_thinking_level` | 可选：`low` / `medium` / `high`；作为 `reasoning_effort` 下限，仅对支持该字段的模型生效 |
| `BIG_MODEL` | `big_model` | `gpt-4o` |
| `MIDDLE_MODEL` | `middle_model` | 默认继承 `big_model` |
| `SMALL_MODEL` | `small_model` | `gpt-4o-mini` |
| `HOST` | `host` | `0.0.0.0` |
| `PORT` | `port` | `8082` |
| `LOG_LEVEL` | `log_level` | `INFO` |
| `REQUEST_TIMEOUT` | `request_timeout` | `90` |
| `STREAM_REQUEST_TIMEOUT` | `stream_request_timeout` | 可选；仅当 `>0` 时生效 |
| `REQUEST_BODY_MAX_SIZE` | `request_body_max_size` | `16777216`（16MB） |
| `SESSION_TTL_MIN_SECS` | `session_ttl_min_secs` | `1800` |
| `SESSION_TTL_MAX_SECS` | `session_ttl_max_secs` | `86400` |
| `SESSION_CLEANUP_INTERVAL_SECS` | `session_cleanup_interval_secs` | `60` |
| `DEBUG_TOOL_ID_MATCHING` | `debug_tool_id_matching` | `false`；开启后输出 tool_call_id 匹配诊断日志 |

### 必填

- `openai_api_key`

### 常用可选

- `openai_base_url`（默认：`https://api.openai.com/v1`）
- `azure_api_version`（设置后会作为 query 参数 `api-version` 附加到上游请求）
- `big_model`（默认：`gpt-4o`）
- `middle_model`（默认：跟随 `big_model`，未设置时为 `gpt-4o`）
- `small_model`（默认：`gpt-4o-mini`）
- `host`（默认：`0.0.0.0`）
- `port`（默认：`8082`）
- `log_level`（默认：`INFO`）
- `request_timeout`（默认：`90`，非流式请求超时）
- `stream_request_timeout`（可选；>0 时生效，流式请求总超时）
- `request_body_max_size`（默认：`16777216`，16MB）
- `debug_tool_id_matching`（默认：`false`；为 `true` 时输出更详细的 tool_call_id 匹配诊断日志）
- `min_thinking_level`（可选：`low` / `medium` / `high`；作为上游 `reasoning_effort` 的最小等级，仅对支持 `reasoning_effort` 的模型生效）
- `wire_api`（默认：`chat`；可选 `chat` / `responses`，详见下文“`WIRE_API` 选择”）
- `session_ttl_min_secs`（默认：`1800`）
- `session_ttl_max_secs`（默认：`86400`）
- `session_cleanup_interval_secs`（默认：`60`）
- `[custom_headers]`（可选，自定义上游请求头）

### 会话粘性（session_id）

为提升部分中转 API 网关的路由缓存命中率，代理会按请求身份生成并复用上游 `session_id`：

- 相同身份（优先 `x-device-id`，否则退化到认证+IP 指纹）在 TTL 有效期内复用同一个 `session_id`
- 不同身份使用不同 `session_id`
- 会话会按访问频率与累计 token 动态延长存活时间，范围由以下配置控制：
  - `session_ttl_min_secs`（默认 1800）
  - `session_ttl_max_secs`（默认 86400）
  - `session_cleanup_interval_secs`（默认 60）

说明：该机制仅影响上游请求路由与缓存亲和性，不改变 Claude 协议语义。

### `min_thinking_level` 说明

`min_thinking_level` 用于给上游请求的 `reasoning_effort` 设置一个全局下限。

- 可选值：`low` / `medium` / `high`（大小写不敏感，内部会归一化）
- 未配置时：不设置全局下限，按请求自身的 thinking 推导
- 配置后：最终 `reasoning_effort = max(请求推导结果, min_thinking_level)`
- 仅对支持 `reasoning_effort` 的模型生效；不支持的模型会忽略该字段

示例：

```bash
MIN_THINKING_LEVEL=medium
```

```toml
min_thinking_level = "medium"
```

### `[custom_headers]` 说明

可通过 `config.toml` 的 `[custom_headers]` 或环境变量 `CUSTOM_HEADER_*` 两种方式配置：

```toml
[custom_headers]
X-API-KEY = "xxx"
ACCEPT = "application/json"
```

```bash
CUSTOM_HEADER_X_API_KEY=xxx
CUSTOM_HEADER_ACCEPT=application/json
```

`CUSTOM_HEADER_*` 映射规则：去掉前缀后将 `_` 转为 `-`，例如 `CUSTOM_HEADER_X_API_KEY` -> `X-API-KEY`。

当 `config.toml` 与环境变量同时配置同名请求头时，环境变量值会覆盖 `config.toml`。

### 环境变量覆盖（可选）

如需在部署时临时覆盖配置，可使用同名环境变量（例如 `OPENAI_API_KEY`、`WIRE_API`、`MIN_THINKING_LEVEL`、`SESSION_TTL_MIN_SECS`、`CUSTOM_HEADER_X_API_KEY`）。

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

## `WIRE_API` 选择

该桥接服务支持两种上游 wire API：

- `chat`（默认）：调用 `/chat/completions`
- `responses`：调用 `/responses`

可通过环境变量或 `config.toml` 设置：

```bash
WIRE_API=responses
```

```toml
wire_api = "responses"
```
