# 开发文档

[产品 README](../README.zh-CN.md) · [English](development.md)

本文档说明 AgentBeacon 的架构、本地开发、测试、打包和贡献流程。

## 架构

```text
agent hooks / CLI
        │ HTTP
        ▼
  agent-beacon 服务
  ├─ 会话注册与状态聚合
  ├─ 语义灯效映射
  ├─ 设备发现 + 每设备 actor
  ├─ HTTP API + SSE
  └─ 内嵌 Web 控制台（127.0.0.1:6980）
        │
        ├─ USB 状态灯（MLight / YH50）
        └─ SSE ──▶ React Web 控制台 / Tauri 桌面应用
```

### 目录结构

```text
crates/
  agent-beacon-core/     # 领域模型、配置、会话、灯效
  agent-beacon-lights/   # 设备发现、协议、传输、actor、mock
  agent-beacon/          # axum API、SSE、内嵌资源、CLI、worker
web/                     # React + Vite + Tailwind + Zustand 控制台
desktop/src-tauri/       # Tauri 桌面壳（splash、托盘、服务托管）
docs/                    # 协议、接入文档、硬件验收清单
scripts/                 # 平台打包与版本辅助脚本
.github/workflows/       # CI 与 tag 驱动的 Release
```

### 数据流

1. Agent hook 或 CLI 向 `POST /api/v1/events` 上报事件。
2. 服务更新会话注册表并计算聚合状态。
3. 配置映射将聚合状态转换成语义灯效。
4. 每个设备 actor 将灯效展开为协议命令并写入设备。
5. Web 控制台通过 SSE 接收快照并渲染当前状态。

## 环境准备

- Rust 1.85+ 与 Cargo
- Node.js 22+ 与 pnpm 10+
- 平台依赖：
  - **Linux**：`libwebkit2gtk-4.1-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libgtk-3-dev`、`libxdo-dev`、`patchelf`、`libudev-dev`、`libusb-1.0-0-dev`、`file`
  - **macOS**：Xcode Command Line Tools；macOS 10.14+ 无需安装 CH340 驱动
  - **Windows**：WebView2；YH50 串口回退需要 WCH CH340 驱动

## 本地开发

```bash
pnpm --dir web install

# 终端 1：服务监听 127.0.0.1:6980
pnpm dev:serve

# 终端 2：Vite 开发服务器 127.0.0.1:6973
pnpm dev:web
```

Vite 会把 `/api` 代理到服务端。打开 `http://127.0.0.1:6973`。

桌面壳开发：

```bash
pnpm tauri:dev
```

## 构建

```bash
pnpm build:web                 # 构建 Web 资源
cargo build -p agent-beacon    # 将 web/dist 编译进服务
cargo run -p agent-beacon      # 启动服务
```

服务会在 `http://127.0.0.1:6980` 提供内嵌控制台。

桌面应用：

```bash
pnpm tauri:build
```

或使用平台打包脚本：

```bash
pnpm package:linux   # deb + AppImage
pnpm package:mac     # macOS dmg（仅 macOS）
pnpm package:win     # NSIS/MSI + 便携 zip（仅 Windows Git Bash）
```

## 测试

```bash
cargo test --workspace
pnpm --dir web test
```

常用手工检查：

- `cargo run -p agent-beacon` 能启动服务。
- `agent-beacon status` 和 `agent-beacon devices` 输出英文诊断信息。
- 没有硬件时，可在配置中设置 `mock.enabled = true` 启用 mock 设备。
- `agent-beacon integrate list` 查看受管集成状态。

## 配置与 API

配置文件由服务独占写入：

- Linux：`~/.config/agent-beacon/config.json`
- macOS：`~/Library/Application Support/agent-beacon/config.json`
- Windows：`%APPDATA%\agent-beacon\config.json`

默认监听 `127.0.0.1:6980`。可通过控制台或 `PUT /api/v1/config` 修改配置。

### HTTP API

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/v1/health` | 服务健康检查 |
| `GET` | `/api/v1/state` | 完整状态快照 |
| `GET` | `/api/v1/stream` | SSE 快照与更新 |
| `GET` | `/api/v1/agents` | Agent 列表 |
| `GET` | `/api/v1/devices` | 设备列表 |
| `POST` | `/api/v1/devices/scan` | 触发设备发现 |
| `POST` | `/api/v1/events` | 上报 agent 事件 |
| `GET/DELETE` | `/api/v1/hooks` | 读取或清空最近的 hook/状态迁移诊断日志 |
| `DELETE` | `/api/v1/sessions` | 清空会话 |
| `POST` | `/api/v1/control` | 应用手动灯效 |
| `POST` | `/api/v1/control/auto` | 恢复自动控制 |
| `GET/PUT` | `/api/v1/config` | 读取或更新配置 |
| `GET` | `/api/v1/integrations` | Agent 集成：检测到、已安装、待安装 |
| `POST` | `/api/v1/integrations/settings` | 只更新 `auto_install` / `onboarded` |
| `POST` | `/api/v1/integrations/install-all` | 安装所有检测到但尚未接入的 agent |
| `POST` | `/api/v1/integrations/{target}/{install\|upgrade\|uninstall}` | 操作单个集成 |

### CLI

```bash
agent-beacon serve
agent-beacon report --agent pi --state working --session default
agent-beacon status
agent-beacon devices
agent-beacon hooks --limit 100
agent-beacon integrate list
agent-beacon integrate install pi
agent-beacon integrate install claude-code
agent-beacon integrate upgrade
agent-beacon integrate uninstall pi
```

CLI help 与诊断输出使用英文。

### 集成安装方式

`agent-beacon integrate install` 由 `Target::kind()` 分发到四种写入方式：

| 类型 | 目标 | 写入内容 |
| --- | --- | --- |
| 文件插件 | `pi`、`opencode` | 带 `@agent-beacon-integration` / `@agent-beacon-version` 标记的 `.js` 插件 |
| Cordis patch | `dsh` | JS hook + `~/.dsh/cordis.patch.yml` 里的 `BEGIN/END` 块 |
| JSON hooks | `claude-code`、`qoder`、`gemini`、`qwen`、`iflow` | 追加到 `settings.json` 的 `hooks`，命令尾部带 `# agent-beacon-hook:<target>:<version>` 标记 |
| Codex notify | `codex` | `config.toml` 里被 `# >>> agent-beacon-hook:codex` 注释包裹的 `notify` 数组 |

控制台同样可以安装（`POST /api/v1/integrations/...`）。`config.integrations.auto_install` 让服务在启动时自动安装新检测到的 agent；`config.integrations.onboarded` 记录首次启动提示是否已处理。

JSON hooks 与 Codex 安装器采用合并写入：用户已有的 hook 与其它配置键保持不变，卸载时只删除带标记的条目。`integrate install` 不带目标时，会安装所有配置目录存在的目标。

所有 hook 都调用 `agent-beacon hook`：它读取 agent 载荷（JSON hooks 走 stdin，Codex notify 走最后一个参数），提取 `session_id`、事件名与工具名，在 `--state auto` 时推导状态，然后上报到 `/api/v1/events`。它始终返回 0，服务不在时不会阻塞 agent。

### 排查灯效变化

服务在内存中保留最近 1000 条 hook 事件与状态迁移记录。每条记录包含收到的事件、是否被接受，以及变化前后的聚合状态与灯效；TTL 过期、手动覆盖、配置修改和无法解析的上报也会记录。

```bash
agent-beacon hooks --limit 100
agent-beacon hooks --limit 100 --json
curl -s 'http://127.0.0.1:6980/api/v1/hooks?limit=100'
curl -X DELETE http://127.0.0.1:6980/api/v1/hooks   # 不重启服务即可清空
```

如果运行中灯先短暂变绿再变黄，查看两条 `working` 记录之间是否夹着一条 `idle`。记录里的 `agent/session`、来源 `message` 和毫秒时间戳可以定位是哪个 hook 发出的。记录同时以 `info` 级别写入服务日志，关键字为 `hook journal`。

### 状态模型

- Agent 状态：`idle`、`working`、`waiting`、`error`
- 会话键：`(agent, session)`；省略 `session` 时为 `{agent}:default`
- 默认 TTL：900 秒；事件可用 `ttl_secs`（30–86400）覆盖
- 聚合优先级：`error > waiting > working > idle`；无会话为 `none`
- 默认映射：`none → off`、`idle → calm`、`working → busy`、`waiting → attention`、`error → alert`

## 硬件协议

- [MLight 串口协议](MLight-serial-protocol.md)
- [YH50-RGY-J 串口协议](YH50-RGY-J-serial-protocol.md)
- [AgentCore-Light 协议（USB / 蓝牙 SPP / BLE）](AgentCore-Light-serial-protocol.md)
- [硬件验收清单](hardware-checklist.md)

设备发现优先使用 USB 序列号而不是端口名，因此重插后绑定仍可保留。设置页支持按设备覆盖型号。

## Agent 集成

集成资源位于 `crates/agent-beacon/assets/integrations/`。服务支持 Pi、OpenCode、DeepSeek Harness 插件的安装、升级与旧文件迁移。各 agent 的接入方式见[集成文档](integrations/)。

## CI 与发布

CI 在推送到 `main` 和 PR 时运行：

- Rust workspace 测试
- Web 测试与生产构建
- Ubuntu / macOS / Windows 桌面编译检查

Release 工作流由 `v*` tag 触发，构建：

- Linux：`deb` 与 `AppImage`
- macOS：arm64 / x86_64 `dmg`
- Windows：NSIS、en-US/zh-CN MSI、便携 zip

手动 `workflow_dispatch` 只构建并上传 artifacts，不创建 GitHub Release。

### 发布流程

1. 更新 `Cargo.toml`、`package.json`、`web/package.json`、`desktop/src-tauri/tauri.conf.json` 中的版本号。
2. 在 `CHANGELOG.md` 和 `CHANGELOG.zh-CN.md` 添加发布条目。
3. 提交发布改动。
4. 创建并推送 annotated tag：

   ```bash
   git tag -a v0.1.0-rc.1 -m "AgentBeacon v0.1.0-rc.1"
   git push origin v0.1.0-rc.1
   ```

Release 工作流会校验 tag 与清单版本一致，提取中英文 changelog 内容，然后发布 GitHub Release。macOS 签名/公证使用仓库 secrets（`MAC_CERT_P12_BASE64`、`MAC_CERT_PASSWORD`、`MAC_SIGN_IDENTITY`、`MAC_NOTARY_API_KEY_ID`、`MAC_NOTARY_API_ISSUER_ID`、`MAC_NOTARY_API_KEY_P8_BASE64`）。Windows MSI 即使在预发布版本也会使用数字 ProductVersion。

## 贡献

1. Fork 仓库并创建聚焦的分支。
2. 保持改动范围清晰，补充或更新测试。
3. 运行 `cargo test --workspace` 和 `pnpm --dir web test`。
4. 提交信息尽量使用清晰的 Conventional Commit 风格。
5. 提交 PR，说明行为变化和验证方式。

提交前请确认日志与 CLI 输出保持英文，并检查 UI 的中英文和浅色/深色主题。

## 许可证

Apache License 2.0，详见 [LICENSE](../LICENSE)。
