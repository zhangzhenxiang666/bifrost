# 路由与模型格式

Bifrost 使用请求体中的 `model` 字段决定请求要转发到哪个上游 Provider。

最推荐的格式是：

```text
provider@model
```

例如：

```json
{
  "model": "openai@gpt-4o"
}
```

这里的 `openai` 对应配置文件中的 `[provider.openai]`，`gpt-4o` 是传给上游 Provider 的模型名。

## Provider 名称

Provider 名称来自配置节：

```toml
[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"
```

这个 Provider 可以通过下面的模型名访问：

```text
openai@gpt-4o
openai@gpt-4.1
openai@your-model-name
```

Bifrost 不要求模型必须提前写在 `models` 中。`models` 配置主要用于为特定模型追加 headers/body。

## Alias 路由

如果常用模型名太长，可以在 `[alias]` 中定义短名称：

```toml
[alias]
"sonnet" = "anthropic@claude-sonnet-4-20250514"
```

请求时可以直接写：

```json
{
  "model": "sonnet"
}
```

复杂 alias 可以附加 headers/body：

```toml
[alias."claude-sonnet"]
target = "anthropic@claude-sonnet-4-20250514"

[[alias."claude-sonnet".body]]
name = "enable_thinking"
value = false
```

## 路由优先级

模型字段解析顺序为：

1. 如果 `model` 是 `provider@model`，优先按显式 Provider 路由。
2. 如果不是 `provider@model`，尝试从 `[alias]` 中查找。
3. 如果找不到匹配项，请求会返回错误。

建议：

- 自动化脚本、服务端集成优先使用 `provider@model`，可读性最好。
- 人手写配置或常用模型可以使用 alias，输入更短。

## 客户端端点

Bifrost 暴露多种客户端兼容接口：

| 接口 | 说明 |
| ---- | ---- |
| `POST /openai/chat/completions` | OpenAI Chat Completions |
| `POST /openai/v1/chat/completions` | OpenAI Chat Completions |
| `POST /openai/responses` | OpenAI Responses API |
| `POST /openai/v1/responses` | OpenAI Responses API |
| `POST /anthropic/messages` | Anthropic Messages |
| `POST /anthropic/v1/messages` | Anthropic Messages |

客户端访问哪个端点，决定了 Bifrost 接收请求时的协议类型。上游 Provider 的 `endpoint` 决定了 Bifrost 转发请求时要转换成什么协议。

例如：

| 客户端访问 | 上游 Provider endpoint | 行为 |
| ---------- | ---------------------- | ---- |
| OpenAI Chat | `openai` | 基本透传 OpenAI 风格请求 |
| OpenAI Chat | `anthropic` | 转换为 Anthropic Messages 请求 |
| Anthropic Messages | `anthropic` | 基本透传 Anthropic 风格请求 |
| Anthropic Messages | `openai` | 转换为 OpenAI Chat 风格请求 |
| OpenAI Responses | `openai` | 转换为 OpenAI Chat 风格请求 |
| OpenAI Responses | `anthropic` | 先按 Responses 语义处理，再转换到 Anthropic |

## condition 与路由的关系

配置中的 `condition` 判断的是客户端访问 Bifrost 时使用的端点类型，不是最终上游 Provider 的 endpoint。

```toml
body = [
  { name = "temperature", value = 0.7, condition = "openai_chat" },
  { name = "thinking_enabled", value = true, condition = "anthropic" }
]
```

如果客户端访问 `/openai/v1/chat/completions`，则 `openai_chat` 生效。即使这个请求最后被转换并转发到 Anthropic Provider，`anthropic` 条件也不会因此生效。
