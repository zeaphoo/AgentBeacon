# Tasks

## 1. 仓库脚手架与工具链

- [x] 1.1 建立 Cargo workspace 与 `crates/agent-beacon-core`、`crates/agent-beacon-lights`、`crates/agent-beacon` 骨架，配置共享依赖与 lints；验证：`cargo check --workspace` 通过，目录结构与 design.md 一致
- [x] 1.2 用 pnpm + Vite 初始化 `web/`（React、纯 JS），接入 Tailwind 与 shadcn/ui 初始化、Zustand、react-router；验证：`pnpm --dir web build` 产出 `web/dist/index.html`
- [x] 1.3 初始化 `desktop/src-tauri`（Tauri v2），产品名 `AgentBeacon`、二进制名 `AgentBeaconApp`，配置本地 splash 页（`frontendDist: ui`）与单实例、托盘、开机自启动插件依赖；验证：`cargo check -p agent-beacon-app` 通过，`AgentBeaconApp` 产物名符合预期
- [x] 1.4 增加根 `package.json` 脚本（`dev:web`、`dev:serve`、`build:web`、`package:linux`、`package:mac`、`package:win`、`tauri:dev`、`tauri:build`）与开发说明；`dev:web` 固定 Vite 端口 6973 并把 `/api` 代理到 6980；验证：按 README 启动后 `http://127.0.0.1:6973` 可访问界面，`cargo run -p agent-beacon` 监听 6980

## 2. 状态核心（agent-status）

- [x] 2.1 定义事件与会话类型、状态枚举与输入校验（字段、合法状态、TTL 范围），缺省 `session` 归一为 `{agent}:default`；验证：`cargo test -p agent-beacon-core session` 覆盖缺省会话与非法输入
- [x] 2.2 实现聚合算法（`error > waiting > working > idle`、同优先级按最近更新、空会话为 `none`）；验证：单元测试覆盖各优先级组合与同优先级决胜
- [x] 2.3 实现 TTL 过期与清空会话，注入时钟以便测试；验证：使用暂停时钟的测试断言过期后会话移除、重复事件刷新时限
- [x] 2.4 定义状态到语义灯效的默认映射与可覆盖配置结构；验证：单元测试断言五种状态映射结果与非法灯效被拒绝

## 3. 灯控与协议（light-control）

- [x] 3.1 定义设备 profile、命令计划（前置命令/间隔/去重键）与 `mock` 设备记录器；验证：`cargo test -p agent-beacon-lights plan` 断言各灯效的命令序列与时序参数
- [x] 3.2 实现 MLight 适配器：115200 8N1、`LF` 结尾、白名单校验、`OFF` + 350ms 前置、组合 300ms 间隔、单灯珠 80ms 预置、重复命令抑制；验证：适配器单元测试断言编码字节与调用时序
- [x] 3.3 实现 YH50 适配器：非 Windows 优先 `rusb` Bulk（config 1 / interface 0 / 首个 Bulk OUT / 1000ms）并在失败时回退串口，Windows 仅串口；9600 8N1、超时 1000ms、5 字节帧二进制写入；验证：单元测试断言 `alert` 帧为 `FF 04 01 03 AA`、蜂鸣器字段与回退路径选择
- [x] 3.4 实现设备发现与识别：串口 VID/PID 匹配、`rusb` 枚举（YH50）、排除 `0x101F`、序列号/端口绑定、profile 覆盖；验证：用注入的伪端口/设备列表测试匹配与忽略逻辑，`cargo test -p agent-beacon-lights discovery`
- [x] 3.5 实现每设备 actor：latest-wins、写入失败标记断开、1s→30s 退避重连、重连后重放最后灯效；验证：暂停时钟的集成测试断言命令不积压、重连后重放
- [x] 3.6 实现手动覆盖（时长、自动到期、按设备或全局、取消）；验证：单元测试断言有效期内的生效设备集合与到期回落

## 4. 服务与 API（beacon-service）

- [x] 4.1 实现配置模块：跨平台配置目录、默认配置、JSON 读写、原子替换、字段校验与版本字段；验证：`cargo test -p agent-beacon config` 覆盖默认生成、非法配置拒绝、重启后保留
- [x] 4.2 实现 `BeaconState`（命令、快照、broadcast 事件），axum 之外可独立测试；验证：单元测试断言事件上报后快照与广播事件一致
- [x] 4.3 实现 `/api/v1` 路由：health、events、agents、sessions、state、devices、devices/scan、control、control/auto、config；验证：`tower::ServiceExt` 集成测试覆盖状态码、错误体与 JSON 字段
- [x] 4.4 实现 SSE：连接即发快照、变更推送、15 秒心跳、慢客户端断开；验证：集成测试读取事件流并断言快照/更新/心跳，模拟 lagged 客户端被断开
- [x] 4.5 实现静态资源内嵌与 SPA 回退，`build.rs` 在 dist 缺失时生成占位页；验证：`cargo test -p agent-beacon static` 断言 `/` 与 `/settings` 返回 HTML，`cargo check` 在未建前端时仍通过
- [x] 4.6 实现受监管运行：`--supervised` + stdin EOF 收尾、stdout `listening`/`bind-failed` 事件、单实例复用、优雅退出（默认关灯）；验证：子进程测试断言事件行格式、stdin 关闭后 5 秒内退出、端口占用时输出 bind-failed 并以非 0 退出
- [x] 4.7 实现 CLI `report|status|devices|serve`；验证：对本地测试服务执行各子命令，断言退出码与输出，服务未启动时 `report` 在 1 秒内失败

## 5. Web 控制台（web-console）

- [x] 5.1 实现 API 客户端、Zustand store 与 `useBeaconStream`（EventSource 重连、快照合并）；验证：Vitest 覆盖 store 合并、断线重连与错误状态
- [x] 5.2 实现仪表盘：聚合状态、灯效、会话列表、设备列表、空状态；验证：Testing Library 用 mock 快照断言渲染内容与更新
- [x] 5.3 实现手动控制页：灯效选择、时长、设备范围、倒计时、恢复自动；验证：Testing Library 断言请求参数与到期后界面回落
- [x] 5.4 实现设置页：映射编辑、默认覆盖时长、退出关灯、MLight 亮度、YH50 蜂鸣、设备 profile 绑定、mock 开关，并提示「开机自启动请在桌面应用中设置」，含失败提示且不丢输入；验证：交互测试断言 PUT 请求体、400 错误展示与自启动提示
- [ ] 5.5 实现三页导航、响应式布局与键盘可达；验证：375px 视口快照无横向滚动，Tab/Enter 可完成主要操作
- [x] 5.6 接入 `pnpm build:web` 产物到服务内嵌流程并在开发模式配置 `/api` 代理；验证：`http://127.0.0.1:6973`（Vite dev）与 `http://127.0.0.1:6980/`（服务内嵌）都能看到控制台并连上 SSE

## 6. 桌面 App（desktop-app）

- [x] 6.1 实现服务监管模块：helper 定位（可执行文件同目录→资源目录→dev target→PATH）、附着探测、`--supervised` 派生（stdin 管道、Windows `CREATE_NO_WINDOW`）、stdout 事件解析与日志落盘；验证：单元测试覆盖定位顺序/去重与事件解析，smoke 覆盖附着/拉起/失败路径
- [x] 6.2 实现 splash 状态页与主窗口：非 `--hidden` 启动即显示 splash，收到 `listening` 且健康检查通过后创建主窗口；失败/超时/停止时展示分类、建议、日志末尾与重试；验证：手动 smoke 覆盖启动中、成功、端口占用、helper 缺失与停止服务
- [ ] 6.3 实现托盘菜单（打开、启动/停止服务、关闭灯效、退出）、关闭隐藏到托盘、单实例唤起，以及服务停止时的内置状态页与「重新启动」按钮；验证：手动 smoke 覆盖关闭/重开/重复启动/停止服务后状态页重启/退出
- [x] 6.4 实现端口冲突提示：读取配置端口探测附着、不静默换端口，`bind-failed` 在 splash 展示地址与原因；验证：占用 6980 后启动 App，splash 显示占用原因；释放后点击重试成功并加载 `http://127.0.0.1:6980`
- [ ] 6.5 实现外链交由系统浏览器、禁止窗口导航到外部站点；验证：手动点击文档链接，默认浏览器打开且窗口地址不变
- [x] 6.6 实现开机自启动：`tauri-plugin-autostart`（注册参数 `--hidden`）、托盘复选与 `desktop-prefs.json` 持久化、启动对账、失败回滚；验证：三平台检查登录项存在/移除，重启后勾选保持，`--hidden` 启动不弹窗且服务在运行

## 7. 打包与接入文档

- [ ] 7.1 实现三平台打包脚本：前端构建 → `cargo build -p agent-beacon` → 临时 `--config` 注入 `bundle.resources` → `tauri build`；Linux deb/AppImage、macOS post-copy + 内层签名 + dmg、Windows NSIS/MSI + 便携 zip；验证：各平台产物内同时存在 `AgentBeaconApp` 与 `agent-beacon`
- [ ] 7.2 配置三平台 CI（前端构建 → helper 构建 → 打包脚本），并加入 `cargo test --workspace`；验证：GitHub Actions 在 Linux/macOS/Windows 矩阵全部通过
- [x] 7.3 编写接入文档：Claude Code 等 agent 的 hook 示例、curl 示例、Linux `dialout`/udev 权限（含 libusb 直连）、Windows CH340 驱动、macOS 免驱与旧 WCH kext 冲突说明、协议待验证项的人工验收清单；验证：照文档在干净环境完成一次「hook 上报 → 灯效变化」
- [x] 7.4 编写 README（架构、目录、开发与运行方式、二进制命名）；验证：README 中每条命令可执行且结果与描述一致

## 8. 端到端验证

- [x] 8.1 使用 mock 设备跑通「CLI report → 聚合 → 设备命令 → 控制台更新」完整链路集成测试；验证：单条 `cargo test -p agent-beacon e2e` 断言最终命令序列与快照
- [x] 8.2 在 Linux 桌面环境完成一次 `pnpm tauri:dev` 与 `scripts/package-linux-app.sh` 的 smoke（splash、附着/拉起、托盘、开机自启动、退出清理）；验证：进程列表无孤儿、登录项状态正确、灯效按预期关闭
- [x] 8.3 在有硬件时按 `docs/hardware-checklist.md` 验收 MLight 与 YH50 的实际灯效与时序；验证：清单逐项勾选并记录未通过项（MLight、YH50 核心灯效 2026-11-03 已真机通过；蜂鸣按用户要求跳过，拔插/协议 §9 未覆盖）
