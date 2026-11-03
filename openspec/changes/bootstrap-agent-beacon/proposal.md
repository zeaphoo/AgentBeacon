# Proposal

## Why

开发者常同时运行多个编码 agent（Claude Code、Codex 等），它们的状态散落在不同终端窗口里，想知道「谁在跑、谁在等确认、谁失败了」只能挨个切换查看。USB 状态灯可以让人在桌边一眼看到当前最需要关注的状态。目前缺少一个跨平台、能被各类 agent 通过 hook 驱动、并且开箱即用的本地服务与桌面应用。

## What Changes

- 新建 Cargo workspace：`crates/agent-beacon-core`（核心库）、`crates/agent-beacon-lights`（串口灯控库）、`crates/agent-beacon`（HTTP 服务与 CLI，产出二进制 `agent-beacon`）。
- 新增常驻后台服务 `agent-beacon`：接收 agent 状态事件、聚合成单一「当前状态」，驱动 USB 状态灯；自带 Web 控制台；默认仅监听 `127.0.0.1`。
- 新增 HTTP API：事件上报、状态快照、SSE 实时推送、设备列表、配置读写、手动灯效控制；新增 CLI 子命令 `agent-beacon report|status|devices` 供 hook 调用。
- 新增串口灯控内核：设备发现（MLight VID `0x2E8A`、YH50 VID `0x1A86`/PID `0x7523`）、协议适配器（MLight 文本行协议、YH50-RGY-J 5 字节帧协议）、时序/去重/断线重连、以及无需硬件的 mock 设备。
- 新增 React Web 控制台（pnpm + Vite + React + Tailwind + shadcn/ui + Zustand，纯 JavaScript）：仪表盘、手动控制、灯效映射、设备与设置。
- 新增 Tauri v2 桌面应用，二进制名 **`AgentBeaconApp`**：随安装包内置 `agent-beacon`，用内置 splash 展示启动与失败状态，负责服务的启动/附着/健康检查/退出清理，就绪后在 WebView 中加载服务页面。
- 桌面应用提供开机自启动开关：托盘复选、偏好持久化、启动时与系统登录项对账，自启动时隐藏到托盘；应用无登录/引导流程，服务就绪后直达控制台。
- 新增三平台打包脚本：Linux（deb/AppImage）、macOS（.app/dmg）、Windows（NSIS/MSI 与便携 zip），流程参考已验证的 solidway 桌面壳。
- 新增接入文档与示例：Claude Code 等 agent 的 hook 片段、手工 curl 示例、打包与安装说明。

## Capabilities

### New Capabilities

- `agent-status`: agent 状态事件的接收模型、会话生命周期、多 agent 聚合与优先级、超时降级，以及 CLI 上报入口。
- `light-control`: USB 状态灯的设备发现、协议适配（MLight / YH50-RGY-J）、状态到灯效的映射与手动覆盖、串口时序与可靠性保障。
- `beacon-service`: 后台服务的进程生命周期、HTTP API 契约、SSE 推送、配置持久化、静态页面托管、单实例与受监管运行。
- `web-console`: Web 控制台的页面结构、实时数据展示、手动控制与配置编辑等用户可见行为。
- `desktop-app`: `AgentBeaconApp` 桌面壳的启动流程、内置服务管理与进程监管、窗口与托盘行为、跨平台打包。

### Modified Capabilities

<!-- 无：这是全新项目，没有既有 capability。 -->

## Impact

- **代码**：全新仓库结构——Rust workspace（`crates/`）、前端（`web/`）、Tauri 应用（`desktop/src-tauri`）。
- **API**：新增本地 HTTP JSON API 与 SSE 流；桌面端通过同一 API 控制服务。
- **依赖**：Rust 侧 `tokio`、`axum`、`serialport`、`rusb`、`serde`、`tracing`、`rust-embed`；前端 `react`、`vite`、`tailwindcss`、`zustand`、`shadcn/ui` 组件；桌面端 `tauri` v2（单实例、托盘、autostart）及按平台签名的内置服务。
- **系统**：需要串口访问权限（Linux 用户在 `dialout` 组并配置 udev；Windows 可能需安装 WCH CH340 驱动；macOS 10.14+ 系统自带驱动、无需安装且不应装旧 WCH kext）；安装包需按目标平台构建、签名并内置服务二进制。
- **兼容性**：不涉及既有系统迁移；v0 只支持单机、单用户、本地回环访问。
