# OpenCode 接入示例

OpenCode 支持插件，插件可以订阅事件并执行命令。当前文档的插件目录：

- 全局：`~/.config/opencode/plugins/`
- 项目级：`.opencode/plugins/`

> 不同 OpenCode 版本可能使用 `plugin/`（单数）目录；以 `opencode --version` 对应的官方文档为准，文件名本身不重要。

创建 `~/.config/opencode/plugins/agent-beacon.js`：

```javascript
import { execFile } from "node:child_process";

function report(state, message, ttl) {
  const args = ["report", "--agent", "opencode", "--state", state];
  if (message) args.push("--message", message);
  if (ttl) args.push("--ttl", String(ttl));
  execFile("agent-beacon", args, { timeout: 900 }, () => {});
}

export const AgentBeaconPlugin = async () => {
  return {
    event: async ({ event }) => {
      switch (event.type) {
        case "session.created":
          report("idle");
          break;
        case "permission.asked":
          report("waiting", "等待权限确认");
          break;
        case "session.error":
          report("error", "会话出错");
          break;
        case "session.idle":
          report("idle");
          break;
        case "session.deleted":
          // 会话结束后尽快让灯恢复，避免长时间残留
          report("idle", undefined, 30);
          break;
        default:
          break;
      }
    },
    "tool.execute.before": async (input) => {
      report("working", input?.tool);
    },
  };
};
```

说明：

- `session.idle` → `idle`；`permission.asked` → `waiting`；`session.error` → `error`；`tool.execute.before` → `working`。
- 插件里不要 `await` 上报命令，避免拖慢 OpenCode；`execFile` 900ms 超时且忽略错误。
- `agent-beacon` 需要能在 OpenCode 的 `PATH` 中找到。
- 多个 OpenCode 会话默认共用一个 `opencode:default`；如需区分，可在 `report` 里加 `--session <id>`（从事件属性中取）。
- 不启用插件时，也可以用 shell wrapper：

```bash
agent-beacon report --agent opencode --state working
opencode
agent-beacon report --agent opencode --state idle
```

验证：

```bash
opencode --version
# 打开 OpenCode 后：
agent-beacon status
```
