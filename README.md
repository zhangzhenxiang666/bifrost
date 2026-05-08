# Bifrost

Bifrost 是一个本地 LLM 代理服务，用一个统一入口管理多个模型提供商，并在 OpenAI、Anthropic 和 OpenAI Responses 风格接口之间自动完成协议转换。

它适合这样的场景：

- 你想让不同客户端只连接一个本地服务。
- 你想通过 `provider@model` 明确路由到指定上游模型。
- 你想用 OpenAI 兼容客户端访问 Anthropic 兼容服务，或反过来使用。
- 你想为不同 Provider、模型或别名追加请求头和请求体字段。
- 你想在本地记录、查询和汇总 API 使用情况。

## 特性

- **统一端点**：配置一个 Provider 后，可以通过 OpenAI Chat、OpenAI Responses 或 Anthropic Messages 接口访问。
- **智能路由**：使用 `provider@model` 格式选择上游 Provider 和模型。
- **模型别名**：用短名称映射到完整的 `provider@model`，并可附加 headers/body。
- **协议转换**：内置 OpenAI、Anthropic、Responses 之间的请求和响应转换。
- **本地管理**：提供启动、停止、重启、日志、用量统计和升级等 CLI 命令。

## 快速开始

### 安装

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/zhangzhenxiang666/bifrost/main/scripts/install.sh | bash
```

Windows PowerShell:

```powershell
powershell -c "& { Invoke-WebRequest -Uri https://raw.githubusercontent.com/zhangzhenxiang666/bifrost/main/scripts/install.ps1 -OutFile ""$env:TEMP\bifrost-install.ps1""; & ""$env:TEMP\bifrost-install.ps1"" }"
```

安装脚本会下载 `bifrost` 和 `bifrost-server`，安装到 `~/.bifrost/bin/`，创建默认配置文件 `~/.bifrost/config.toml`，并尝试配置 PATH。

### 配置 Provider

编辑 `~/.bifrost/config.toml`，添加一个上游 Provider：

```toml
[provider.openai]
base_url = "https://api.openai.com/v1"
api_key = "your-key"
endpoint = "openai"
```

Anthropic 兼容 Provider 可以这样写：

```toml
[provider.anthropic]
base_url = "https://api.anthropic.com/v1"
api_key = "your-key"
endpoint = "anthropic"
```

更多配置项见 [配置说明](docs/configuration.md)。

### 启动服务

```bash
bifrost start
```

默认监听端口为 `5564`。启动后可以将客户端的 base URL 指向本地 Bifrost 服务。

### 发起请求

把请求里的 `model` 改成 `provider@model` 格式即可路由到指定 Provider：

```json
{
  "model": "openai@gpt-4o"
}
```

例如访问 OpenAI Chat Completions 兼容接口：

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

## 常用端点

| 接口 | 说明 |
| ---- | ---- |
| `POST /openai/chat/completions` | OpenAI Chat Completions |
| `POST /openai/v1/chat/completions` | OpenAI Chat Completions |
| `POST /openai/responses` | OpenAI Responses API |
| `POST /openai/v1/responses` | OpenAI Responses API |
| `POST /anthropic/messages` | Anthropic Messages |
| `POST /anthropic/v1/messages` | Anthropic Messages |

详细路由规则见 [路由与模型格式](docs/routing.md)。

## 常用命令

| 命令 | 说明 |
| ---- | ---- |
| `bifrost start` | 启动 Bifrost 服务 |
| `bifrost stop` | 停止服务 |
| `bifrost restart` | 重启服务 |
| `bifrost status` | 查看服务状态 |
| `bifrost list` | 列出已配置的 Provider |
| `bifrost usage` | 查看 API 使用记录 |
| `bifrost log` | 查看和监听日志 |
| `bifrost upgrade` | 从 GitHub Releases 升级 |

完整参数和示例见 [CLI 使用说明](docs/cli.md)。

## 文档

- [配置说明](docs/configuration.md)：Server、Provider、模型、headers/body、body policy 和 alias。
- [路由与模型格式](docs/routing.md)：`provider@model`、alias 优先级和端点列表。
- [协议转换](docs/protocol-conversion.md)：OpenAI、Anthropic、Responses 之间如何转换。
- [使用示例](docs/examples.md)：常见配置和请求示例。
- [CLI 使用说明](docs/cli.md)：服务管理、日志和用量统计。

`ARCHITECTURE.md` 保留在仓库根目录，主要用于 agent 或贡献者理解内部架构。

## License

MIT
