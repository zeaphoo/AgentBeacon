# Spec Delta

## Purpose

设备层从「所有设备消费同一个灯效」改为「每台设备消费自己的目标状态」，并让手动覆盖、设备查询与新配置字段在多设备分配场景下保持可解释。

## MODIFIED Requirements

### Requirement: 设备型号与配置覆盖

系统 SHALL 按设备型号选择协议配置（profile）：`mlight-single`（单灯珠，如 `single-rgb-1003`）、`mlight-3led`（三灯珠）、`yh50-rgy-j`、`agentcore-light`、`mock`。当自动识别不可靠时，用户 SHALL 能按设备 id、USB 序列号或端口名绑定 profile。

设备绑定 SHALL 同时承载分配相关设置：`label`（仅展示）、`assignment`（`mirror`/`auto`/`manual`，缺省 `mirror`）、`brightness` 与 `buzzer_on_alert`（每设备覆盖）。`label` MUST NOT 参与设备匹配。本变更不提供 `enabled` / 删除设备：设备只有检测到与未检测到两种状态。

#### Scenario: 单灯珠组合命令

- **WHEN** profile 为 `mlight-single` 且灯效 `busy` 需要绿常亮叠加黄呼吸
- **THEN** 设备只收到最后一条动画命令 `YELLOW_BREATH`，不发送组合中的全部命令

#### Scenario: 三灯珠组合命令

- **WHEN** profile 为 `mlight-3led` 且灯效 `busy`
- **THEN** 设备先收到 `GREEN_ON`、间隔 300 毫秒后再收到 `YELLOW_BREATH`

#### Scenario: 按序列号绑定并命名

- **WHEN** 配置 `{"serial":"ABC123","profile":"mlight-3led","label":"左灯","assignment":"auto"}`
- **THEN** 该设备使用 `mlight-3led` 协议、界面显示「左灯」、参与自动分配

#### Scenario: 手动专用灯

- **WHEN** 配置 `{"serial":"ABC123","profile":"mlight-3led","label":"Claude 专用","assignment":"manual"}`，且 Claude 被 pin 到该设备
- **THEN** 该设备只显示 Claude 的 session，不接收其他自动 session

#### Scenario: 设备离线

- **WHEN** 设备在配置中但当前未检测到
- **THEN** 设备显示为未连接，actor 继续尝试重连；首次连接前不参与自动分配，超过 `MISSING_GRACE_SECS` 后其 session 迁移到其他在线 auto 设备

### Requirement: 手动覆盖

系统 SHALL 允许手动设置灯效覆盖自动映射。覆盖具有时长（默认 60 秒，范围 1–3600 秒），到期后自动恢复；覆盖期间聚合状态仍正常更新，覆盖可随时取消。覆盖可指定单个设备，未指定时作用于全部设备。

覆盖 MUST 压过设备的分配结果与空分配回退。全局覆盖作用于全部设备；单设备覆盖只作用于该设备。覆盖到期或取消后，设备 MUST 回到自己的分配结果，而不是全局聚合结果。服务退出关灯 MUST 压过一切覆盖。

#### Scenario: 临时点亮全部设备

- **WHEN** 用户手动选择 `alert`、时长 30 秒且不指定设备
- **THEN** 所有设备立即显示 `alert`，30 秒后各自回到自己的分配结果

#### Scenario: 只覆盖一台灯

- **WHEN** 设备 A 分到 Claude、设备 B 分到 Codex，用户只对 A 应用 `alert`
- **THEN** A 显示 `alert`，B 继续显示自己的分配结果；A 到期后回到自己的分配结果

#### Scenario: 取消覆盖

- **WHEN** 覆盖期间用户点击「恢复自动」
- **THEN** 覆盖立即结束，设备在 1 秒内回到各自的自动分配结果

### Requirement: 设备状态查询

系统 SHALL 提供 `GET /api/v1/devices`，返回每个设备的标识、端口、profile、展示标签、连接状态、最后连接时间、最近错误与当前灯效；`POST /api/v1/devices/scan` SHALL 触发一次立即扫描。设备的分配方式、分到的 agent/session 与回退原因 SHALL 通过 `GET /api/v1/state` 的 `routing` 字段提供。

#### Scenario: 查看设备

- **WHEN** 客户端请求 `GET /api/v1/devices`
- **THEN** 返回设备数组，每项包含 `id`、`port`、`profile`、`label`、`connected`、`last_seen_ms`、`effect`、`last_error` 字段

#### Scenario: 查看分配来源

- **WHEN** 客户端请求 `GET /api/v1/state`
- **THEN** `routing` 中对应设备显示分配方式、分到的 agent/session、聚合状态与最终灯效

## ADDED Requirements

### Requirement: 每设备目标状态

每台设备 actor SHALL 从 `DesiredState` 读取自己的 `DeviceDesired`（`effect`、`brightness`、`buzzer_on_alert`），不得读取其他设备的灯效或自行分配/聚合 session。

设备绑定上的 `brightness` 与 `buzzer_on_alert` SHALL 覆盖全局配置；profile 不具备该能力时 MUST 忽略对应设置。服务退出关灯时，所有设备 MUST 收到 `off`，不受分配与覆盖影响。

#### Scenario: 两台设备不同命令

- **WHEN** 设备 A 与设备 B 分别解析出 `busy` 与 `alert`
- **THEN** 两个 actor 分别下发各自 profile 的 `busy` / `alert` 命令，互不等待

#### Scenario: 每设备亮度

- **WHEN** 全局亮度为 50，设备 A 的绑定设置 `brightness: 80`
- **THEN** A 收到按 80 计算的亮度命令，其他设备仍使用 50

#### Scenario: 退出关灯压过覆盖

- **WHEN** 某设备正被手动覆盖为 `alert`，服务收到退出信号且 `shutdown.turn_off_lights = true`
- **THEN** 该设备收到 `off` 命令
