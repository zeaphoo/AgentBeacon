# Codex CLI 接入

Codex CLI 目前只暴露一个可配置的钩子：`config.toml` 里的 `notify`。它会在**回合结束**和**等待审批**时被调用，事件载荷作为最后一个参数传入，因此只能覆盖 `idle` 与 `waiting`；`working` 无法自动产生。

## 安装

```bash
agent-beacon integrate install codex
```

安装器会在 `~/.codex/config.toml` 顶部写入一段带标记的配置（可用 `--dir` 或 `AGENT_BEACON_CODEX_DIR` / `CODEX_HOME` 覆盖）：

```toml
# >>> agent-beacon-hook:codex 0.2.0-rc.1 (managed by agent-beacon)
notify = ["/usr/local/bin/agent-beacon", "hook", "--agent", "codex", "--state", "auto"]
# <<< agent-beacon-hook:codex

model = "gpt-5"

[mcp]
...
```

`notify` 是根级键，TOML 要求它出现在所有表头之前，所以这段固定在文件开头。

如果 `config.toml` 里已经有你自己写的根级 `notify`，安装器会拒绝执行并要求 `--force`；加 `--force` 后旧值会被逐行注释保留，方便你恢复：

```toml
# >>> agent-beacon-hook:codex 0.2.0-rc.1 (managed by agent-beacon)
notify = ["/usr/local/bin/agent-beacon", "hook", "--agent", "codex", "--state", "auto"]
# <<< agent-beacon-hook:codex

# notify = ["say", "done"]
```

## 事件映射

`--state auto` 表示由 hook 载荷里的 `type` 推导：

| Codex 载荷 `type` | 状态 |
| --- | --- |
| `agent-turn-complete` | `idle` |
| `approval-requested` | `waiting` |
| 其它（含错误类） | `error`（含 `error` / `fail`）或不上报 |

载荷里的 `turn-id` 会作为 session，`tool_name` 会作为 message。

## 局限

- Codex 不通知“开始工作”，所以灯在回合进行中会停在上一状态（通常是 `idle`）。需要实时 `working` 时，可在 shell 提示符或包装脚本里手动调用：

  ```bash
  codex() { agent-beacon report --agent codex --state working; command codex "$@"; agent-beacon report --agent codex --state idle; }
  ```

- Codex 0.6x 的二进制里找不到 `SessionStart` / `PreToolUse` 等字符串；写成 `~/.codex/hooks.json` 的那些 hook 不会被调用。

## 卸载与升级

```bash
agent-beacon integrate upgrade codex
agent-beacon integrate uninstall codex
```

卸载只会删掉标记块，`config.toml` 的其它内容保持不动。

## 排查

```bash
agent-beacon hooks --limit 20
```

服务不可达时 hook 退出码仍为 0，不会影响 Codex 的回合流程。
