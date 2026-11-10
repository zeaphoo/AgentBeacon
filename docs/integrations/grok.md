# Grok Build 接入

Grok Build（xAI 的 `grok` CLI）支持全局 hooks：`~/.grok/hooks/` 下的每个 JSON 文件都会被加载，格式与 Claude Code 的 `settings.json` `hooks` 一致。AgentBeacon 使用独立的 `agent-beacon.json`，不会改动其它 hook 文件。

## 安装

```bash
agent-beacon integrate install grok
agent-beacon integrate list
```

默认写入 `~/.grok/hooks/agent-beacon.json`，可用 `--dir` 或 `AGENT_BEACON_GROK_DIR` / `GROK_HOME` 覆盖。

## 写入结构

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "'/usr/local/bin/agent-beacon' hook --agent grok --state idle --event SessionStart # agent-beacon-hook:grok:0.2.0-rc.1"
          }
        ]
      }
    ]
  }
}
```

尾部 `# agent-beacon-hook:...` 是归属与版本标记。Grok 信任全局 hooks，不需要在 UI 里逐条确认。

## 事件映射

| 事件 | 状态 |
| --- | --- |
| `SessionStart` | `idle` |
| `UserPromptSubmit` | `working` |
| `PreToolUse` / `PostToolUse` | `working` |
| `Stop` | `idle` |
| `SubagentStop` | `idle` |
| `StopCancelled` | `idle`（TTL 30s） |

`agent-beacon hook` 从 stdin 读取 Grok 的 JSON 载荷，提取会话 ID、事件名与工具名。载荷里没有可识别字段时会安静退出，不影响 Grok。

Grok 也会读取 Claude Code / Cursor 的 hook 配置做兼容，但 AgentBeacon 使用 Grok 原生 hooks 文件，避免重复上报；不要同时把同一份命令装到 `~/.claude/settings.json`。

## 生效方式

装完请新建一个 grok 会话；已经启动的会话不会重新加载 hooks。

## 升级与卸载

```bash
agent-beacon integrate upgrade grok
agent-beacon integrate uninstall grok
```

卸载只删除 `agent-beacon.json` 中带标记的命令，保留文件里的其它内容；不会动 `~/.grok/hooks/` 下的其它 hook 文件。

## 排查

```bash
agent-beacon integrate list
agent-beacon hooks --limit 50
```

如果 `Stop` 之后状态没有恢复 `idle`，先确认 `~/.grok/hooks/agent-beacon.json` 里 `Stop` / `StopCancelled` 条目存在，再检查 `agent-beacon hooks` 是否收到对应事件。
