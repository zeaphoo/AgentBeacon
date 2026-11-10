# GitHub Copilot 接入

GitHub Copilot CLI（以及走同一套 hook 运行时的 Copilot 本地客户端）把 hook 配置放在 `~/.copilot/settings.json` 的 `hooks` 对象里。结构与 Claude Code 相同（`[{ matcher?, hooks: [{ type, command }] }]`），但**事件名是 camelCase**，跟 Claude Code 不一样，不能混用。

## 安装

```bash
agent-beacon integrate install github-copilot
```

别名：`copilot`、`copilot-cli`。写入 `~/.copilot/settings.json`（可用 `--dir` 或 `AGENT_BEACON_COPILOT_DIR` / `COPILOT_CONFIG_DIR` 覆盖）：

```json
{
  "hooks": {
    "preToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "'/usr/local/bin/agent-beacon' hook --agent github-copilot --state working --event preToolUse # agent-beacon-hook:github-copilot:0.2.0-rc.1"
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
| `sessionStart` | `idle` |
| `userPromptSubmitted` | `working` |
| `preToolUse` / `postToolUse` | `working` |
| `agentStop` | `idle` |
| `errorOccurred` | `error`（TTL 30s） |
| `sessionEnd` | `idle`（TTL 30s） |

## 生效方式

装完请重启 Copilot 会话。如果你用的是 ChatGPT 桌面版走同一套 hooks，还需要在设置页里逐条**信任**刚写入的 hook。

## 升级与卸载

```bash
agent-beacon integrate upgrade github-copilot
agent-beacon integrate uninstall github-copilot
```

只删除带 `# agent-beacon-hook:github-copilot:<version>` 标记的命令；同一事件下的其它命令（包括你自己写的）保持不动。

## 排查

```bash
agent-beacon hooks --limit 50
```

若灯一直不动，先确认 `~/.copilot/settings.json` 里的事件名是 camelCase —— 写成 `PreToolUse` 这类 Claude 式名字时 Copilot 会静默忽略。
