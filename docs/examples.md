# 使用示例

这里整理一些常见配置和请求方式，方便直接复制后调整。

## 最小 OpenAI 兼容配置

```toml
[server]
port = 5564

[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"
```

请求：

```bash
curl http://127.0.0.1:5564/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai@gpt-4o",
    "messages": [
      { "role": "user", "content": "Hello" }
    ]
  }'
```

## 最小 Anthropic 兼容配置

```toml
[server]
port = 5564

[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
```

使用 Anthropic Messages 风格请求：

```bash
curl http://127.0.0.1:5564/anthropic/v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic@claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [
      { "role": "user", "content": "Hello" }
    ]
  }'
```

## 用 OpenAI 客户端访问 Anthropic Provider

Provider 仍然配置为 Anthropic：

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
body_policy = "drop_unknown"
```

客户端可以访问 OpenAI Chat 端点：

```bash
curl http://127.0.0.1:5564/openai/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic@claude-sonnet-4-20250514",
    "messages": [
      { "role": "user", "content": "用一句话介绍 Bifrost" }
    ]
  }'
```

Bifrost 会把 OpenAI Chat 风格请求转换为 Anthropic Messages 风格请求。

## 多 Deployment 轮询和指定 Deployment

同一个 Provider 可以配置多个命名 deployment，例如一个订阅套餐入口和一个按量付费入口：

```toml
[provider.openai]
endpoint = "openai"

[[provider.openai.deployments]]
id = "subscription"
base_url = "https://subscription.example.com/v1"
api_key = "sk-subscription"
weight = 3

[[provider.openai.deployments]]
id = "payg"
base_url = "https://api.openai.com/v1"
api_key = "sk-payg"
weight = 0
```

未指定 deployment 时，Bifrost 会在启用且 `weight > 0` 的 deployment 之间按权重轮询，并在 retryable 错误后尝试下一个 deployment。`weight = 0` 的 deployment 只允许手动指定，不参与自动轮询。失败的 deployment 会进入冷却期，后续未指定 deployment 的请求会暂时跳过它。临时指定某个 deployment 可以写在模型名后：

```json
{
  "model": "openai@gpt-4o#payg"
}
```

常用固定 deployment 可以写到 alias：

```toml
[alias."gpt4-payg"]
target = "openai@gpt-4o"
deployment = "payg"
```

请求头 `x-bifrost-deployment: payg` 会覆盖模型后缀和 alias 配置。

## 配置模型别名

```toml
[alias]
"sonnet" = "anthropic@claude-sonnet-4-20250514"
"gpt4o" = "openai@gpt-4o"
```

请求时可以使用短名称：

```json
{
  "model": "sonnet"
}
```

## Alias 附加参数

如果某个模型总是需要固定参数，可以使用复杂 alias：

```toml
[alias."sonnet-thinking"]
target = "anthropic@claude-sonnet-4-20250514"

[[alias."sonnet-thinking".body]]
name = "thinking_enabled"
value = true
condition = "anthropic"

[[alias."sonnet-thinking".body]]
name = "thinking_budget"
value = 16000
condition = "anthropic"
```

请求：

```json
{
  "model": "sonnet-thinking",
  "messages": [
    { "role": "user", "content": "分析这个问题" }
  ]
}
```

## 为某个模型添加请求头

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"

[[provider.anthropic.models]]
name = "claude-sonnet-4-20250514"

headers = [
  { name = "X-Model-Header", value = "sonnet-model" }
]
```

当请求 `anthropic@claude-sonnet-4-20250514` 时，这个 header 会被添加到上游请求。

## 只对某类客户端接口添加字段

```toml
[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"

body = [
  { name = "response_format", value = "json", condition = "openai_responses" },
  { name = "temperature", value = 0.7, condition = "openai_chat" }
]
```

这里的 `response_format` 只会在客户端访问 `/openai/responses` 或 `/openai/v1/responses` 时生效。

## 使用代理

```toml
[server]
port = 5564
proxy = "http://127.0.0.1:8080"
```

不需要代理时删除 `proxy` 字段即可。

## 查看日志和用量

```bash
# 实时查看日志
bifrost log --tail

# 查看当天用量
bifrost usage

# 查看月度 token 汇总
bifrost usage month
```
