# 协议转换

Bifrost 的核心能力之一，是让客户端和上游 Provider 可以使用不同协议。你可以用 OpenAI 风格客户端请求 Anthropic 兼容 Provider，也可以用 Anthropic 风格客户端请求 OpenAI 兼容 Provider。

## 基本概念

配置中的 `endpoint` 表示上游 Provider 使用的协议类型：

```toml
[provider.openai]
endpoint = "openai"

[provider.anthropic]
endpoint = "anthropic"
```

客户端访问 Bifrost 时使用的路径表示客户端请求协议：

- `/openai/chat/completions` 或 `/openai/v1/chat/completions`：OpenAI Chat Completions
- `/openai/responses` 或 `/openai/v1/responses`：OpenAI Responses
- `/anthropic/messages` 或 `/anthropic/v1/messages`：Anthropic Messages

Bifrost 会根据“客户端协议 + 上游 endpoint”自动选择转换方式。

## 转换矩阵

| 客户端请求 | 上游 endpoint | 说明 |
| ---------- | ------------- | ---- |
| OpenAI Chat | `openai` | OpenAI 风格请求转发到 OpenAI 兼容上游 |
| OpenAI Chat | `anthropic` | OpenAI Chat 请求转换为 Anthropic Messages |
| Anthropic Messages | `anthropic` | Anthropic 风格请求转发到 Anthropic 兼容上游 |
| Anthropic Messages | `openai` | Anthropic Messages 请求转换为 OpenAI Chat |
| OpenAI Responses | `openai` | Responses 请求转换为 Chat Completions |
| OpenAI Responses | `anthropic` | Responses 请求经过适配后转到 Anthropic Messages |

## OpenAI Responses

OpenAI Responses API 与 Chat Completions 的请求结构不同。Bifrost 会把 Responses 请求转换为内部可处理的 Chat 风格请求，再根据上游 Provider 的 endpoint 继续转换。

这意味着你可以用 Responses 风格的客户端接口访问 OpenAI 或 Anthropic 兼容 Provider：

```bash
curl http://127.0.0.1:5564/openai/v1/responses \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai@gpt-4o",
    "input": "Hello"
  }'
```

## 字段保留与过滤

协议转换时，有些字段会被 Bifrost 识别并转换，有些字段属于上游 Provider 的扩展字段。默认情况下，Bifrost 会尽量保留请求体中的字段。

如果上游 Provider 不接受未知字段，可以在 Provider 配置中使用 `body_policy`：

```toml
[provider.anthropic]
endpoint = "anthropic"
body_policy = "drop_unknown"
```

也可以使用 allowlist 或 blocklist：

```toml
body_policy = { allowlist = ["temperature", "top_p"] }
body_policy = { blocklist = ["prediction", "modalities"] }
```

更多细节见 [配置说明](configuration.md#body-policy)。

## Headers 和 Body 的追加时机

Provider、模型和 alias 中配置的 headers/body 会在请求处理过程中应用。它们可以带 `condition`，用于区分客户端访问的是 OpenAI Chat、OpenAI Responses 还是 Anthropic Messages。

```toml
body = [
  { name = "temperature", value = 0.7, condition = "openai_chat" },
  { name = "thinking_enabled", value = true, condition = "anthropic" }
]
```

注意：`condition` 判断的是客户端请求端点，不是转换后的上游协议。

## 使用建议

- 如果上游服务是 OpenAI 兼容接口，把 `endpoint` 设置为 `openai`。
- 如果上游服务是 Anthropic 兼容接口，把 `endpoint` 设置为 `anthropic`。
- 如果上游对未知字段严格，优先配置 `body_policy = "drop_unknown"`。
- 如果某个字段只适合某类客户端接口，使用 `condition` 限定它的生效范围。
- 如果常用模型需要固定附加参数，可以用 alias 的复杂映射封装。
