# Qoder CLI 接入

Qoder CLI 的 hook 配置与 Claude Code 一致：`~/.qoder/settings.json` 里的 `hooks` 对象，键是生命周期事件名。区别是事件集合略少，且没有 `SubagentStop`。

## 安装

```bash
agent-beacon integrate install qoder
```

写入 `~/.qoder/settings.json`（可用 `--dir` 或 `AGENT_BEACON_QODER_DIR` / `QODER_CONFIG_DIR` 覆盖）：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "'/usr/local/bin/agent-beacon' hook --agent qoder --state working --event PreToolUse # agent-beacon-hook:qoder:0.2.0-rc.1"
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
| `Stop` | `idle` |
| `SessionEnd` | `idle`（TTL 30s） |

## 和 herdr 共存

如果同一台机器上还装了 herdr 之类的工具，它也会往 `~/.qoder/hooks/` 放脚本并在 `settings.json` 里注册 hook。两边互不干扰：

- AgentBeacon 只认自己命令末尾的 `# agent-beacon-hook:qoder:<version>` 标记；
- 卸载时只删带标记的命令，同事件下的其它 hook 原样保留。

## 卸载与升级

```bash
agent-beacon integrate upgrade qoder
agent-beacon integrate uninstall qoder
```

## 排查

```bash
agent-beacon hooks --limit 50
```

若 `OUTCOME=rejected`，`DETAIL` 会说明原因。服务不可达时 hook 静默失败且退出 0，不会阻塞 Qoder。
