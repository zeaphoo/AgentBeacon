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
- `tool_call` 在工具执行前触发，适合把状态保持为 `working`。
- 插件用异步 `fetch` 上报，不阻塞 Pi；服务不在默认地址时设置 `AGENT_BEACON_URL`。
- 多个 Pi 会话当前共用 `pi:default`；服务端支持 `session` 字段，后续可按 Pi 的 session 上下文扩展区分。
- 状态含义：`idle` 绿灯常亮、`working` 绿 + 黄呼吸、`waiting` 黄闪、`error` 红闪。

验证：

```bash
pi --extension ~/.pi/agent/extensions/agent-beacon-hook-pi.js
agent-beacon status
```

如果服务不在默认 `127.0.0.1:6980`，启动 Pi 前设置 `AGENT_BEACON_URL`。
