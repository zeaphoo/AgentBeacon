# Gemini CLI 家族接入（Gemini / Qwen Code / iFlow）

Gemini CLI 及其兼容分支使用同一套 `settings.json` hooks 机制，事件名与 Claude Code 不同。AgentBeacon 用一份安装器覆盖三者：

| Agent | 配置目录 | 目标名 |
| --- | --- | --- |
| Gemini CLI | `~/.gemini/settings.json` | `gemini` |
| Qwen Code | `~/.qwen/settings.json` | `qwen` |
| iFlow CLI | `~/.iflow/settings.json` | `iflow` |

## 安装

```bash
agent-beacon integrate install gemini
agent-beacon integrate install qwen
agent-beacon integrate install iflow
```

配置目录可由 `AGENT_BEACON_GEMINI_DIR` / `GEMINI_CONFIG_DIR` 等变量或 `--dir` 覆盖。安装器只追加带 `# agent-beacon-hook:<target>:<version>` 标记的命令，你已有的 hook 不会被改动（键顺序保留，但整个文件会按 2 空格缩进重写）。

## 事件映射

| 事件 | 状态 |
| --- | --- |
| `SessionStart` | `idle` |
| `BeforeAgent` | `working` |
| `BeforeTool` / `AfterTool` | `working`（消息取 `tool_name`） |
| `AfterAgent` | `idle` |
| `Notification` | `waiting` |
| `SessionEnd` | `idle`（TTL 30s） |

`agent-beacon hook` 从 stdin 读 JSON 载荷，提取 `session_id`、`hook_event_name`、`tool_name`。不同分支的字段名不完全一致（可能叫 `sessionId`、`event`、`type`），hook 命令会逐个尝试，认不出来时安静退出。

## 关于 iFlow

iFlow 0.3.23 内部实现的是 Claude 兼容层，`SessionStart` / `SessionEnd` / `UserPromptSubmit` / `PreToolUse` / `PostToolUse` / `Notification` 都存在于 bundle 中，但它同样接受 Gemini 风格的事件名。装完如果发现某些事件不触发，可以把 `~/.iflow/settings.json` 里的事件名换成 `UserPromptSubmit` / `Stop` / `PreToolUse` / `PostToolUse`，`--state` 值不用改。

## 卸载与升级

```bash
agent-beacon integrate upgrade gemini
agent-beacon integrate uninstall gemini
```

## 排查

```bash
agent-beacon integrate list
agent-beacon hooks --limit 50
```

`integrate list` 里 `STATE=unmanaged` 说明同名文件存在但缺少标记；`agent-beacon hook` 本身不受影响。
