# Spec Delta

## Purpose

提供一个常驻、可被本地程序调用与托管的服务进程，承载状态 API、实时推送、配置持久化与 Web 控制台，是 agent 与桌面 App 之间的唯一中枢。

## ADDED Requirements

### Requirement: 服务启动与回环监听

`agent-beacon` 不带子命令或使用 `serve` 子命令时 SHALL 启动服务，默认监听 `127.0.0.1:6980`；端口可由命令行参数、环境变量或配置文件覆盖。除显式配置非回环地址外，服务 MUST 只接受回环地址的连接。

#### Scenario: 默认监听

- **WHEN** 以默认参数启动服务
- **THEN** 只有 `127.0.0.1:6980` 可访问 API，局域网地址无法连接

#### Scenario: 自定义端口

- **WHEN** 以 `--port 9090` 启动
- **THEN** 服务监听 `127.0.0.1:9090`，健康检查在该端口返回成功

### Requirement: 单实例与端口冲突

同一端口上若已有健康的 agent-beacon 实例，新进程 SHALL 复用该实例并正常退出，不得重复占用端口。若端口被其他程序占用，新进程 MUST 以非 0 退出码结束并输出可操作的错误信息，不得杀死其他进程。

#### Scenario: 重复启动

- **WHEN** 已有实例运行在 6980，再启动一个使用相同端口的服务
- **THEN** 新进程检测到健康实例后退出，退出码为 0，已有实例继续服务

#### Scenario: 端口被占用

- **WHEN** 6980 被无关程序占用
- **THEN** 服务以非 0 退出码结束，stderr 指明端口被占用及更换端口的方法

### Requirement: 健康检查

服务 SHALL 提供 `GET /api/v1/health`，返回 `status`、`version`、`pid`、`port`、`uptime_secs`，并在 200 毫秒内响应。桌面 App 与 CLI SHALL 以该接口判断服务是否可用。

#### Scenario: 探活

- **WHEN** 客户端请求 `GET /api/v1/health`
- **THEN** 返回 200 与 `{"status":"ok","version":"...","pid":123,"port":6980,"uptime_secs":42}`

### Requirement: HTTP API 契约

服务 SHALL 在 `/api/v1` 下提供：`GET /health`、`POST /events`、`GET /agents`、`DELETE /sessions`、`GET /state`、`GET /stream`、`GET /devices`、`POST /devices/scan`、`POST /control`、`POST /control/auto`、`GET /config`、`PUT /config`。`GET /state` SHALL 返回会话、聚合状态与设备灯效的组合快照。所有响应 SHALL 为 JSON，错误响应包含可读的 `error` 字段与对应 HTTP 状态码。

#### Scenario: 统一错误格式

- **WHEN** 客户端向不存在的 `/api/v1/nope` 发起请求
- **THEN** 返回 404 且响应体包含 `error` 字段，不返回 HTML

#### Scenario: 组合快照

- **WHEN** 客户端请求 `GET /api/v1/state`
- **THEN** 响应同时包含 `aggregate`、`sessions` 与 `devices`，与当前 SSE 快照一致

### Requirement: 实时状态推送

服务 SHALL 提供 `GET /api/v1/stream` 的 SSE 流：连接建立后先发送一次完整状态快照，之后在状态、设备或配置变化时推送更新，并每 15 秒发送心跳。慢消费者 MUST 被单独断开，不得阻塞服务或其他客户端。

#### Scenario: 建立连接

- **WHEN** 客户端连接 `/api/v1/stream`
- **THEN** 首先收到 `snapshot` 事件，随后状态变化在 500 毫秒内收到对应更新事件

#### Scenario: 断线重连

- **WHEN** 客户端连接中断后重新连接
- **THEN** 服务接受新连接并重新发送完整快照，旧连接被回收

### Requirement: 配置持久化

服务 SHALL 将配置保存为 JSON 文件，存放于当前操作系统的用户配置目录；首次运行 SHALL 生成带默认值的配置文件。写入 MUST 为原子替换，配置非法时 MUST 拒绝保存并保留原文件。

#### Scenario: 保存并重启

- **WHEN** 用户修改灯效映射并通过 `PUT /api/v1/config` 保存，随后重启服务
- **THEN** 新配置仍然生效

#### Scenario: 拒绝非法配置

- **WHEN** 提交包含未知灯效名称的配置
- **THEN** 服务返回 400，磁盘上的配置保持修改前内容

### Requirement: 静态控制台托管

服务 SHALL 在 `/` 提供编译期内嵌的 Web 控制台静态资源，并对未知的非 API 路径回退到入口页面。控制台 MUST 在完全离线、无外部 CDN 的条件下可用。

#### Scenario: 打开控制台

- **WHEN** 浏览器访问 `http://127.0.0.1:6980/`
- **THEN** 返回控制台页面；访问 `/settings` 等前端路由也返回同一入口页面

### Requirement: 受监管运行与就绪通告

服务 SHALL 在绑定成功后向 stdout 输出一行 `agent-beacon: listening addr=<host:port>`，绑定失败时输出 `agent-beacon: bind-failed addr=<addr> kind=<kind>` 并以非 0 退出码结束。服务 SHALL 支持 `--supervised`：启用后把 stdin 作为存活信号，读到 EOF 时按优雅退出流程停止；未启用时 MUST NOT 依赖 stdin，可独立运行。

#### Scenario: 托管方退出

- **WHEN** 以 `--supervised` 启动的服务检测到 stdin 关闭（桌面 App 崩溃或退出）
- **THEN** 服务在 5 秒内退出、释放端口，并按配置关闭灯效

#### Scenario: 端口绑定失败

- **WHEN** 以 `--supervised` 启动且配置端口已被占用
- **THEN** stdout 输出 `agent-beacon: bind-failed` 事件，进程以非 0 退出码结束，桌面 App 可据此展示原因

### Requirement: 优雅退出

服务 SHALL 在收到 SIGINT 或 SIGTERM 后停止接受新请求并退出，退出前按配置执行最终灯效（默认 `off`），整个过程 MUST 在 3 秒内完成。

#### Scenario: 正常关闭

- **WHEN** 服务收到 SIGTERM，且配置为退出时关闭灯效
- **THEN** 设备收到 `off` 对应命令，进程在 3 秒内以 0 退出

### Requirement: 本地信任与无外发网络

服务 MUST NOT 主动向外部网络发送数据，也不收集遥测；状态与配置仅保存在本机。跨源浏览器请求默认不获得 CORS 许可，修改类接口 SHALL 要求 JSON 请求体。

#### Scenario: 不产生外发连接

- **WHEN** 服务运行并处理状态上报
- **THEN** 除回环地址外不建立任何出站网络连接

### Requirement: 运维 CLI

`agent-beacon status` SHALL 输出服务健康、聚合状态、活动会话数与设备摘要；`agent-beacon devices` SHALL 输出设备列表。两者在服务不可达时 MUST 以非 0 退出码结束并给出原因。

#### Scenario: 查看状态

- **WHEN** 执行 `agent-beacon status`
- **THEN** 输出服务地址、版本、聚合状态、会话数与设备连接数，退出码为 0
