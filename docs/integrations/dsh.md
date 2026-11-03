# DeepSeek Harness（DSH）接入

DSH 使用 Cordis 插件系统。AgentBeacon 的插件已内置在 `agent-beacon` 二进制中：

```bash
agent-beacon integrate install dsh
agent-beacon integrate list
```

安装会做两件事：

1. 写入 `~/.local/share/agent-beacon/integrations/agent-beacon-hook-dsh.js`（macOS 在 `~/Library/Application Support/...`）
2. 在 `~/.dsh/cordis.patch.yml` 追加受管加载项：

```yaml
# >>> agent-beacon-hook-dsh (managed by agent-beacon)
- insert:
    - id: agent-beacon-hook-dsh
      name: 'file:///home/you/.local/share/agent-beacon/integrations/agent-beacon-hook-dsh.js'
# <<< agent-beacon-hook-dsh
```

DSH 默认开启 HMR 时会自动组合新 patch；否则重启 `dsh` 后生效。

## 状态映射

| DSH 事件 | AgentBeacon 状态 |
| --- | --- |
| `agent/status` → `running` | `working` |
| `agent/status` → `idle` | `idle` |
| `approval/asked` | `waiting`（等待审批） |
| `approval/decided` | `working` |
| `agent/error` | `error` |
| `agent/disposed` | `idle`，TTL 30 秒 |

会话 ID 取自 `agent.session.id`，会作为 `session` 上报；多个 DSH 会话可以独立显示。

## 升级与卸载

```bash
agent-beacon integrate upgrade dsh
agent-beacon integrate uninstall dsh
```

`agent-beacon serve` 启动时会自动升级已安装的受管插件；DSH 需要重启（或等待 HMR）才能加载新代码。

如果服务不在默认 `127.0.0.1:6980`，启动 DSH 前设置 `AGENT_BEACON_URL`。
