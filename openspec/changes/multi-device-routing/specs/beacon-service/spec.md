# Spec Delta

## Purpose

让服务快照能够解释每台设备分到了哪些 session，并让配置持久化安全地承载 agent 固定与设备分配方式，同时保持旧配置与旧客户端可用。

## MODIFIED Requirements

### Requirement: HTTP API 契约

服务 SHALL 在 `/api/v1` 下提供：`GET /health`、`POST /events`、`GET /agents`、`DELETE /sessions`、`GET /state`、`GET /stream`、`GET /devices`、`POST /devices/scan`、`POST /control`、`POST /control/auto`、`POST /routing/rebuild`、`GET /config`、`PUT /config`。`GET /state` SHALL 返回会话、全局聚合状态、设备列表、路由视图与当前多灯策略的组合快照。所有响应 SHALL 为 JSON，错误响应包含可读的 `error` 字段与对应 HTTP 状态码。

`GET /state` 与 SSE 快照 SHALL 包含：

- `aggregate` 与 `effect`：全局聚合及其映射灯效，语义不变；
- `sessions`：活动会话，新增 `created_at_ms`；
- `devices`：每台设备的连接、profile 与当前灯效，既有字段不变；
- `routing`：每台已配置或已发现的设备的路由视图，至少包含 `device`、`label`、`assignment`、`agents`、`sessions`、`aggregate`、`auto_effect`、`effect`、`overridden`、`connected`、`last_seen_ms`；
- `unassigned_agents`：没有分配到任何启用设备的 agent；

`POST /routing/rebuild` SHALL 清空粘性分配并按当前策略重新计算，MUST NOT 修改配置。新增字段 MUST 为增量字段；旧客户端忽略它们时行为不受影响。

#### Scenario: 统一错误格式

- **WHEN** 客户端向不存在的 `/api/v1/nope` 发起请求
- **THEN** 返回 404 且响应体包含 `error` 字段，不返回 HTML

#### Scenario: 组合快照

- **WHEN** 客户端请求 `GET /api/v1/state`
- **THEN** 响应同时包含 `aggregate`、`sessions`、`devices`、`routing`、`unassigned_agents` 与 `config.routing`，与当前 SSE 快照一致

#### Scenario: 旧客户端兼容

- **WHEN** 一个只读取 `aggregate`、`sessions` 与 `devices[].effect` 的旧客户端连接 SSE
- **THEN** 它仍能正常工作，新增字段被忽略

#### Scenario: 重新分配

- **WHEN** 客户端调用 `POST /api/v1/routing/rebuild`
- **THEN** 服务按当前策略重新计算分配，快照中的 `routing` 随之更新，`GET /config` 不变

### Requirement: 配置持久化

服务 SHALL 将配置保存为 JSON 文件，存放于当前操作系统的用户配置目录；首次运行 SHALL 生成带默认值的配置文件。写入 MUST 为原子替换，配置非法时 MUST 拒绝保存并保留原文件。

配置 SHALL 支持：

- `routing.strategy`：`unified` / `auto_agent` / `manual_agent` / `auto_session`；
- `routing.manual`：设备 id 到 Agent 列表的映射；
- `devices[]` 上的 `id`、`label`、`brightness`、`buzzer_on_alert`。

服务 MUST 对分配字段执行校验：`strategy` 只能为四种策略之一、`manual` 的设备 id 与 Agent 非空、同一个 Agent 不能出现在多台灯、亮度在 `0..=100`、每个绑定至少提供 `id`/`serial`/`port` 之一、`label` 不超过 64 字符。

旧配置缺少 `routing` 或新增字段时 MUST 按默认值加载，且 `version` 保持 `1`；本变更不要求配置迁移。

#### Scenario: 保存并重启

- **WHEN** 用户修改 `routing.strategy` 或 `routing.manual` 并通过 `PUT /api/v1/config` 保存，随后重启服务
- **THEN** 新策略仍然生效，session 按保存的规则重新分配

#### Scenario: 拒绝非法分配

- **WHEN** 提交 `{"routing":{"strategy":"split"}}`
- **THEN** 服务返回 400，磁盘上的配置与运行中的分配均不变

#### Scenario: 旧配置加载

- **WHEN** 服务加载一份没有 `routing` 段的旧配置
- **THEN** 策略取 `unified`、`manual` 取空，所有设备显示同一个全局状态
