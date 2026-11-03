# Design

## Context

这是一个全新仓库，目前只有两份设备协议文档（`docs/MLight-serial-protocol.md`、`docs/YH50-RGY-J-serial-protocol.md`）和厂商指令图，没有任何实现代码，见 `proposal.md` 的动机。

约束条件：

- 服务端必须是 Rust 编写的常驻进程，二进制名 `agent-beacon`；桌面应用二进制名 `AgentBeaconApp`，通过 Tauri 打包。
- Web 前端固定使用 pnpm + Vite + React + Tailwind + shadcn/ui + Zustand，**纯 JavaScript**，不使用 TypeScript。
- 桌面 App 随安装包以固定文件名内置 `agent-beacon`（`bundle.resources`），负责其启动、附着、健康检查与退出清理，双击即可用。
- 目标平台：Linux、macOS、Windows；设备为 USB 串口转接（MLight 是 CDC，YH50 是 CH340）。
- 两类设备的协议差异很大：MLight 是 115200 的 ASCII 文本行、有多通道组合语义与严格时序；YH50 是 9600 的 5 字节二进制帧、同一时刻只能呈现一种颜色，Linux/macOS 可直接走 USB Bulk，Windows 仅串口。
- 服务必须能脱离桌面 App 独立运行（headless、SSH 场景），Web 控制台也要能被普通浏览器直接访问。
- 可复用的参考实现：`~/workspace/solidway/solidway/app`（Tauri 桌面壳的进程托管、splash/错误页、托盘与三平台打包流程）与 `~/workspace/deepsight/app/alertapp`（已实测的 YH50 USB Bulk / 串口控制）。

## Goals / Non-Goals

**Goals:**

- 一个 `agent-beacon` 二进制同时承担服务与 CLI（`serve` / `report` / `status` / `devices`），无 GUI 依赖。
- 状态模型、灯效语义与设备协议三层解耦：新增设备型号只新增协议适配器，不触碰状态与 UI。
- 无硬件可开发/可测试：mock 设备 + 可注入时钟，串口时序能被自动化验证。
- 同一套前端产物同时服务于浏览器与 Tauri WebView，避免两套 UI。
- 进程生命周期明确：App 只负责自己拉起的服务，异常退出不留孤儿进程。

**Non-Goals:**

- 不做多用户、远程访问、云端同步或团队共享。
- 不做原生 GUI 重写：Tauri 只做进程托管、窗口与托盘，业务界面全部在 Web 控制台。
- 不覆盖 MLight 协议未暴露的颜色（蓝/青/紫/白），也不伪造亮度分通道能力。
- v0 不做系统服务注册与自动更新；开机自启动由桌面壳提供（见 D15）；代码签名/公证只预留打包配置（未配置签名身份时产出未签名产物并告警）。
- 不做账户与登录体系：应用就绪后直接进入控制台，不复制 solidway 的 bootstrap 引导、admin 登录或桌面自动登录。
- 不做 agent 侧自动注入：由用户按文档配置 hook。

## Decisions

### D1. 服务独立成进程，Tauri 以随包 helper 托管

`agent-beacon` 是独立二进制，随安装包内置（`bundle.resources`，固定名 `agent-beacon` / `agent-beacon.exe`）。Tauri App 启动时先探测配置端口上的健康实例；没有则拉起 helper（`serve --supervised --port <port>`），从 stdout 的 `agent-beacon: listening addr=...` 事件获知实际地址，就绪后窗口加载 `http://<addr>/`。

- 理由：headless 可独立运行；Web 控制台可被浏览器直接访问；服务崩溃不会带崩 UI；App 只是客户端之一。
- 备选：把 axum 嵌入 Tauri 进程——无法 headless，崩溃连坐，Web 访问缺失；注册系统服务——安装与权限复杂，v0 不做。
- 代价：多一个进程与端口，需要健康检查、就绪握手、孤儿清理，这些在 spec 中已定义为行为。

### D2. Web 资源编译期内嵌进服务

Vite 产物输出到 `web/dist`，由 `agent-beacon` 用 `rust-embed` 在编译期嵌入，`/` 与 SPA 回退路径直接返回 `index.html`。

- 理由：一套产物两处复用；不依赖文件系统路径；离线可用；Tauri 只需内置 splash 页，无需另配控制台资源。
- 备选：Tauri 自带 `frontendDist` —— 浏览器访问服务时没有 UI；运行时读目录——安装包路径易碎。
- 代价：`cargo build` 前必须先构建前端。用 `build.rs` 在 `dist` 缺失时生成占位页并打印警告，保证 `cargo check/test` 不被阻塞；发布流水线强制先 `pnpm build:web`。

### D3. 状态核心采用「会话 + 聚合」模型

事件按 `(agent, session)` 归并为会话，每个会话有状态、消息、TTL；聚合状态取活动会话中的最高优先级（`error > waiting > working > idle`），无会话为 `none`。

- 理由：多 agent 并发是常态，需要确定性规则；TTL 解决 agent 崩溃/关闭时无法通知的问题；聚合结果是灯效的唯一输入，UI 与设备看到同一结论。
- 备选：全局单状态——多 agent 会互相覆盖；事件队列回放——查询与恢复复杂，收益低。
- 默认 TTL 15 分钟，可由事件覆盖；同优先级按最近更新时间决胜。

### D4. 灯效语义化，协议映射下沉到 profile

对外只暴露 `off` / `calm` / `busy` / `attention` / `alert` 五个语义灯效；每个设备 profile 定义「语义灯效 → 协议命令计划」，包含前置命令、间隔与去重键。

| 灯效 | mlight-3led | mlight-single | yh50-rgy-j |
| --- | --- | --- | --- |
| off | `OFF` | `OFF` | `FF 01 01 01 AA` |
| calm | `GREEN_ON` | `GREEN_ON` | `FF 02 01 01 AA` |
| busy | `GREEN_ON` + 300ms + `YELLOW_BREATH` | `YELLOW_ON` + 80ms + `YELLOW_BREATH` | `FF 03 01 01 AA`（黄常亮） |
| attention | `YELLOW_BLINK` | `YELLOW_BLINK` | `FF 03 01 03 AA`（黄 1.7s 闪烁） |
| alert | `RED_BLINK` | `RED_BLINK` | `FF 04 01 03 AA`（红 1.7s 闪烁；蜂鸣开启时蜂鸣字段为 `02`） |

- 理由：YH50 同一时刻只能一种颜色，若让 UI 直接选颜色/通道，就无法跨设备统一；语义层让「等待确认」在不同设备上都表现为黄色警示。
- 备选：直接暴露颜色枚举——需要为每个型号做特例 UI，且配置无法迁移。
- MLight 的 `OFF` + 350ms 前置、300ms 组合间隔、单灯珠 80ms 预置、重复命令抑制，都实现在适配器的命令计划里。
- YH50 的帧格式与闪烁档位以 deepsight 已验证实现为准（见 D13）；默认映射用 1.7s 中速闪烁，避免与「快闪」语义绑死。

### D5. 设备层每设备一个 actor，latest-wins

`DeviceManager` 负责发现与去重；每台设备由独立 tokio 任务持有串口与适配器，从 `watch` 通道读取「期望灯效」，与已应用值比较后执行命令计划。计划执行中若期望值变化，立即中止当前计划、按新值重算（不排队陈旧命令）。

- 理由：串口写入是慢操作（含 sleep 时序），不能阻塞 HTTP 请求；latest-wins 避免状态快速切换时积压一串过期灯效。
- 备选：全局串口锁 + 同步写入——请求延迟抖动，且多设备互相阻塞。
- 设备 actor 内部用 `tokio::time::sleep` 与可注入时钟，测试时 `tokio::time::pause()` 可精确断言时序。

### D6. 状态核心串行化更新，SSE 作为输出总线

`BeaconState` 用一把 `Mutex` 串行化会话、配置与手动覆盖的更新，变更后通过 `tokio::sync::broadcast` 广播完整快照；`watch` 通道把期望灯效推给设备 actor。axum handler 只做协议转换与状态调用。SSE 连接先发 `snapshot`，之后发增量；`Lagged` 的慢客户端直接断开。

- 理由：锁内没有 IO 与 await，临界区很短；实现比「actor + 命令通道」简单，同时保证一次更新对应一次广播。
- 备选：actor + mpsc——隔离性更强但样板代码多，v0 不需要；`RwLock` + 轮询——广播与一致性更差；WebSocket——双向能力用不上。

### D7. 配置由服务独占写入

配置存为 JSON（带 `version` 字段），位于平台配置目录（Linux `~/.config/agent-beacon/config.json`，macOS `~/Library/Application Support/agent-beacon/config.json`，Windows `%APPDATA%\agent-beacon\config.json`），通过 `PUT /api/v1/config` 修改，落盘用临时文件 + rename 原子替换。

- 理由：浏览器与桌面 App 可能同时打开，若都直接写文件会互相覆盖；服务是唯一写者，客户端纯 API。
- 备选：TOML——可读但需要额外依赖且无结构化校验；SQLite——v0 数据量不值得。
- 校验在服务端完成，非法配置返回 400 且不落盘。
- v0 配置结构（字段名即 API 契约，后续只增不改）：

```json
{
  "version": 1,
  "server": { "host": "127.0.0.1", "port": 6980 },
  "mapping": { "none": "off", "idle": "calm", "working": "busy", "waiting": "attention", "error": "alert" },
  "manual_override_default_secs": 60,
  "shutdown": { "turn_off_lights": true },
  "mlight": { "brightness": 100 },
  "yh50": { "buzzer_on_alert": false },
  "devices": [{ "serial": "E6614C03...", "port": null, "profile": "mlight-3led" }],
  "mock": { "enabled": false }
}
```

### D8. 前端：JS + Zustand + SSE，单一 store

React 19 + Vite + Tailwind v4 + shadcn/ui（组件源码直接放入 `src/components/ui`）+ Zustand。`useBeaconStore` 保存快照、连接状态与操作结果；`useBeaconStream` 建立 EventSource 并在 `onmessage` 合并快照。API 调用封装在 `src/lib/api.js`。

- 理由：纯 JS 符合用户约束；shadcn/ui 是复制式组件，不引入运行时依赖；SSE 无需额外库。
- 备选：TanStack Query——缓存与重试能力更强，但单一实时快照场景收益有限，增加心智负担。
- 路由用 `react-router-dom` 的三个页面：`/`、`/control`、`/settings`；未知路径回退首页。
- 开发期 Vite 固定端口 **6973**（`server.port = 6973`、`strictPort: true`），`/api` 代理到后端 `http://127.0.0.1:6980`；生产由服务在 6980 同源托管，无需跨域配置。

### D9. HTTP 采用 axum，错误统一 JSON

所有 `/api/v1/*` 返回 JSON；错误体 `{ "error": "..." }`；修改类接口要求 `Content-Type: application/json`，默认不发送 CORS 头，从而阻断跨站表单/脚本写入；服务默认只绑定回环地址，非回环地址必须通过显式 `--host` 或配置开启并打印告警。

- 理由：本地无鉴权的服务最大风险是浏览器跨站请求；JSON content-type + 无 CORS 足以阻止简单请求，且不给 hook 增加 token 负担。
- 备选：预共享 token——更严格，但每个 hook/脚本都要配置，v0 可后续以配置项加入。

### D10. Tauri 生命周期：splash + stdout 握手 + 进程监管

参考 solidway 桌面壳（`~/workspace/solidway/solidway/app/src/supervisor.rs`、`config.rs`）。启动流程：

1. 单实例插件最先注册，重复启动只唤起已有窗口，不派生服务。
2. 读取共享配置中的服务端口（缺省 6980），先探测 `GET /api/v1/health`：健康则附着，不启动新进程。
3. 否则在非 `--hidden` 时显示随包内置 splash（`ui/index.html`，Tauri `frontendDist`），以 `serve --supervised --port <port>` 拉起 helper；`--hidden` 时不显示窗口，只驻留托盘。
4. 扫 stdout 找 `agent-beacon: listening addr=...`；收到后做健康检查，通过才创建主窗口加载 `http://<addr>/` 并关闭 splash（上限 10 秒）。
5. `bind-failed`、超时、子进程退出时 splash 变为状态页：失败分类、建议、日志末尾与「重试」按钮，不出现空白窗口。

监管细节：

- stdout 事件契约固定为 `agent-beacon: <event> k=v ...`：ASCII、不本地化、不经 `tracing`，`RUST_LOG` 过滤不会吞掉；消费方按行扫描前缀，不能假设是首行。
- 子进程 stdin 以管道打开并保持；服务在 `--supervised` 下把 stdin EOF 当作退出信号，App 崩溃时管道关闭即可让服务退出，无需轮询父进程。
- Unix 壳安装 SIGTERM/SIGINT/SIGHUP 处理器，被中断时同步杀死子进程；正常退出先 SIGTERM，3 秒未退再强杀。
- Windows 派生时加 `CREATE_NO_WINDOW`，不弹控制台窗口；`AgentBeaconApp.exe` 与 `agent-beacon.exe` 即使大小写不敏感也不冲突。
- helper 定位顺序：可执行文件同目录 → Tauri 资源目录 → 开发期 `target/<profile>` → `PATH`；失败时状态页列出尝试过的路径。
- 子进程 stdout/stderr 追加写入应用数据目录的 `agent-beacon.log`，状态页展示末尾若干行（去 ANSI）。
- 服务被主动停止或运行中不可达时，复用同一状态页（含「重新启动」），不显示 WebView 的网络错误页。

- 理由：splash 早于服务就绪出现，避免白屏；stdout 握手让 App 不假设端口，绑定失败可读；stdin EOF + 信号处理覆盖崩溃与正常退出两条路径。
- 备选：固定端口扫描 6980–6989——会改变浏览器访问地址，且与「配置端口」语义冲突；`--parent-pid` 轮询——需要跨平台进程查询，stdin 更简单可靠。
- 端口冲突时不自动换端口：保持浏览器访问地址稳定，由状态页提示用户释放端口或改配置。

### D11. 构建与目录结构

```
Cargo.toml                     # workspace
package.json                   # pnpm 脚本：dev / build:web / package:* / tauri:*
crates/
  agent-beacon-core/           # 会话、聚合、灯效、配置模型（纯逻辑，无 IO）
  agent-beacon-lights/         # 串口发现、MLight/YH50/mock 适配器
  agent-beacon/                # axum API、SSE、静态资源、配置读写、CLI → bin agent-beacon
web/                           # React SPA（由服务内嵌）
desktop/
  src-tauri/                   # Tauri 壳 → bin AgentBeaconApp
    ui/                        # 本地 splash / 错误状态页（frontendDist）
scripts/
  package-linux-app.sh         # deb + AppImage
  package-mac.sh               # .app + dmg（签名顺序参考 solidway）
  package-windows.sh           # NSIS/MSI + 便携 zip
docs/                          # 协议文档、接入示例、打包说明
```

- `agent-beacon-core` 不依赖串口与 HTTP，便于单元测试；`agent-beacon-lights` 依赖 `serialport`；`agent-beacon` 组合两者，负责配置落盘、CLI 与 API。
- 设备发现用 `serialport::available_ports()` 的 `UsbPortInfo`（vid/pid/serial_number），2 秒轮询；设备 id 优先 USB 序列号，其次端口名。YH50 在 Linux/macOS 另外用 `rusb` 枚举，以支持直接 Bulk 传输。

### D12. 测试策略

- 单元测试：聚合优先级、TTL 过期、映射表、MLight 白名单与命令计划、YH50 帧编码、配置校验。
- 集成测试：axum Router + `tower::ServiceExt` 覆盖 API 契约；mock 设备 + `tokio::time::pause()` 断言命令序列与时序。
- 前端：Vitest + Testing Library 覆盖 store 与关键交互；不做 v0 的端到端浏览器矩阵。
- 硬件：`docs/hardware-checklist.md` 人工验收（CRLF 兼容、未知命令、连续帧间隔等协议文档中的待验证项）。

### D13. YH50 传输以 deepsight 已验证实现为准

参考 `~/workspace/deepsight/app/alertapp/src-tauri/src/lib.rs`（已实测）：

- 非 Windows：先用 `rusb` 直接访问 `1A86:7523`（config 1、interface 0、第一个 Bulk OUT 端点、1000ms 超时）写 5 字节帧；失败或找不到设备时回退串口。macOS 上接口通常已被 Apple 自带驱动绑定，claim 失败即走串口回退。
- Windows：仅串口。CH340 不是 USB CDC-ACM 类设备，Windows 没有内置驱动，需要 WCH CH341SER（联网时可能由 Windows Update 自动安装，离线、LTSC 或更新被拦截时需手动安装）。
- macOS 10.14 起系统自带 `AppleUSBCHCOM`（macOS 11+ 为 `com.apple.DriverKit-AppleUSBCHCOM.dext`），CH340/CH341 免驱；不要安装 WCH 旧 kext，否则可能出现第二个无效端口。只有 10.9–10.13 才需要 WCH 驱动。
- Linux 内核自带 `ch341` 驱动（`/dev/ttyUSB*`），只需 `dialout`/udev 权限。
- 串口：9600 8N1、无流控、读写超时 1000ms、`write_all` + `flush`；不额外操作 DTR/RTS（厂商文档中的 DTR/RTS 时序来自 Electron 实现，以已验证的 Rust 实现为准）。
- 不读取设备回显；写成功即视为帧已发出，失败交给设备 actor 重连。
- 闪烁档位用 Medium `0x03`（1.7s），与参考实现的 `red_flash` / `yellow_flash` 预设一致。

### D14. 打包按 solidway 的分平台脚本

不把服务声明为 Tauri `externalBin`，而是用 `bundle.resources` 打入固定文件名 `agent-beacon` / `agent-beacon.exe`：

- 壳按固定名定位（可执行文件同目录 → 资源目录），不需要处理 target triple 后缀；
- Windows 资源目录就是安装目录，天然满足「helper 与 App 同目录」；
- macOS 需要把 helper 放入 `Contents/MacOS` 并按「先内层、后 .app」顺序签名，资源注入后由脚本 post-copy 更可控。

流程（参考 `~/workspace/solidway/solidway/scripts/package-*.sh`）：

1. `pnpm build:web` → `cargo build --release -p agent-beacon`。
2. 把 helper 复制到 `desktop/src-tauri/`，用临时 `--config` JSON 注入 `bundle.resources`（源路径用绝对路径），构建后删除。
3. Linux：`cargo tauri build --bundles deb,appimage`，设置 `APPIMAGE_EXTRACT_AND_RUN=1`。
4. macOS：先构建 `.app`，post-copy helper 到 `Contents/MacOS`；配置了签名身份时按「内层 helper → 外层 .app」顺序签名并从已签名 `.app` 组装 dmg，未配置时产出未签名 dmg 并告警（不使用 ad-hoc 签名冒充正式签名）。
5. Windows：NSIS/MSI；另出便携 zip（`AgentBeaconApp.exe` + `agent-beacon.exe` 同目录）。
6. 版本一致性：`tauri.conf.json`、各 Cargo.toml 与 package.json 由脚本校验，CI 在 tag 上执行。

### D15. 开机自启动由桌面壳管理

- 使用 `tauri-plugin-autostart`（macOS LaunchAgent、Linux XDG autostart、Windows 注册表启动项），注册参数带 `--hidden`。
- 托盘提供「开机自启动」复选项；偏好写入应用数据目录 `desktop-prefs.json`，启动时与系统登录项对账，以系统读回状态为准。
- `--hidden` 启动不创建 splash 与主窗口，服务照常拉起并驻留托盘；启动失败仍展示状态页，避免静默失败。
- 系统调用失败时回滚勾选与偏好，并在状态页显示原因。
- 不做登录/口令/引导：应用就绪后直接进入控制台；Web 控制台设置页只提示「开机自启动请在桌面应用托盘中设置」，不提供该开关（浏览器无法操作系统登录项）。

## Risks / Trade-offs

- [串口/设备权限差异：Linux 串口需 `dialout`，libusb 直连需 udev；Windows 的 CH340 无内置驱动；macOS 10.14+ 自带驱动、装旧 WCH kext 反而冲突] → 按平台写安装文档 + 启动时检测并展示 `last_error` + mock 设备保证无硬件可跑通全链路。
- [MLight 无 ACK，固件对行尾/未知命令行为未验证] → 严格白名单 + 只用 `LF` + 时序遵循实测结论 + 硬件验收清单覆盖待验证项。
- [Tauri WebView 依赖系统组件（Linux WebKitGTK、Windows WebView2）] → 打包文档列明依赖；若 WebView 不可用则回退为打开系统浏览器并提示。
- [无鉴权的本地服务可能被本机其他进程调用] → 仅回环绑定 + 禁 CORS + JSON content-type；文档明确本地信任边界。
- [内置服务与 App 版本不一致（用户手动运行旧版服务）] → 健康接口返回版本，附着时校验主版本，不匹配则提示并提供「改用内置服务」。
- [SSE 慢消费者占用资源] → broadcast 有界 + Lagged 即断开，客户端负责重连拉快照。
- [libusb 直接访问与 ch341 内核驱动争用] → claim/write 失败即回退串口；两条路径共用同一帧编码测试。
- [应用被杀导致服务成为孤儿] → stdin EOF 看门狗 + Unix 信号处理器 + 退出时 3 秒强杀，三重兜底。
- [macOS 公证/Windows 签名缺失导致首次启动被拦截] → v0 在文档说明；打包脚本按 solidway 的「内层先签、外层后签」顺序预留签名变量，发布不使用 ad-hoc 签名。
- [端口 6980 被占用] → 服务输出 bind-failed 事件，桌面 splash 展示占用原因与重试；不自动换端口，保证浏览器访问地址稳定。

## Migration Plan

全新项目，无数据迁移。上线步骤：

1. 本地开发链路：`pnpm install` → `pnpm build:web` → `cargo test --workspace` → `pnpm tauri:dev`。
2. 发布构建：按平台执行 `scripts/package-linux-app.sh`、`scripts/package-mac.sh`、`scripts/package-windows.sh`（内部完成前端构建、helper 构建、资源注入与 `tauri build`）。
3. 回滚策略：v0 无持久化业务数据，回滚即卸载重装；配置文件保留向后兼容的 `version` 字段，后续迁移在启动时按版本升级。

## Open Questions

- 是否要在 v1 增加 Windows Toast / macOS 通知，用于 `error` 状态提醒。
- 是否提供「一键生成 agent hook 配置」的辅助命令（目前先提供文档模板）。
