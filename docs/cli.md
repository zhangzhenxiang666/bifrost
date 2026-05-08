# CLI 使用说明

`bifrost` CLI 用于管理本地 Bifrost 服务、查看日志、统计用量和升级版本。

## 服务管理

| 命令 | 说明 |
| ---- | ---- |
| `bifrost start` | 启动 Bifrost 服务 |
| `bifrost stop` | 停止 Bifrost 服务 |
| `bifrost restart` | 重启 Bifrost 服务 |
| `bifrost status` | 查看服务运行状态 |
| `bifrost list` | 列出当前配置的 Provider |
| `bifrost upgrade` | 从 GitHub Releases 自动升级到最新版本 |

常见流程：

```bash
bifrost start
bifrost status
bifrost list
```

修改配置后，建议重启服务：

```bash
bifrost restart
```

## 查看用量

`bifrost usage` 用于查看 API 使用记录。

```bash
bifrost usage
```

### 查询参数

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--date` | 无 | 今天 | 指定日期，格式为 `YYYY-MM-DD` |
| `--from` | 无 | 无 | 起始日期，格式为 `YYYY-MM-DD`，需要与 `--to` 配合 |
| `--to` | 无 | 无 | 结束日期，格式为 `YYYY-MM-DD`，需要与 `--from` 配合 |
| `--time-range` | `-t` | 无 | 时间范围过滤，例如 `12:00-16:00` |
| `--provider` | `-p` | 无 | 按 Provider 过滤，支持 `*` 通配符 |
| `--model` | `-m` | 无 | 按模型过滤，支持 `*` 通配符 |

### 示例

```bash
# 查看当天记录
bifrost usage

# 查看指定日期记录
bifrost usage --date 2026-04-01

# 查看日期范围记录
bifrost usage --from 2026-04-01 --to 2026-04-15

# 查看某个 Provider 的记录
bifrost usage --provider openai

# 组合过滤
bifrost usage --provider openai* --time-range 09:00-12:00
```

## 月度用量统计

`bifrost usage month` 用于查看月度 token 用量，并按 Provider 分组汇总。

```bash
bifrost usage month [month]
```

| 参数 | 说明 |
| ---- | ---- |
| `month` | 可选。可以是月份数字 `1-12`，表示当前年份；也可以是 `YYYY-MM` 格式。省略时默认当月 |

示例：

```bash
# 查看当月 token 用量
bifrost usage month

# 查看当前年份 4 月
bifrost usage month 4

# 查看 2026 年 4 月
bifrost usage month 2026-04
```

输出会按 Provider 汇总请求数、Prompt Token、Completion Token 和 Total Token。

## 查看日志

`bifrost log` 用于查看或实时监听服务日志。

```bash
bifrost log
```

### 参数

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--date` | 无 | 今天 | 指定日期，格式为 `YYYY-MM-DD` |
| `--time-range` | `-t` | 无 | 时间范围过滤，例如 `12:00-16:00` |
| `--level` | `-l` | 无 | 按日志级别过滤，支持 `*` 通配符 |
| `--lines` | 无 | `30` | 显示的日志条数 |
| `--tail` | 无 | `false` | 实时监听新日志 |

### 示例

```bash
# 查看当天最近日志
bifrost log

# 查看指定日期的 INFO 级别日志
bifrost log --date 2026-04-01 --level info

# 查看更多行
bifrost log --lines 100

# 按时间范围过滤
bifrost log --time-range 09:00-12:00

# 实时监听日志
bifrost log --tail
```

## 排查建议

如果请求没有按预期路由，可以按这个顺序检查：

1. `bifrost status` 确认服务正在运行。
2. `bifrost list` 确认 Provider 已加载。
3. 检查请求中的 `model` 是否为 `provider@model` 或已配置的 alias。
4. 用 `bifrost log --tail` 观察实时日志。
5. 修改配置后执行 `bifrost restart`。
