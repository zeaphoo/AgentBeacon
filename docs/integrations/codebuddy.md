# CodeBuddy 接入

CodeBuddy 的 hook 配置与 Claude Code 同构：`~/.codebuddy/settings.json` 里的 `hooks` 对象，键是生命周期事件名，值是一组 `{ matcher, hooks: [{ type, command }] }`。

## 安装

```bash
agent-beacon integrate install codebuddy
```

写入 `~/.codebuddy/settings.json`（可用 `--dir` 或 `AGENT_BEACON_CODEBUDDY_DIR` / `CODEBUDDY_CONFIG_DIR` 覆盖）：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "'/usr/local/bin/agent-beacon' hook --agent codebuddy --state working --event PreToolUse # agent-beacon-hook:codebuddy:0.2.0-rc.1"
          }
        ]
      }
    ]
  }
}
```

## 事件映射

| 事件 | 状态 |
| --- | --- |
| `SessionStart` | `idle` |
| `UserPromptSubmit` | `working` |
| `PreToolUse` / `PostToolUse` | `working` |
| `Notification` | `waiting` |
| `Stop` / `SubagentStop` | `idle` |
| `SessionEnd` | `idle`（TTL 30s） |

## 生效方式

外部写入的 hook 不会热加载，装完请**重启 CodeBuddy 或新建一个会话**。

## 升级与卸载

```bash
agent-beacon integrate upgrade codebuddy
agent-beacon integrate uninstall codebuddy
```

卸载只删除带 `# agent-beacon-hook:codebuddy:<version>` 标记的命令，用户自己的 hook 原样保留。

## 排查

```bash
agent-beacon hooks --limit 50
```

服务不可达时 hook 静默失败且退出 0，不会阻塞 CodeBuddy。
