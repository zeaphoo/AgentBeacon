# Claude Code 接入

Claude Code 支持通过 `settings.json` 的 `hooks` 在生命周期事件上执行命令。AgentBeacon 内置了安装器，会自动合并到已有的配置里，不会覆盖你手写的 hook。

## 安装

```bash
agent-beacon integrate install claude-code
agent-beacon integrate list           # 确认状态
```

安装器会写入 `~/.claude/settings.json`（可用 `--dir` 或 `AGENT_BEACON_CLAUDE_DIR` / `CLAUDE_CONFIG_DIR` 覆盖），每条形如：

```json
"PreToolUse": [
  {
    "matcher": "*",
    "hooks": [
      {
        "type": "command",
        "command": "'/usr/local/bin/agent-beacon' hook --agent claude-code --state working --event PreToolUse # agent-beacon-hook:claude-code:0.1.0-rc.1"
      }
    ]
  }
]
```

末尾的 `# agent-beacon-hook:...` 注释是归属标记，用来判断“这是不是我们装的”和“装的是哪个版本”。删除它等于放弃管理，卸载时不会再动这条命令。

`--force` 只在极少数情况下需要（例如标记被手工删掉）。安装器不会覆盖没有标记的同事件 hook。

注意：安装器会重写整个 `settings.json`，统一成 2 空格缩进。键的顺序、注释以外的一切内容和用户自己的 hook 都会保留，但手工排版的空白会被规整。介意的话先 `cp ~/.claude/settings.json{,.bak}`。

## 事件映射

| 事件 | 状态 | 说明 |
| --- | --- | --- |
| `SessionStart` | `idle` | 会话启动，等输入 |
| `UserPromptSubmit` | `working` | 用户提交了提示词 |
| `PreToolUse` / `PostToolUse` | `working` | 正在调用工具；消息取 `tool_name` |
| `Notification` | `waiting` | 需要确认 / 等待交互 |
| `Stop` | `idle` | 本轮回答结束 |
| `SubagentStop` | `idle` | 子 agent 结束 |
| `SessionEnd` | `idle`（TTL 30s） | 会话退出，超时后自动清除 |

`agent-beacon hook` 会从 stdin 读取 Claude Code 传入的 JSON 载荷，取出 `session_id`、`hook_event_name`、`tool_name`，因此多个终端会话不会互相覆盖。

## 手动配置（不使用安装器）

如果希望完全自己控制，把下面内容合并到 `~/.claude/settings.json`：

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "agent-beacon report --agent claude-code --state idle" }] }
    ],
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "agent-beacon report --agent claude-code --state working" }] }
    ],
    "Notification": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": "agent-beacon report --agent claude-code --state waiting --message needs-confirmation" }
        ]
      }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "agent-beacon report --agent claude-code --state idle" }] }
    ]
  }
}
```

区别只是手工版拿不到 `session_id`，所有会话会共用 `claude-code:default`。

## 卸载与升级

```bash
agent-beacon integrate upgrade claude-code   # 重写成当前版本的命令
agent-beacon integrate uninstall claude-code # 只删带标记的命令
```

服务启动时会自动升级已安装且版本落后的集成，无需手动操作。

## 排查

```bash
agent-beacon hooks --limit 50
```

若看到 `OUTCOME=rejected`，`DETAIL` 会给出原因（例如状态非法、TTL 越界）。服务不可达时 hook 会静默失败并退出 0，不会阻塞 Claude Code。

- `error` 状态没有对应 hook；需要时可在自己的 `PostToolUse` 脚本里判断失败分支后调用 `agent-beacon report --state error`。
- `agent-beacon` 必须在 hook 能拿到的路径上；安装器写的是可执行文件的绝对路径，所以移动二进制后需要重新 `integrate upgrade`。

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
