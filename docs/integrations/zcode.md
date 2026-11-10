# ZCode 接入

ZCode 的用户级 hook 配置在 `~/.zcode/cli/config.json`。它的 `hooks` 也是“事件名 → 数组”，但数组元素本身就是一条命令，没有 Claude 那种 `matcher` / `hooks` 嵌套：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "type": "process",
        "command": "'/usr/local/bin/agent-beacon' hook --agent zcode --state working --event PreToolUse --marker agent-beacon-hook:zcode:0.2.0-rc.1",
        "enabled": true,
        "timeoutMs": 10000
      }
    ]
  }
}
```

`type: "process"` 的条目按**参数数组**执行（不经过 shell），所以识别标记用 `--marker` 参数携带，而不是行尾 `#` 注释 —— 注释会被当成普通参数传给程序。

## 安装

```bash
agent-beacon integrate install zcode
```

写入 `~/.zcode/cli/config.json`（可用 `--dir` 或 `AGENT_BEACON_ZCODE_DIR` / `ZCODE_CONFIG_DIR` 覆盖）。

## 事件映射

| 事件 | 状态 |
| --- | --- |
| `SessionStart` | `idle` |
| `UserPromptSubmit` | `working` |
| `PreToolUse` / `PostToolUse` | `working` |
| `PermissionRequest` | `waiting` |
| `PostToolUseFailure` | `error`（TTL 30s） |
| `Stop` | `idle` |

## 生效方式

装完请**完全退出并重新打开 ZCode**，或新建会话，让新配置被加载。

## 升级与卸载

```bash
agent-beacon integrate upgrade zcode
agent-beacon integrate uninstall zcode
```

卸载只删带 `--marker agent-beacon-hook:zcode:` 的条目，`config.json` 里的其它键（主题、模型、你自己写的 hook）原样保留。

## 排查

```bash
agent-beacon hooks --limit 50
```

`OUTCOME=rejected` 说明上报被服务拒绝（通常是状态名不合法）；服务不可达时 hook 静默失败且退出 0，不会阻塞 ZCode。
