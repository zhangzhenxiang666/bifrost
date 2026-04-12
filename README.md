# Bifrost

LLM 本地代理服务 - 统一管理多模型提供商，一键切换 API 端点。

## 特性

- **统一端点**: 提供 OpenAI 和 Anthropic 兼容端点，无需修改应用代码
- **智能路由**: 通过 `provider@model` 格式自动路由到对应模型提供商
- **协议转换**: 内置 OpenAI ↔ Qwen ↔ Anthropic 协议转换器
- **本地 Qwen 支持**: 可调用本地 Qwen CLI 进行推理
- **CLI 管理**: 简单的命令行工具管理服务器启停和状态查看

## 快速开始

### 安装

**Linux / macOS (bash/zsh/fish):**

```bash
curl -fsSL https://raw.githubusercontent.com/zhangzhenxiang666/bifrost/main/scripts/install.sh | bash
```

安装脚本会自动：

- 下载最新版本的 `bifrost` 和 `bifrost-server`
- 安装到 `~/.bifrost/bin/`
- 创建默认配置文件 `~/.bifrost/config.toml`
- 配置 PATH 环境变量

### 启动服务

```bash
bifrost start
```

### 示例配置 Provider

编辑 `~/.bifrost/config.toml`:

```toml
[server]
port = 5564
timeout_secs = 600
max_retries = 5

[provider.qwen-code]
base_url = "https://portal.qwen.ai/v1"
api_key = "any-key"
endpoint = "openai"
adapter = "openai-to-qwen"

[provider.openai]
base_url = "https://openai.com/v1"
api_key = "your-key"
endpoint = "openai"

[provider.an-qwen]
base_url = "https://portal.qwen.ai/v1"
api_key = "any-key"
endpoint = "anthropic"
adapter = "anthropic-to-qwen"

[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
```

### 使用

将你的应用 API 地址设置为:

```text
http://localhost:5564/openai
# 或
http://localhost:5564/anthropic
```

然后将请求中的 `model` 字段改为 `provider@model` 格式即可路由到对应模型：

```json
{
  "model": "qwen@qwen-plus"
}
```

## CLI 命令

| 命令 | 说明 |
| ---- | ---- |
| `bifrost start` | 启动 Bifrost 服务器 |
| `bifrost stop` | 停止服务器 |
| `bifrost restart` | 重启服务器 |
| `bifrost status` | 查看服务器运行状态 |
| `bifrost list` | 列出所有配置的 Provider |
| `bifrost upgrade` | 从 GitHub Releases 自动升级到最新版本 |

## 配置说明

### Server 配置

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `port` | u16 | 5564 | 服务监听端口 |
| `timeout_secs` | u64 | 600 | HTTP 请求超时时间（秒） |
| `max_retries` | u32 | 5 | HTTP 请求失败最大重试次数 |
| `retry_backoff_base_ms` | u64 | 100 | 指数回避基础延迟（毫秒） |
| `proxy` | String | - | HTTP 代理地址（可选） |

### Provider 配置 `[provider.<name>]`

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `base_url` | String | - | 服务提供商的 API 地址 |
| `api_key` | String | - | API 密钥 |
| `endpoint` | String | openai | 端点类型：`openai` 或 `anthropic` |
| `adapter` | String/Array | passthrough | 适配器类型，可指定单个或多个（按顺序执行） |
| `headers` | Array | - | Provider 级别的额外请求头，会添加到所有请求 |
| `body` | Array | - | Provider 级别的额外请求体字段，会合并到请求体中 |
| `exclude_headers` | Array | - | 排除的请求头（仅影响原始请求 headers） |
| `extend` | bool | false | 是否继承原始请求的 headers |
| `models` | Array | - | 模型特定配置，详见下表 |

#### Provider.models 子配置 `[[provider.<name>.models]]`

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `name` | String | - | 模型名称 |
| `headers` | Array | - | 该模型的额外请求头（优先级高于 Provider 级别） |
| `body` | Array | - | 该模型的额外请求体字段（会与 Provider 级别合并） |

#### Header/Body 字段格式

```toml
{ name = "X-Header-Name", value = "header-value" }
{ name = "body_field", value = "field-value" }
```

### Endpoint 配置 `[endpoint.<name>]`

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `mapping` | Table | - | 简短模型名称到 `provider@model` 格式的映射 |

#### 简单字符串映射

```toml
[endpoint.openai.mapping]
"sonnet" = "qwen-code@gpt-4o"
```

#### 复杂映射（支持 headers 和 body）

复杂映射允许你为目标请求添加额外的 `headers` 或 `body` 字段。

```toml
[endpoint.openai.mapping]
# 简单字符串映射
"sonnet" = "qwen-code@gpt-4o"

# 复杂映射：target 必填，headers/body 可选
[endpoint.openai.mapping."qwen3.6-plus-flush"]
target = "qwen-code@coder-model"

[[endpoint.openai.mapping."qwen3.6-plus-flush".headers]]
name = "X-Custom-Header"
value = "custom-value"

[[endpoint.openai.mapping."qwen3.6-plus-flush".body]]
name = "enable_think"
value = false
```

| 复杂映射字段 | 类型 | 说明 |
| ------------ | ---- | ---- |
| `target` | String | 必填，目标 provider@model 字符串 |
| `headers` | Array | 可选，额外的请求头数组 |
| `body` | Array | 可选，额外的请求体字段数组 |

**优先级**：`provider@model` 格式 > endpoint mapping > 报错

### 适配器类型

| 适配器 | 说明 |
| ---- | ---- |
| `passthrough` | 透传，不做任何转换 |
| `openai-to-qwen` | 将 OpenAI 格式转换为本地 Qwen CLI 格式 |
| `anthropic-to-qwen` | 将 Anthropic 格式转换为本地 Qwen CLI 格式 |
| `anthropic-to-openai` | 将 Anthropic 格式转换为 OpenAI 格式 |

### 完整配置示例

```toml
[server]
port = 5564
timeout_secs = 600
max_retries = 5

[provider.qwen-code]
base_url = "https://portal.qwen.ai/v1"
api_key = "any-key"
endpoint = "openai"
adapter = "openai-to-qwen"

[endpoint.openai]
mapping = { "sonnet" = "qwen-code@gpt-4o" }
```

## 架构

```text
┌─────────────────────────────────────────────────────────────┐
│                      Your Application                       │
│                    (Claude Code, etc.)                      │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Bifrost Server                           │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │ /openai/... │    │/anthropic/..│    │  Adapter Chain  │  │
│  └──────┬──────┘    └──────┬──────┘    └────────┬────────┘  │
│         │                  │                    │           │
│         └──────────────────┴────────────────────┘           │
│                          │                                  │
└──────────────────────────┼──────────────────────────────────┘
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
     ┌────────────────┐         ┌────────────────┐
     │   Provider 1   │         │   Provider N   │
     └────────────────┘         └────────────────┘
```

## License

MIT
