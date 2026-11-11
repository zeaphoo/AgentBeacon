# Pi 接入示例

Pi 支持 TypeScript/JavaScript 扩展，可以订阅 `agent_start`、`agent_settled`、`tool_call`、`session_start`、`session_shutdown` 等事件。

插件已内置在 `agent-beacon` 二进制中，直接安装：

```bash
agent-beacon integrate install pi
agent-beacon integrate list
```

安装位置是 `~/.pi/agent/extensions/agent-beacon-hook-pi.js`（项目级可手动放到 `.pi/extensions/`）。旧版本安装的 `agent-beacon.js` 会在安装/升级时自动迁移并删除。

服务升级后会随启动自动升级已安装的受管插件；也可以手动执行：

```bash
agent-beacon integrate upgrade pi
agent-beacon integrate uninstall pi
```

插件通过 `fetch` 直接 POST `/api/v1/events`，不依赖 `agent-beacon` 在 PATH 中；900ms 超时，失败静默，不阻塞 Pi。

说明：

- `agent_start` / `agent_settled` 对应一次 agent 运行开始与最终结束；`agent_end` 不是最终空闲信号（可能还有后续 run），不要用错。
- 插件还会在 `before_agent_start` 上报 `working`，覆盖 `agent_start` 之前压缩检查/准备阶段的空档。
- `tool_call` 在工具执行前触发，适合把状态保持为 `working`。
- 插件用异步 `fetch` 上报，不阻塞 Pi；但上报会经过 latest-wins 队列串行发送，避免 `idle`/`working` 因并发请求乱序而让灯短暂变绿。服务不在默认地址时设置 `AGENT_BEACON_URL`。
- 每次上报都带 Pi 的 session id（来自 `ctx.sessionManager.getSessionId()`）与 Pi 事件名（如 `tool_call`、`agent_settled`），多个 Pi 会话不会互相覆盖。没有 session id 时才回退到 `pi:default`；事件名只用于控制台的诊断日志。
- `session_start` 在扩展热重载时也会触发；插件会检查 `ctx.isIdle()`，运行中不会误报 `idle`。`session_shutdown` 的 `reload` 原因也会被忽略，避免热重载时先闪一下绿色。
- 状态含义：`idle` 绿灯常亮、`working` 绿 + 黄呼吸、`waiting` 黄闪、`error` 红闪。

验证：

```bash
pi --extension ~/.pi/agent/extensions/agent-beacon-hook-pi.js
agent-beacon status
agent-beacon hooks --limit 100
```

如果灯在 Pi 运行中短暂变绿，`agent-beacon hooks` 会显示两次 `working` 之间是否夹了一条 `idle`，并给出对应的 session、message 与毫秒时间戳。

如果服务不在默认 `127.0.0.1:6980`，启动 Pi 前设置 `AGENT_BEACON_URL`。
