# Spec Delta

## Purpose

`AgentBeaconApp` 作为跨平台桌面入口，负责发现或拉起本地 agent-beacon 服务，并在应用窗口内呈现同一套 Web 控制台，让非技术用户双击即可使用。

## ADDED Requirements

### Requirement: 应用标识与打包

桌面应用的可执行文件名 SHALL 为 `AgentBeaconApp`，产品名称为 `AgentBeacon`；服务二进制 SHALL 以固定文件名 `agent-beacon`（Windows 为 `agent-beacon.exe`）随安装包内置。打包产物 SHALL 覆盖 Linux（deb/AppImage）、macOS（.app/dmg）、Windows（NSIS/MSI，另附便携 zip）。

#### Scenario: 安装后直接启动

- **WHEN** 用户在受支持平台安装并启动 AgentBeacon
- **THEN** 应用在无额外依赖安装的情况下可用，内置服务二进制位于可执行文件同目录或资源目录

### Requirement: 无登录直达

应用 SHALL 在服务就绪后直接加载控制台，不要求登录、口令或引导步骤；首次启动与后续启动行为一致。

#### Scenario: 首次启动

- **WHEN** 用户首次启动应用
- **THEN** 服务就绪后直接显示状态页，不出现登录页或引导页

### Requirement: 启动时附着或拉起服务

应用启动时 SHALL 读取服务配置端口（缺省 6980）并探测 `GET /api/v1/health`：健康且版本可用则附着；否则 SHALL 拉起随包服务并传入 `--supervised --port <端口>`。应用 SHALL 从服务 stdout 的 `agent-beacon: listening addr=...` 事件获知实际地址，并只加载该地址。附着模式下 MUST NOT 再启动第二个服务进程。

#### Scenario: 复用已有服务

- **WHEN** 用户已手动运行 agent-beacon，随后启动桌面应用
- **THEN** 应用复用该实例并显示界面，任务管理器中不会出现第二个服务进程

#### Scenario: 首次启动

- **WHEN** 本机没有运行中的服务
- **THEN** 应用拉起随包服务，收到 `listening` 事件并通过健康检查后加载该地址

### Requirement: 就绪等待与失败反馈

应用 SHALL 在非 `--hidden` 启动时立即显示内置 splash 状态页，并在收到 `listening` 事件且健康检查通过后创建主窗口加载控制台，最长等待 10 秒。`--hidden` 启动成功时不创建窗口，仅驻留托盘。超时、绑定失败或子进程异常退出时，splash MUST 变为可操作的错误页，展示失败分类、建议、日志末尾与「重试」按钮，不得出现空白窗口。

#### Scenario: 服务启动失败

- **WHEN** 随包服务因权限、缺少二进制或端口问题无法启动
- **THEN** 应用在 splash 错误页展示失败原因、建议、日志末尾与重试按钮，不打开空白主窗口

#### Scenario: 启动中

- **WHEN** 应用已拉起服务但尚未收到 `listening` 事件
- **THEN** splash 持续显示「正在启动」状态，不判定为失败

### Requirement: 进程归属与退出清理

应用 SHALL 只停止由本次运行拉起的服务进程，附着到的外部服务 MUST NOT 被终止。拉起的服务以 stdin 管道作为存活信号；应用退出或崩溃导致管道关闭时，服务 SHALL 自行退出。正常退出时应用 SHALL 先发送 SIGTERM，3 秒未退出再强制结束。

#### Scenario: 正常退出

- **WHEN** 用户从托盘菜单选择退出
- **THEN** 应用终止自己拉起的服务，窗口关闭，进程完全退出

#### Scenario: 应用崩溃

- **WHEN** 应用进程被强制杀死
- **THEN** stdin 管道关闭使服务在 5 秒内退出，不留孤儿进程

### Requirement: 开机自启动

应用 SHALL 在托盘提供「开机自启动」复选项，并把偏好持久化到应用数据目录。启用后应用 SHALL 在用户登录时以隐藏方式启动并驻留托盘；关闭后 MUST 移除系统登录项。系统调用失败时 MUST 回滚开关状态并显示可读原因。

#### Scenario: 启用并重启

- **WHEN** 用户在托盘勾选「开机自启动」，随后重启应用
- **THEN** 系统登录项存在，应用启动后勾选状态保持为启用

#### Scenario: 登录时隐藏启动

- **WHEN** 系统登录触发自启动
- **THEN** 应用不弹出主窗口，服务在后台启动，托盘图标可用

#### Scenario: 系统调用失败

- **WHEN** 系统拒绝写入登录项
- **THEN** 勾选回滚为原状态，状态页显示失败原因

### Requirement: 托盘与窗口生命周期

应用 SHALL 常驻系统托盘，托盘菜单包含打开主窗口、启动/停止服务、关闭灯效、开机自启动与退出。停止与重启操作 MUST 只作用于本应用拉起的服务，附着外部实例时不可停止。关闭窗口 SHALL 最小化到托盘而非退出；macOS 上 SHALL 符合平台习惯（点击 Dock 图标可重新打开窗口）。

#### Scenario: 关闭窗口

- **WHEN** 用户点击窗口关闭按钮
- **THEN** 窗口隐藏，托盘图标保留，服务继续运行

#### Scenario: 停止服务

- **WHEN** 用户从托盘选择「停止服务」
- **THEN** 主窗口切换为内置状态页，显示「服务已停止」与重新启动入口

### Requirement: 端口冲突提示

当配置端口被其他程序占用时，应用 SHALL 保持在 splash 错误页并展示 `bind-failed` 事件中的地址与原因，提供「重试」与「打开日志」入口；应用 MUST NOT 静默改用其他端口，以保证浏览器访问地址稳定。

#### Scenario: 端口被占用

- **WHEN** 6980 被其他程序占用
- **THEN** splash 显示端口占用原因与重试按钮；用户释放端口后点击重试可正常启动

### Requirement: 单实例应用

同一用户会话内 SHALL 只运行一个应用实例；重复启动时 MUST 唤起并聚焦已存在的窗口，而不是再启动一个服务或窗口。

#### Scenario: 重复双击图标

- **WHEN** 应用已在运行，用户再次双击图标
- **THEN** 已有窗口被显示并聚焦，不产生新进程

### Requirement: 外部链接与导航

应用窗口 SHALL 只加载本地服务页面；点击外部链接 MUST 交由系统默认浏览器打开，窗口本身不得导航到任意外部站点。

#### Scenario: 点击文档链接

- **WHEN** 用户点击界面中的外部文档链接
- **THEN** 默认浏览器打开该链接，应用窗口保持在控制台页面

### Requirement: 离线可用与配置共享

应用 MUST 在无网络环境下完整可用，界面资源来自本地服务；浏览器与桌面应用访问同一服务时 SHALL 共享同一份配置与状态。

#### Scenario: 断网使用

- **WHEN** 设备处于无外网环境
- **THEN** 应用仍可启动、显示界面并驱动状态灯，功能不受影响
