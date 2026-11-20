# 更新日志

本项目的重要变更记录在此文件中。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

[English](CHANGELOG.md) | 中文

## [0.2.0-rc.5] - 2026-11-19

### 变更

- 自动分配（按 Agent / 按 Session）不再把新会话交给当前掉线的灯；15 秒宽限期只保留已有归属，反复瞬断的灯不会继续接活，它的会话会迁到在线的灯。
- 控制台改为轮询 `/api/v1/state`，不再持有 SSE 常驻连接：前台每秒一次、标签页在后台 5 秒一次，未变化时用 `If-None-Match` 拿 `304`，失败指数退避。这样不再占用浏览器同域连接槽位，多标签页/多窗口不会挤占其它请求。

### 新增

- `Snapshot.revision` 与 `GET /api/v1/state` 的 ETag 支持（内容未变返回 `304 Not Modified`）。
- `/api/v1/stream` 保留，供脚本与调试使用，控制台不再使用。

## [0.2.0-rc.4] - 2026-11-19

### 新增

- 硬件购买页（`docs/hardware-purchase.md`）：维护者自购并验收过的型号，README 设备表与控制台补充设备配图。
- 中英文 README 增加「How it works」示意图（SMIL 动画，`scripts/how-it-works-svg.py` 可重新生成）。
- AgentCore-Halo 自定义灯效表：连接时逐条下发 6 条 `LC:`，`BUSY` 橙色呼吸（`#FF6A00`）、`WAIT_CONFIRM` 橙色常亮，替代固件偏黄的 `#FFD400`；`AGENT_BEACON_HALO_LC` 可覆盖灯表做真机试验。
- 新增 `battery::BatteryModel`（crates/agent-beacon-lights）：静置基线 + 电压证据判定充电，电压中值滤波，并按容量/充电电流叠加电量模型（上限 99%）。

### 变更

- 充电状态不再信任 `CHG`：实测某些 AgentCore-Light 固件拔电后恒为 1、某些 Halo 固件恒为 0，因此只作为诊断信息放在悬停提示；充电改由“电压被拉低 / 小幅抬压持续 20 秒”判定，读数回到静置值 30 秒后撤销。
- 电量百分比按厂商上位机规则：最近 5 个电压样本取中值，窗口不稳定时保持上一次的值，模型推算值显示 `≈`，100% 只来自实测电压。
- 控制台当前灯效改用橙色 tone，灯效文案为橙色呼吸 / 橙色常亮。

### 移除

- 自造的 `ChargeTracker` / `PercentModel` 充电判定。
- `docs/hardware-checklist.md` 及其引用（实测结论保留在协议文档中）。

### 修复

- 设备卡片的手动灯效徽章不再显示不会跳动的倒计时，剩余时间仍在手动控制页。
- 顶部导航改为不透明背景，滚动时不再透出页面内容。

## [0.2.0-rc.3] - 2026-11-18

### 修复

- 设备 actor 改为每台设备独立 OS 线程 + current-thread runtime：串口/BLE 的打开、写超时、断链清理等同步阻塞不再占用主 tokio runtime，单台设备掉线不会拖慢 API/SSE。
- actor 停止改为协作式，`abort` 不再阻塞等待驱动 I/O；线程 panic 也能正确标记为已结束。
- 直连 SPP（RFCOMM）增加 1 秒发送超时，避免对端不读时设备线程永久卡住。

## [0.2.0-rc.2] - 2026-11-18

### 新增

- 新增 AgentCore-Halo 型号（`agentcore-halo`），与 AgentCore-Light 共用文本行协议，按产品名自动区分；控制台展示设备自报名、固件版本与电量。
- 控制台设备卡片、设备列表与「支持的型号」弹窗展示型号渲染图，非方形素材按原比例适配，未收录型号回落为通用图标。
- 设置页新增「显示设备图片」开关（默认开启），关闭后设备卡片、列表与支持型号弹窗改用通用图标。
- 活动会话卡片只展示最近 7 条，新增内存会话历史与「查看全部会话」详情弹窗。
- 设置页服务卡片新增事件日志弹窗，可按 agent 与关键词过滤；事件日志展示 agent 事件名与原始上报载荷。
- 新增 Google Antigravity 与 Grok Build 集成。Antigravity 在 `~/.gemini/config/hooks.json` 写受管的 `agent-beacon` 块（工具事件用 matcher 分组，其余事件直接放 handler）；Grok Build 使用独立的 `~/.grok/hooks/agent-beacon.json`，事件名与 Claude 式一致。
- 补齐 CodeBuddy、WorkBuddy、GitHub Copilot、Hermes、ZCode 等集成的品牌 Logo。

### 变更

- 设备发现改为独立扫描 worker；运行时离线设备保留 3 分钟后自动移除，蓝牙设备连上后才显示。
- 电量与充电状态变化时主动广播给控制台，避免界面读数滞后。
- 数据目录统一到应用数据根，Bundle ID 改为 `net.quadhub.agent-beacon`。
- 手动灯效的时长改为在控制台本地记住上次使用值，不再依赖全局配置（见「移除」）。

### 修复

- 充电状态改为看电压是否持续上升，不再信 `CHG` 引脚；电量优先按电压换算，`CHG` 只作为充电输入标志展示，不再直接显示成「USB 已接入」或「充电中」。
- 事件日志与最近会话弹窗在内容过少时保持最小高度，避免弹窗抖动。
- 「手动灯光控制」合并为单卡片，去掉成功横幅与独立「恢复自动」按钮；状态区改为与灯效同色系的面板：无覆盖时显示「无手动控制」与自动说明，有覆盖时显示灯效、倒计时和「结束手动控制」按钮。
- 首页手动控制提示改为按作用范围显示「全部/部分设备被手动控制」，颜色跟随被覆盖的灯效。

### 移除

- 移除「手动覆盖默认时长」配置项（`manual_override_default_secs`）：控制台改为在本地记住上次使用的时长，后端在未传 `duration_secs` 时固定使用 60 秒；旧配置中的遗留字段会被忽略。
- 移除 mock 设备概念：`mock` profile、`mock.enabled` 配置、mock 命令日志、控制台引用和无需硬件的 mock actor 全部删除。设备只来自真实发现。

## [0.2.0-rc.1] - 2026-11-10

### 新增

- 控制台新增「Agent 集成」页面：列出所有检测到的 agent 及其安装状态，支持一键安装/升级/卸载，以及「自动安装」开关。
- 首次启动提示：检测到已安装但未接入的 agent 时弹出，接入或跳过后写入 `integrations.onboarded`，不会重复打扰。
- 新增 `GET /api/v1/integrations`、`POST /api/v1/integrations/settings`、`POST /api/v1/integrations/install-all`，以及 `POST /api/v1/integrations/{target}/{install|upgrade|uninstall}`。
- 新增配置项 `integrations.auto_install`，服务启动时自动安装检测到的 agent。
- 新增 Claude Code、Codex CLI、Qoder CLI 以及 Gemini CLI 家族（Gemini / Qwen Code / iFlow）集成，可用 `agent-beacon integrate install <target>` 安装。
- 新增 CodeBuddy、WorkBuddy、GitHub Copilot、ZCode、Hermes Agent 集成。前三个写 Claude 式 `settings.json` hooks（GitHub Copilot 用 camelCase 事件名），ZCode 写 `~/.zcode/cli/config.json` 里的 `{ "type": "process" }` 条目，Hermes Agent 则是 `~/.hermes/plugins/agent-beacon/` 下的受管插件目录（`plugin.yaml` + python 模块）。
- 新增 `agent-beacon hook` 命令：从 stdin 或参数读取 agent 的 hook 载荷，按事件名推导状态并上报。它始终返回 0，服务未运行也不会阻塞 agent。
- 集成安装器改为合并写入：`settings.json` 的 hooks 与 `config.toml` 的 `notify` 会带标记保留，升级时原地更新，卸载时只删除自己的条目，不影响用户已有配置。
- 新增 hook 诊断日志：`agent-beacon hooks`、`GET/DELETE /api/v1/hooks`，并在服务日志中输出 `hook journal`。
- 新增 AgentCore-Light 设备协议文档（USB CDC / 蓝牙 SPP / BLE NUS），以及无需依赖的调试脚本 `scripts/agentcore_probe.py`。
- 新增 PromLight 硬件控制协议文档（BLE HID，`5E 5E` 二进制帧 / READ-WRITE-LED / 应答与错误码 / 持久参数 / 电量），供后续接入该设备时参照。

- 新增 AgentCore-Light 状态灯支持（`agentcore-light` profile）：文本行协议 + `HELLO` 握手、`IDLE`/`BUSY`/`SUCCESS`/`ERROR`/`WAIT_CONFIRM` 状态、`BRIGHTNESS_LEVEL`（百分比 ×3）与蜂鸣器映射，并有后台读线程消费设备的 `STATUS`/`CHG` 上报。
- AgentCore-Light 设备发现改为**先读 BlueZ 已知设备、再退回广播扫描**（每 10 s 一次，不触发 BLE 发现）：设备连上后就不再广播，只靠扫描永远发现不了「已经连着」的灯；顺带避免发现过程打断已有 LE 连接。
- 新增 AgentCore-Light 的 **SPP（RFCOMM）免 root 直连**：`spp:AA:BB:CC:DD:EE:FF[:通道]`，自己开 `AF_BLUETOOTH/BTPROTO_RFCOMM` socket，不依赖 `/dev/rfcomm*` 节点、不需要 root，作为 BLE 不可用时的兜底链路。
- `devices[{ port: "ble:…" / "spp:…" }]` 现在会被直接用作链路绑定：这两类端口系统不会枚举出来，之前写进配置也不会生效。BLE 优先于 SPP。
- 新增诊断工具 `--example bt_links`（已知设备，不扫描）与 `--example spp_probe`（SPP 直连握手）。
- 新增 AgentCore-Light 的 BLE（Nordic UART Service）链路：按广播名发现，连 NUS 后跑同一套文本协议；写入按 MTU 分片、无响应写不可用时降级为带响应写、通知去重，并有后台通知读取线程。
- `crates/agent-beacon-lights/examples` 新增 `ble_scan` / `ble_status` 诊断工具。
- AgentCore-Light 自动识别：USB CDC 按 `4C4A:4155`（Espressif `303A` 需产品名关键字）认定，蓝牙 SPP 端口先做一次 `HELLO` 握手确认后才挂 actor。

### 修复

- Pi 集成在扩展热重载时不再于运行中误报 `idle`；上报会携带 Pi session id，并按顺序发送（latest-wins）。

## [0.1.0-rc.1] - 2026-11-04

### 新增

- 新增 `agent-beacon` 服务：会话注册、状态聚合、语义灯效、HTTP API 与 SSE 推送。
- 支持 MLight 单灯珠/三灯珠和 YH50-RGY-J USB 状态灯，包括串口/libusb 传输、设备发现和每设备 actor。
- 提供 mock 设备，可在无硬件时验证「事件上报 → 聚合 → 设备命令」完整链路。
- 内置 Pi、OpenCode、DeepSeek Harness 集成，支持受管安装、旧文件迁移和自动升级。
- 新增 React Web 控制台，用于状态查看、手动灯光控制和服务配置。
- 新增 Tauri 桌面应用：启动页、托盘菜单、开机自启动和服务托管。
- 新增设备能力元数据（亮度、蜂鸣器）和设备特性徽章。
- 支持中文/English UI，首次启动按系统语言初始化，并可在导航栏切换。
- 支持浅色/深色主题、跟随系统主题和主题偏好持久化，状态色适配两种主题。
- 支持 Linux（deb/AppImage）、macOS（arm64/x86_64 dmg）和 Windows（NSIS、en-US/zh-CN MSI、便携 zip）发布打包。
- 新增 GitHub Actions CI 和 tag 驱动的 Release 工作流，自动生成 GitHub Release。
- 配置签名 secrets 后，macOS 发布产物会进行签名和公证。

### 变更

- MLight 亮度修改立即生效，并重放当前灯效命令而不插入 `OFF` 闪烁。
- MLight 默认亮度调整为 50%。
- 设置页改为 500ms 防抖自动保存，移除显式保存/重新载入按钮。
- 移除设置页的 mock 设备开关；仍可通过 `mock.enabled` 配置启用。
- 仪表盘聚合状态改为状态色 Hero 卡片：状态图标、低饱和背景和大号状态文字。
- 活动会话状态颜色统一按配置灯效映射显示；运行时长按秒/分钟/小时/天人性化展示。
- 服务信息移到设置页；移除 PID 和导航栏版本号。
- 导航名称改为「状态」，副标题改为「Agent 状态灯」。
- 服务日志、CLI help 和 CLI 输出统一为英文。
- 项目许可证从 MIT 切换为 Apache License 2.0。
- 文档拆分为产品 README 和开发文档，并提供中英文版本。

### 修复

- 修复 SSE 长连接导致 Ctrl+C/服务关停卡住的问题。
- 修复 Windows CI 的 LF 检出、MSYS 路径和桌面可执行文件检查顺序问题。
- Windows 打包现在同时生成 `en-US` 和 `zh-CN` MSI。
- 启用告警蜂鸣前增加响度与不适警告确认对话框。
- 统一聚合状态与活动会话的状态颜色，并实现服务运行时长实时显示。

[0.2.0-rc.3]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.2.0-rc.3
[0.2.0-rc.2]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.2.0-rc.2
[0.2.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.2.0-rc.1
[0.1.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.1.0-rc.1
