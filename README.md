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

### 配置 Provider

编辑 `~/.bifrost/config.toml`:

```toml
[server]
port = 5564
timeout_secs = 600
max_retries = 5

[provider.openai-example]
base_url = "https://api.openai.com/v1"
api_key = "sk-your-key"
endpoint = "openai"
adapter = "passthrough"

[provider.qwen]
base_url = "https://portal.qwen.ai/v1"
api_key = "your-qwen-key"
endpoint = "openai"
adapter = "openai-to-qwen"

[provider.anthropic]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
api_key = "your-key"
endpoint = "anthropic"
adapter = "anthropic-to-qwen"
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

## 配置说明

### Server 配置

```toml
[server]
port = 5564              # 服务端口
timeout_secs = 600       # 请求超时时间（秒）
max_retries = 5          # 最大重试次数
proxy = "http://proxy:8080"  # HTTP 代理（可选）
```

### Provider 配置

```toml
[provider.<name>]
base_url = "https://api.example.com/v1"  # 提供商 API 地址
api_key = "your-api-key"                 # API 密钥
endpoint = "openai" | "anthropic"        # 端点类型
# adapter = "openai-to-qwen" | "anthropic-to-qwen" | "anthropic-to-openai"  # 可选，默认透传
```

### 适配器类型

| 适配器 | 说明 |
| ---- | ---- |
| `openai-to-qwen` | 将 OpenAI 格式转换为 Qwen 格式 |
| `anthropic-to-qwen` | 将 Anthropic 格式转换为 Qwen 格式 |
| `anthropic-to-openai` | 将 Anthropic 格式转换为 OpenAI 格式 |

## 架构

```text
┌─────────────────────────────────────────────────────────────┐
│                      Your Application                        │
│                    (Claude Code, etc.)                      │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Bifrost Server                           │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │ /openai/... │    │/anthropic/..│    │  Adapter Chain  │  │
│  └──────┬──────┘    └──────┬──────┘    └────────┬────────┘  │
│         │                  │                    │            │
│         └──────────────────┴────────────────────┘          │
│                          │                                   │
└──────────────────────────┼───────────────────────────────────┘
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
     ┌────────────────┐         ┌────────────────┐
     │  OpenAI API    │         │  Qwen CLI      │
     └────────────────┘         └────────────────┘
```

## License

MIT
