# Claude Code 接入示例

Claude Code 支持通过 hooks 在生命周期事件上执行命令。把下面内容合并到项目的 `.claude/settings.json`（或用户级 `~/.claude/settings.json`）。

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "agent-beacon report --agent claude-code --state idle"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "agent-beacon report --agent claude-code --state working"
          }
        ]
      }
    ],
    "Notification": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "agent-beacon report --agent claude-code --state waiting --message \"需要确认\""
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "agent-beacon report --agent claude-code --state idle"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "agent-beacon report --agent claude-code --state idle"
          }
        ]
      }
    ]
  }
}
```

说明：

- `agent-beacon` 需要在 `PATH` 中，或写绝对路径。
- 多个终端的 Claude Code 会话共用 `claude-code:default`；如需区分，加 `--session "$CLAUDE_SESSION_ID"`（如果环境提供）或使用终端标识。
- `error` 状态没有对应 hook 时，可用 `PostToolUse` 的失败分支或自定义脚本上报。
- 如果服务未启动，`report` 会在 1 秒内失败并返回非 0；hook 不应因此阻塞 agent。

## 通用 HTTP 接入

任何 agent 都可以直接上报：

```bash
curl -sS http://127.0.0.1:6980/api/v1/events \
  -H 'Content-Type: application/json' \
  -d '{
    "agent": "codex",
    "session": "terminal-1",
    "state": "working",
    "message": "正在修改 crates/agent-beacon",
    "ttl_secs": 900
  }'
```

合法状态：`idle`、`working`、`waiting`、`error`。

## 排查

```bash
agent-beacon status
agent-beacon devices
curl -s http://127.0.0.1:6980/api/v1/state | jq .
```

服务默认只监听 `127.0.0.1:6980`；桌面应用与浏览器使用同一个实例。
