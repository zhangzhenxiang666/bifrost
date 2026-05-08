# 配置说明

Bifrost 默认读取 `~/.bifrost/config.toml`。安装脚本会自动创建一个默认配置文件，你也可以参考仓库根目录的 `config.toml` 自行调整。

配置主要分为三部分：

- `[server]`：本地服务的监听端口、代理、超时和重试策略。
- `[provider.<name>]`：上游模型提供商配置。
- `[alias]`：模型短名称到 `provider@model` 的映射。

## Server 配置

```toml
[server]
port = 5564
timeout_secs = 600
max_retries = 5
retry_backoff_base_ms = 700
retry_status_codes = [429, 500, 502, 503, 504]
proxy = "http://127.0.0.1:8080"
```

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `port` | `u16` | `5564` | Bifrost 本地服务监听端口 |
| `timeout_secs` | `u64` | `600` | 请求上游 Provider 的超时时间，单位为秒 |
| `max_retries` | `u32` | `5` | 上游请求失败后的最大重试次数 |
| `retry_backoff_base_ms` | `u64` | `700` | 指数退避的基础延迟，单位为毫秒 |
| `retry_status_codes` | `Array<u16>` | `[429, 500, 502, 503, 504]` | 触发重试的 HTTP 状态码，会与默认值合并 |
| `proxy` | `String` | 无 | 可选 HTTP 代理地址 |

如果不需要代理，可以删除 `proxy` 字段。

## Provider 配置

Provider 表示一个上游模型服务。Provider 名称会成为路由模型时的前缀，例如 `[provider.openai]` 对应 `openai@gpt-4o`。

### OpenAI 兼容 Provider

```toml
[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"
```

### Anthropic 兼容 Provider

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
```

### Provider 字段

| 字段 | 类型 | 默认值 | 必填 | 说明 |
| ---- | ---- | ------ | ---- | ---- |
| `base_url` | `String` | 无 | 是 | 上游 Provider 的 API 地址 |
| `api_key` | `String` | 无 | 是 | 上游 Provider 的 API key |
| `endpoint` | `String` | `openai` | 否 | 上游端点类型，可选 `openai` 或 `anthropic` |
| `headers` | `Array` | 无 | 否 | Provider 级别的额外请求头，会添加到所有匹配请求 |
| `body` | `Array` | 无 | 否 | Provider 级别的额外请求体字段，会合并到请求体 |
| `exclude_headers` | `Array<String>` | 无 | 否 | 从客户端原始请求中排除的 header |
| `extend` | `bool` | `false` | 否 | 是否继承客户端原始请求 headers |
| `body_policy` | `String` 或 `Table` | 保留所有字段 | 否 | 请求体字段过滤策略 |
| `models` | `Array` | 无 | 否 | 模型级别配置 |

`endpoint` 描述的是上游 Provider 的协议类型，不是客户端访问 Bifrost 时使用的接口。客户端可以访问 OpenAI 或 Anthropic 风格接口，Bifrost 会根据上游 endpoint 自动选择转换方式。

## Headers 和 Body 扩展

你可以在 Provider 或模型上配置额外的 headers/body。它们适合用于传递特殊开关、实验参数、供应商自定义字段等。

```toml
[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"

headers = [
  { name = "X-Provider-Header", value = "provider-value" }
]

body = [
  { name = "extra_field", value = "extra-value" }
]
```

字段格式为：

```toml
{ name = "X-Header-Name", value = "header-value" }
{ name = "body_field", value = "field-value" }
```

## 条件生效

`headers` 和 `body` 都支持可选字段 `condition`。它表示该字段只在客户端访问某类 Bifrost 端点时生效。

有效值：

| condition | 匹配的客户端接口 |
| --------- | ---------------- |
| `openai_chat` 或 `openai-chat` | `/openai/chat/completions`、`/openai/v1/chat/completions` |
| `openai_responses` 或 `openai-responses` | `/openai/responses`、`/openai/v1/responses` |
| `anthropic` | `/anthropic/messages`、`/anthropic/v1/messages` |

示例：

```toml
headers = [
  { name = "X-Chat-Only", value = "chat-value", condition = "openai_chat" },
  { name = "X-Anthropic-Only", value = "anthropic-value", condition = "anthropic" },
  { name = "X-Common-Header", value = "common-value" }
]

body = [
  { name = "response_format", value = "json", condition = "openai-responses" },
  { name = "thinking_enabled", value = true, condition = "anthropic" }
]
```

匹配规则：

- `condition` 匹配客户端访问的端点类型时，该字段会被应用。
- `condition` 不匹配时，该字段不会被应用。
- 省略 `condition` 时，该字段对所有端点生效。

注意：`condition` 看的不是上游 Provider 的 `endpoint`，而是客户端请求 Bifrost 时使用的接口类型。

## 模型级配置

模型级配置适合给某个模型单独添加 headers/body。模型级字段优先级高于 Provider 级字段。

```toml
[[provider.anthropic.models]]
name = "claude-sonnet-4-20250514"

headers = [
  { name = "X-Model-Header", value = "sonnet-model" }
]

body = [
  { name = "thinking_budget", value = 16000, condition = "anthropic" }
]
```

| 字段 | 类型 | 默认值 | 必填 | 说明 |
| ---- | ---- | ------ | ---- | ---- |
| `name` | `String` | 无 | 是 | 上游模型名称 |
| `headers` | `Array` | 无 | 否 | 该模型的额外请求头 |
| `body` | `Array` | 无 | 否 | 该模型的额外请求体字段 |

## Body Policy

`body_policy` 用于控制请求体字段过滤。它常用于上游 Provider 对 unknown field 比较严格的场景。

| 格式 | 说明 |
| ---- | ---- |
| 省略 | 保留所有字段 |
| `"drop_unknown"` | 丢弃所有转换逻辑未处理的字段 |
| `{ allowlist = ["field1", "field2"] }` | 只保留指定字段 |
| `{ blocklist = ["field1", "field2"] }` | 丢弃指定字段 |

示例：

```toml
# 丢弃所有未处理字段
body_policy = "drop_unknown"

# 仅保留指定字段
body_policy = { allowlist = ["temperature", "top_p"] }

# 丢弃指定字段
body_policy = { blocklist = ["prediction", "modalities"] }
```

## Alias 配置

Alias 可以把短模型名映射到完整的 `provider@model`。这样客户端无需记忆 Provider 前缀。

### 简单映射

```toml
[alias]
"sonnet" = "anthropic@claude-sonnet-4-20250514"
```

客户端可以直接使用：

```json
{
  "model": "sonnet"
}
```

### 复杂映射

复杂映射支持在 alias 上追加 headers/body。

```toml
[alias."claude-sonnet"]
target = "anthropic@claude-sonnet-4-20250514"

[[alias."claude-sonnet".headers]]
name = "X-Custom-Header"
value = "custom-value"

[[alias."claude-sonnet".body]]
name = "enable_thinking"
value = false
```

| 字段 | 类型 | 必填 | 说明 |
| ---- | ---- | ---- | ---- |
| `target` | `String` | 是 | 目标 `provider@model` |
| `headers` | `Array` | 否 | alias 级额外请求头 |
| `body` | `Array` | 否 | alias 级额外请求体字段 |

Alias 也支持 `condition`，规则与 Provider 的 headers/body 相同。

## 路由优先级

模型解析优先级为：

1. `provider@model` 格式。
2. `[alias]` 中定义的短名称。
3. 如果都无法匹配，请求会报错。

也就是说，显式写 `provider@model` 永远最清晰，alias 更适合常用模型的简短入口。
