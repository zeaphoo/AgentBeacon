# WorkBuddy 接入

WorkBuddy 桌面版复用 CodeBuddy 的 hook 运行时，只是把用户级设置重定向到 `~/.workbuddy/settings.json`。因此配置结构与事件名跟 [CodeBuddy](codebuddy.md) 完全一致。

## 安装

```bash
agent-beacon integrate install workbuddy
```

写入 `~/.workbuddy/settings.json`（可用 `--dir` 或 `AGENT_BEACON_WORKBUDDY_DIR` / `WORKBUDDY_HOME` 覆盖）。

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

装完请**彻底退出并重新打开 WorkBuddy**（或新建会话），否则进程里还是旧配置。

## 升级与卸载

```bash
agent-beacon integrate upgrade workbuddy
agent-beacon integrate uninstall workbuddy
```

只删除带 `# agent-beacon-hook:workbuddy:<version>` 标记的命令。

## 排查

```bash
agent-beacon hooks --limit 50
```
