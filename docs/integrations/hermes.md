# Hermes Agent 接入

Hermes Agent 的扩展方式是**插件目录**：`~/.hermes/plugins/<name>/` 里放一个清单 `plugin.yaml` 和一个 python 模块，模块导出 `register(ctx)`，用 `ctx.register_hook(<event>, handler)` 订阅生命周期事件。

AgentBeacon 往 `~/.hermes/plugins/agent-beacon/` 写两个文件：

| 文件 | 内容 |
| --- | --- |
| `plugin.yaml` | 插件清单（`name`、`version`、`description`、`python`） |
| `agent_beacon.py` | 事件表 + 回调，回调里调用 `agent-beacon hook --agent hermes ...` |

## 安装

```bash
agent-beacon integrate install hermes
```

插件目录可用 `--dir` 或 `AGENT_BEACON_HERMES_DIR` / `HERMES_HOME` 覆盖；Windows 默认落在 `%LOCALAPPDATA%\.hermes\plugins\agent-beacon`。

安装后的模块形如：

```python
# @agent-beacon-integration: hermes
# @agent-beacon-version: 0.2.0-rc.1
# Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.
"""AgentBeacon integration for Hermes Agent: report lifecycle states."""

import subprocess

BINARY = '/usr/local/bin/agent-beacon'
MARKER = "agent-beacon-hook:hermes:0.2.0-rc.1"

EVENT_STATES = {
    "on_session_start": "idle",
    "pre_llm_call": "working",
    "pre_tool_call": "working",
    "post_tool_call": "working",
    "pre_approval_request": "waiting",
    "post_approval_response": "working",
    "api_request_error": "error",
    "on_session_end": "idle",
    "subagent_start": "working",
    "subagent_stop": "working",
}


def register(ctx):
    for event, state in EVENT_STATES.items():
        def handler(_state=state, _event=event, **_kwargs):
            _report(_state, _event)

        ctx.register_hook(event, handler)
```

## 事件映射

| Hermes 事件 | 状态 |
| --- | --- |
| `on_session_start` | `idle` |
| `pre_llm_call` / `pre_tool_call` / `post_tool_call` | `working` |
| `pre_approval_request` | `waiting` |
| `post_approval_response` | `working` |
| `api_request_error` | `error` |
| `on_session_end` | `idle` |
| `subagent_start` / `subagent_stop` | `working` |

## 生效方式

装完请重启 Hermes Agent 会话；插件目录是启动时扫描的。

## 升级与卸载

```bash
agent-beacon integrate upgrade hermes
agent-beacon integrate uninstall hermes
```

卸载只删带 `# @agent-beacon-integration:` 标记的文件；你在同一个插件目录里放的其它文件、以及其它插件都不会被碰（目录里只剩我们的文件时会顺手收掉空目录）。

## 排查

```bash
agent-beacon hooks --limit 50
```

如果灯完全不动，先确认 Hermes 已启用该插件（部分版本需要在配置里显式允许插件目录），再看 `agent_beacon.py` 里的 `BINARY` 是否指向真实可执行文件路径。
