# Pi 接入示例

Pi 支持 TypeScript/JavaScript 扩展，可以订阅 `agent_start`、`agent_settled`、`tool_call`、`session_start`、`session_shutdown` 等事件。

把下面文件放到：

- 用户级：`~/.pi/agent/extensions/agent-beacon.js`
- 或项目级：`.pi/extensions/agent-beacon.js`

```javascript
import { execFile } from "node:child_process";

function report(state, message, ttl) {
  const args = ["report", "--agent", "pi", "--state", state];
  if (message) args.push("--message", message);
  if (ttl) args.push("--ttl", String(ttl));
  // hook 里不要阻塞 Pi；失败时静默，避免服务未启动影响 agent
  execFile("agent-beacon", args, { timeout: 900 }, () => {});
}

export default function (pi) {
  pi.on("session_start", () => report("idle"));
  pi.on("agent_start", () => report("working"));
  pi.on("tool_call", (event) => report("working", event.toolName));
  pi.on("agent_settled", () => report("idle"));
  // Pi 退出后让会话在 30 秒内自然过期，避免灯一直亮
  pi.on("session_shutdown", () => report("idle", undefined, 30));
}
```

说明：

- `agent_start` / `agent_settled` 对应一次 agent 运行开始与最终结束；`agent_end` 不是最终空闲信号（可能还有后续 run），不要用错。
- `tool_call` 在工具执行前触发，适合把状态保持为 `working`。
- `agent-beacon` 需要在 `PATH` 中，或把 `execFile` 的第一个参数改成绝对路径。
- 多个 Pi 会话默认共用一个 `pi:default` 会话；如果要区分，可在 `report` 里加 `--session <id>`（从 Pi 的 session 上下文取）。
- 状态含义：`idle` 绿灯常亮、`working` 绿 + 黄呼吸、`waiting` 黄闪、`error` 红闪。

验证：

```bash
pi --extension ~/.pi/agent/extensions/agent-beacon.js
agent-beacon status
```
