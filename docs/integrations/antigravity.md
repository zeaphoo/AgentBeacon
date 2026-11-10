# Google Antigravity 接入

Antigravity 提供官方生命周期 hooks：App、CLI 和 IDE 都读取全局 `~/.gemini/config/hooks.json`，工作区则可以用 `.agents/hooks.json`。AgentBeacon 只写全局文件，因此三个入口都能生效。

## 安装

```bash
agent-beacon integrate install antigravity
agent-beacon integrate list
```

默认写入 `~/.gemini/config/hooks.json`，可用 `--dir` 或 `AGENT_BEACON_ANTIGRAVITY_DIR` / `ANTIGRAVITY_HOME` 覆盖。

检测时看 `~/.gemini/antigravity`、`~/.gemini/antigravity-cli`、`~/.gemini/antigravity-ide` 是否存在。

## 写入结构

Antigravity 的 `hooks.json` 以 hook 名为顶层键。AgentBeacon 使用固定的 `agent-beacon` 键：

```json
{
  "agent-beacon": {
    "SessionStart": [
      {
        "type": "command",
        "command": "'/usr/local/bin/agent-beacon' hook --agent antigravity --state idle --event SessionStart # agent-beacon-hook:antigravity:0.2.0-rc.1"
      }
    ],
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "'/usr/local/bin/agent-beacon' hook --agent antigravity --state working --event PreToolUse # agent-beacon-hook:antigravity:0.2.0-rc.1"
          }
        ]
      }
    ]
  }
}
```

`PreToolUse` / `PostToolUse` 使用 `{ matcher, hooks: [...] }` 分组；`SessionStart` / `PreInvocation` / `PostInvocation` / `Stop` 直接把 handler 放进事件数组。安装器按这个差异分别写入，卸载时只删除 `agent-beacon` 键，`hooks.json` 里其它 hook 原样保留。

## 事件映射

| 事件 | 状态 |
| --- | --- |
| `SessionStart` | `idle` |
| `PreInvocation` | `working` |
| `PreToolUse` / `PostToolUse` | `working` |
| `PostInvocation` | `working` |
| `Stop` | `idle` |

Antigravity 的 hook 载荷用 `conversationId` 标识会话，工具名在 `toolCall.name`。`agent-beacon hook` 两者都会读取，因此多会话不会互相覆盖，工具事件也会显示工具名。

## 生效方式

装完请新建一个 Antigravity 会话；已经在运行的会话不会重新加载 hooks。

## 升级与卸载

```bash
agent-beacon integrate upgrade antigravity
agent-beacon integrate uninstall antigravity
```

服务启动时会自动升级版本落后的受管 hook。

## 排查

```bash
agent-beacon integrate list
agent-beacon hooks --limit 50
```

Antigravity 会同时加载全局和工作区 hooks。如果你在工作区 `.agents/hooks.json` 里也手写了 `agent-beacon` 键，建议只保留一份，避免重复上报。
