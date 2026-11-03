# OpenCode 接入示例

插件已内置在 `agent-beacon` 二进制中，直接安装：

```bash
agent-beacon integrate install opencode
agent-beacon integrate list
```

默认安装到 `~/.config/opencode/plugins/agent-beacon.js`（项目级可手动放到 `.opencode/plugins/`）。服务启动时会自动升级已安装的受管插件，也可手动执行：

```bash
agent-beacon integrate upgrade opencode
agent-beacon integrate uninstall opencode
```

插件通过 `fetch` 直接 POST `/api/v1/events`，不依赖 `agent-beacon` 在 PATH 中；900ms 超时，失败静默，不阻塞 OpenCode。

说明：

- `session.idle` → `idle`；`permission.asked` → `waiting`；`session.error` → `error`；`tool.execute.before` → `working`。
- 插件用异步 `fetch` 上报，不阻塞 OpenCode；900ms 超时，失败静默。
- 插件会从事件里读取 `sessionID`（存在时）并按会话区分；读不到时归入 `opencode:default`。
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

如果服务不在默认 `127.0.0.1:6980`，启动 OpenCode 前设置 `AGENT_BEACON_URL`。
