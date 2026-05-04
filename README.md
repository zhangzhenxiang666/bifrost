# Bifrost

LLM 本地代理服务 - 统一管理多模型提供商，一键切换 API 端点。

## 特性

- **统一端点**: 只需配置一个 Provider，即可通过所有 OpenAI/Anthropic 接口访问
- **智能路由**: 通过 `provider@model` 格式自动路由到对应模型提供商
- **别名定义**: 通过`alias`配置模型映射到具体模型提供商的模型
- **协议转换**: 内置 OpenAI ↔ Anthropic ↔ Responses 协议自动转换

## 快速开始

### 安装

**Linux / macOS (bash/zsh/fish):**

```bash
curl -fsSL https://raw.githubusercontent.com/zhangzhenxiang666/bifrost/main/scripts/install.sh | bash
```

**Windows (PowerShell 5+ / 7):**

```powershell
powershell -c "& { Invoke-WebRequest -Uri https://raw.githubusercontent.com/zhangzhenxiang666/bifrost/main/scripts/install.ps1 -OutFile ""$env:TEMP\bifrost-install.ps1""; & ""$env:TEMP\bifrost-install.ps1"" }"
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

### 配置 Provider

编辑 `~/.bifrost/config.toml`

配置上游模型提供商openai兼容:

```toml
[provider.openai]
base_url = "https://openai.com/v1"
api_key = "your-key"
endpoint = "openai"
```

或配置 Anthropic兼容：

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
```

### 使用

配置完成后，以下所有接口均可使用，系统内部自动完成协议转换：

| 接口 | 说明 |
| ---- | ---- |
| `POST /openai/chat/completions` | OpenAI Chat Completions |
| `POST /openai/v1/chat/completions` | OpenAI Chat Completions |
| `POST /openai/responses` | OpenAI Responses API |
| `POST /openai/v1/responses` | OpenAI Responses API |
| `POST /anthropic/v1/messages` | Anthropic Messages |
| `POST /anthropic/messages` | Anthropic Messages |

将请求中的 `model` 字段改为 `provider@model` 格式即可路由：

```json
{
  "model": "openai@gpt-4o"
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
| `bifrost usage` | 查看 API 使用记录 |
| `bifrost log` | 查看和监听日志 |
| `bifrost upgrade` | 从 GitHub Releases 自动升级到最新版本 |

### Usage 命令参数

#### 子命令

| 子命令 | 说明 |
| ------ | ---- |
| `usage month` | 查看月度 token 用量，按 Provider 分组汇总 |

#### 子命令：`usage month`

```bash
bifrost usage month [month]
```

| 参数 | 说明 |
| ---- | ---- |
| `month` (可选) | 月份数字 (1-12，表示当前年份) 或 `YYYY-MM` 格式，默认当月 |

**使用示例：**

```bash
# 查看当月 token 用量
bifrost usage month

# 查看指定月份（4月，当前年份）
bifrost usage month 4

# 查看指定月份（2026年4月）
bifrost usage month 2026-04
```

按 Provider 分组汇总，显示每个 Provider 的请求数、Prompt Token、Completion Token、Total Token。

#### 查询参数

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--date` | - | 今天 | 指定日期 (YYYY-MM-DD)，默认当天 |
| `--from` | - | - | 起始日期 (YYYY-MM-DD)，与 `--to` 配合使用 |
| `--to` | - | - | 结束日期 (YYYY-MM-DD)，与 `--from` 配合使用 |
| `--time-range` | `-t` | - | 时间范围过滤，格式如 `12:00-16:00` |
| `--provider` | `-p` | - | 按 Provider 过滤，支持 `*` 通配符 |
| `--model` | `-m` | - | 按模型过滤，支持 `*` 通配符 |

**使用示例：**

```bash
# 查看当天记录
bifrost usage

# 查看指定日期记录
bifrost usage --date 2026-04-01

# 查看日期范围记录
bifrost usage --from 2026-04-01 --to 2026-04-15

# 组合过滤：查看某 Provider 在特定时间段的记录
bifrost usage --provider openai* --time-range 09:00-12:00
```

### Log 命令参数

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--date` | - | 今天 | 指定日期 (YYYY-MM-DD)，默认当天 |
| `--time-range` | `-t` | - | 时间范围过滤，格式如 `12:00-16:00` |
| `--level` | `-l` | - | 按日志级别过滤，支持 `*` 通配符 |
| `--lines` | - | 30 | 显示的日志条数 |
| `--tail` | - | false | 实时监听新日志 |

**使用示例：**

```bash
# 查看当天日志
bifrost log

# 查看指定日期的 INFO 级别日志
bifrost log --date 2026-04-01 --level info

# 实时监听日志
bifrost log --tail

# 按时间范围过滤
bifrost log --time-range 09:00-12:00
```

## 配置说明

### Server 配置

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `port` | u16 | 5564 | 服务监听端口 |
| `timeout_secs` | u64 | 600 | HTTP 请求超时时间（秒） |
| `max_retries` | u32 | 5 | HTTP 请求失败最大重试次数 |
| `retry_backoff_base_ms` | u64 | 700 | 指数回避基础延迟（毫秒） |
| `retry_status_codes` | Array\<u16\> | - | 额外触发重试的 HTTP 状态码（与默认值 [429, 500, 502, 503, 504] 合并） |
| `proxy` | String | - | HTTP 代理地址（可选） |

### Provider 配置 `[provider.<name>]`

| 字段 | 类型 | 默认值 | 必填 | 说明 |
| ---- | ---- | ------ | ---- | ---- |
| `base_url` | String | - | ✅ | 服务提供商的 API 地址 |
| `api_key` | String | - | ✅ | API 密钥 |
| `endpoint` | String | openai | | 端点类型：`openai`（默认）或 `anthropic` |
| `headers` | Array | - | | Provider 级别的额外请求头，会添加到所有请求 |
| `body` | Array | - | | Provider 级别的额外请求体字段，会合并到请求体中 |
| `exclude_headers` | Array | - | | 排除的请求头（仅影响原始请求 headers） |
| `extend` | bool | false | | 是否继承原始请求的 headers |
| `body_policy` | String / Table | - | | 请求体字段转换策略，详见下表 |
| `models` | Array | - | | 模型特定配置，详见下表 |

#### Body Policy 配置

| 格式 | 说明 |
| ---- | ---- |
| `"drop_unknown"` | 丢弃所有未处理的字段 |
| `{ allowlist = ["field1", "field2"] }` | 仅保留指定字段 |
| `{ blocklist = ["field1", "field2"] }` | 丢弃指定字段 |
| 省略 | 保留所有字段（默认） |

```toml
# 简单字符串：丢弃所有未处理字段
body_policy = "drop_unknown"

# 仅保留指定字段
body_policy = { allowlist = ["temperature", "top_p"] }

# 丢弃指定字段
body_policy = { blocklist = ["prediction", "modalities"] }
```

#### Provider.models 子配置 `[[provider.<name>.models]]`

| 字段 | 类型 | 默认值 | 必填 | 说明 |
| ---- | ---- | ------ | ---- | ---- |
| `name` | String | - | ✅ | 模型名称 |
| `headers` | Array | - | | 该模型的额外请求头（优先级高于 Provider 级别） |
| `body` | Array | - | | 该模型的额外请求体字段（会与 Provider 级别合并） |

#### Header/Body 字段格式

```toml
{ name = "X-Header-Name", value = "header-value" }
{ name = "body_field", value = "field-value" }
```

**可选字段 `condition`**：根据**客户端请求的端点类型**决定是否生效。即客户端访问 Bifrost server 时使用的接口类型，而非上游 provider 的端点类型。

**匹配规则**：

- 当客户端访问的端点类型与 `condition` 值匹配时，该字段会被应用
- 当客户端访问的端点类型与 `condition` 值**不匹配**时，该字段**不会**被应用
- 当 `condition` 为 `null` 或省略时，该字段**总是**被应用（适用于所有端点）

有效值：

- `"openai_chat"` 或 `"openai-chat"`：匹配 OpenAI Chat Completions 接口（`/openai/chat/completions` 或 `/openai/v1/chat/completions`）
- `"openai_responses"` 或 `"openai-responses"`：匹配 OpenAI Responses 接口（`/openai/responses` 或 `/openai/v1/responses`）
- `"anthropic"`：匹配 Anthropic Messages 接口（`/anthropic/messages` 或 `/anthropic/v1/messages`）

```toml
# 仅当客户端访问 /openai/chat/completions 时，此 header 才会被添加
{ name = "X-Chat-Only", value = "chat-value", condition = "openai_chat" }

# 仅当客户端访问 /openai/responses 时，此 body 字段才会被添加
{ name = "response_format", value = "json", condition = "openai-responses" }

# 仅当客户端访问 /anthropic/messages 时，此 header 才会被添加
{ name = "thinking_enabled", value = true, condition = "anthropic" }

# 适用于所有端点（无论客户端访问哪个接口）
{ name = "X-Common-Header", value = "common-value" }
```

**示例**：假设配置了 `provider.anthropic`（上游是 Anthropic），客户端请求如下：

- 访问 `/openai/chat/completions` → `condition = "openai_chat"` 的字段**会**应用，`condition = "anthropic"` 的字段**不会**应用
- 访问 `/anthropic/messages` → `condition = "anthropic"` 的字段**会**应用，`condition = "openai_chat"` 的字段**不会**应用
- 访问任何接口 → `condition = null` 的字段**总是**应用

### Alias 配置 `[alias.<name>]`

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| 简单字符串 | String | - | 目标 `provider@model` 格式 |
| 复杂映射 | Table | - | 简短模型名称到目标 provider@model 的映射，支持 headers 和 body |

#### 简单字符串映射

```toml
[alias]
"sonnet" = "openai@gpt-4o"
```

#### 复杂映射（支持 headers 和 body）

复杂映射允许你为目标请求添加额外的 `headers` 或 `body` 字段。

```toml
[alias]
# 简单字符串映射
"sonnet" = "openai@gpt-4o"

# 复杂映射：target 必填，headers/body 可选
[alias."claude-sonnet"]
target = "openai@claude-sonnet-4-20250514"

[[alias."claude-sonnet".headers]]
name = "X-Custom-Header"
value = "custom-value"

[[alias."claude-sonnet".body]]
name = "enable_thinking"
value = false
```

| 复杂映射字段 | 类型 | 说明 |
| ------------ | ---- | ---- |
| `target` | String | 必填，目标 provider@model 字符串 |
| `headers` | Array | 可选，额外的请求头数组 |
| `body` | Array | 可选，额外的请求体字段数组 |

**优先级**：`provider@model` 格式 > alias > 报错

---

## License

MIT
