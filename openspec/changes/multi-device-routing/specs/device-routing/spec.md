# Spec Delta

## Purpose

用四种全局策略决定多台状态灯如何显示 Agent / 会话：统一显示、按 Agent 自动分配、按 Agent 手动分配、按 Session 自动分配。手动配置只在「按 Agent 手动分配」时出现；设备只有检测到/检测不到两种状态。

## ADDED Requirements

### Requirement: 四种全局策略

系统 SHALL 提供 `routing.strategy`，取值为：

- `unified`：所有灯显示同一个全局聚合；
- `auto_agent`：每个 Agent 自动分到一台灯，一台灯可以显示多个 Agent；
- `manual_agent`：用户手动把 Agent 分到灯；
- `auto_session`：同一个 Agent 的多个 session 自动分散到不同灯。

缺少 `routing` 的旧配置 MUST 按 `unified` 加载，行为与现在一致。策略是全局单选，不按设备分别配置。

#### Scenario: 默认统一显示

- **WHEN** 服务加载一份没有 `routing` 的旧配置
- **THEN** 所有灯显示同一个全局状态

#### Scenario: 切换策略

- **WHEN** 用户从 `unified` 切换到 `auto_session`
- **THEN** 服务清空旧的自动分配并按 session 重新计算，配置中的 `manual` 映射保留

### Requirement: 策略决定设备角色

系统 SHALL 由全局策略推导每台设备的内部角色：

- `unified` → `mirror`：显示全部活动 session；
- `auto_agent` / `auto_session` → `auto`：参与自动分配；
- `manual_agent` → `manual`：只显示 `routing.manual` 分配给它的 Agent。

角色 MUST NOT 由用户逐设备配置。

#### Scenario: 统一显示

- **WHEN** 策略为 `unified`
- **THEN** 每台灯都聚合全部活动 session

#### Scenario: 手动模式的空灯

- **WHEN** 策略为 `manual_agent` 且某台灯没有分配任何 Agent
- **THEN** 该灯显示 `mapping.none`（默认 off），而不是全局状态

### Requirement: 按 Agent 自动分配

`auto_agent` SHALL 以 Agent 为单位分配：

1. 活动 Agent 按名字排序；
2. 已有有效归属的 Agent 保持不动；
3. 新 Agent 选择当前 Agent 数最少的可用 `auto` 设备，平手按设备 id 升序；
4. 同一 Agent 的全部 session MUST 落在同一台灯；
5. 一台灯 MAY 显示多个 Agent。

#### Scenario: 一个 Agent 多个 session

- **WHEN** 1 个 Agent 有 3 个 session，存在 3 台灯
- **THEN** 3 个 session 全部落在同一台灯，其他灯不显示该 Agent

#### Scenario: Agent 多于灯

- **WHEN** 4 个 Agent、3 台灯
- **THEN** 三台灯分别分到 2、1、1 个 Agent

#### Scenario: 新增 Agent 不移动已有

- **WHEN** 已有 3 个 Agent 分别落在 A/B/C，新增第 4 个 Agent
- **THEN** A/B/C 的归属不变，第 4 个 Agent 进入当前 Agent 数最少的灯

### Requirement: 按 Session 自动分配

`auto_session` SHALL 以 session 为单位分配：

1. 活动 session 按 `(created_at_ms, agent, session)` 排序；
2. 已有有效归属的 session 保持不动；
3. 新 session 选择当前 session 数最少的可用 `auto` 设备，平手按设备 id 升序；
4. 同一个 Agent 的多个 session MAY 分散到不同灯；一台灯 MAY 混合多个 Agent。

#### Scenario: 三 session 三灯

- **WHEN** 1 个 Agent 有 3 个活动 session，存在 3 台灯
- **THEN** 每台灯各分到 1 个 session

#### Scenario: 四 session 三灯

- **WHEN** 4 个活动 session，存在 3 台灯
- **THEN** 三台灯分别分到 2、1、1 个 session

#### Scenario: 新增 session 不移动已有

- **WHEN** 已有 3 个 session 分别落在 A/B/C，新增第 4 个
- **THEN** A/B/C 的归属不变，第 4 个进入当前 session 数最少的灯

### Requirement: 按 Agent 手动分配

`manual_agent` SHALL 使用 `routing.manual`（设备 id → Agent 列表）作为唯一映射：

- 同一个 Agent MUST 只能出现在一台灯上；配置校验 MUST 拒绝重复分配；
- 一个灯 MAY 分配多个 Agent；
- 未出现在任何灯上的 Agent 视为未分配，SHALL 出现在 `unassigned_agents`；
- 未检测到的灯 SHALL 保留其 Agent 配置并继续显示在设置页；
- 用户 SHALL 能删除未检测到的灯；删除 MUST 移除对应的 `manual` 条目和 `devices` 绑定；
- 存在于 `manual` 但不在设备列表里的灯 SHALL 保留并显示为「配置已失效」，其 Agent 视为未分配。

#### Scenario: 一台灯多个 Agent

- **WHEN** `manual` 为 `{"serial:A":["claude","codex"]}`
- **THEN** 设备 A 同时显示 Claude 与 Codex 的 session

#### Scenario: 同一个 Agent 分到两台灯

- **WHEN** 配置把 `claude` 同时放进 A 和 B 的 Agent 列表
- **THEN** 服务返回 400 配置校验错误，配置不落盘

#### Scenario: 离线灯保留配置

- **WHEN** 设备 A 已分配 Claude，随后被拔掉
- **THEN** 设置页仍显示 A 和 Claude，A 标记为未连接；重新插上后继续显示 Claude

#### Scenario: 删除失效灯

- **WHEN** 用户点击某台未检测到灯的「删除这台灯的配置」
- **THEN** 该灯的 `manual` 条目和 `devices` 绑定被移除，其 Agent 变为未分配

### Requirement: 设备在线状态与宽限

设备只有检测到（`connected: true`）和检测不到（`connected: false`）两种状态，系统 MUST NOT 提供删除/停用设备。

- 短暂离线（≤ `MISSING_GRACE_SECS`，默认 60 秒）保留已有归属；
- 超过 grace 的 `auto` 设备退出自动分配集合，其 Agent/session 按策略重新分配；
- `manual` 模式保留离线灯的映射；灯不在设备列表时其 Agent 视为未分配；
- 设备恢复后重新加入，已有归属保持，新 session/Agent 才使用它；
- 设备连接/断开 MUST NOT 触发整体重排。

#### Scenario: 短暂离线

- **WHEN** 设备 A 被拔掉但在 grace 内
- **THEN** A 的归属不变，快照显示 `connected: false`

#### Scenario: 超过 grace

- **WHEN** 设备 A 持续未检测到超过 `MISSING_GRACE_SECS`，策略为 `auto_session`
- **THEN** A 退出自动分配集合，原本在 A 上的 session 迁移到其他在线灯，其他灯不动

#### Scenario: 设备恢复

- **WHEN** 长时间离线的设备 A 重新被检测到
- **THEN** A 重新加入分配集合，已有 session 不被搬走，新 session 可以使用 A

### Requirement: 聚合与空灯

每台设备的 session 集合 SHALL 为：

- `mirror`：全部活动 session；
- `auto`：分配表中属于该设备的 session；
- `manual`：`routing.manual` 分配给该设备的 Agent 的 session。

设备聚合 MUST 复用全局优先级：`error > waiting > working > idle`，同优先级取最近更新；空集合为 `none`，灯效为 `mapping.effect_for(none)`（默认 off）。设备聚合 MUST NOT 修改全局聚合或影响其他设备。

#### Scenario: 多 Agent 聚合到一台灯

- **WHEN** 设备 A 分到 Claude 的 `working` session 与 Codex 的 `error` session
- **THEN** A 的聚合为 `error`，灯效为 `alert`

#### Scenario: 空灯

- **WHEN** 设备 A 没有分到任何活动 session
- **THEN** A 显示 `mapping.none` 对应的灯效（默认 off）

### Requirement: 每设备目标状态

系统 SHALL 为每台已注册设备计算独立的 `DeviceDesired`（`effect`、`brightness`、`buzzer_on_alert`）。设备 actor MUST 只读取自身的 `DeviceDesired`。

亮度/蜂鸣解析顺序 SHALL 为：设备绑定覆盖 > 全局配置。服务退出关灯时，所有设备 MUST 收到 `off`，优先级高于策略与手动覆盖。

#### Scenario: 不同设备不同灯效

- **WHEN** 设备 A 分到 `working` session，设备 B 分到 `error` session
- **THEN** A/B 分别收到 `busy`/`alert` 命令

#### Scenario: 退出关灯

- **WHEN** 某设备正被手动覆盖为 `alert`，服务收到退出信号且 `shutdown.turn_off_lights = true`
- **THEN** 该设备收到 `off` 命令

### Requirement: 手动覆盖与策略

手动覆盖 SHALL 压过策略结果：全局覆盖作用于全部设备，单设备覆盖只作用于该设备；覆盖到期后设备 MUST 回到当前策略的结果。

#### Scenario: 单设备覆盖

- **WHEN** 用户只对设备 A 应用 `alert` 30 秒
- **THEN** A 显示 `alert`，其他设备继续按策略显示；30 秒后 A 回到策略结果

### Requirement: 可观测性

`GET /api/v1/state` 与 SSE 快照 SHALL 包含：

- `routing`：每台已配置或已发现的设备，至少包含 `device`、`label`、`assignment`、`agents`、`sessions`、`aggregate`、`auto_effect`、`effect`、`overridden`、`connected`、`last_seen_ms`；
- `unassigned_agents`：当前没有出现在任何设备 session 集合中的 Agent；
- `config.routing`：当前策略与手动映射。

`POST /api/v1/routing/rebuild` SHALL 清空粘性分配并按当前策略重新计算，不修改配置。

#### Scenario: 解释灯效来源

- **WHEN** 客户端请求 `GET /api/v1/state`
- **THEN** `routing` 中每台设备都能看到角色、分到的 Agent/session、聚合状态与最终灯效

#### Scenario: 重新分配

- **WHEN** 用户调用 `POST /api/v1/routing/rebuild`
- **THEN** 服务清空粘性分配并重新计算，快照更新，`GET /config` 不变

### Requirement: 配置校验

配置校验 SHALL 覆盖：

- `routing.strategy` 只能为 `unified` / `auto_agent` / `manual_agent` / `auto_session`；
- `routing.manual` 的设备 id 非空、Agent 非空；
- 同一个 Agent MUST NOT 出现在多台灯；
- 每设备亮度在 `0..=100`；
- 每个绑定至少提供 `id`/`serial`/`port` 之一；
- `label` 不超过 64 字符。

非法配置 MUST 返回 400，且 MUST NOT 写入磁盘或改变运行中的分配。

#### Scenario: 拒绝非法策略

- **WHEN** 配置提交 `"strategy": "split"`
- **THEN** 服务返回 400，运行中的策略与配置文件均不变

#### Scenario: 拒绝重复 Agent

- **WHEN** `manual` 把同一个 Agent 放进两台灯
- **THEN** 服务返回 400 并指出重复的 Agent

### Requirement: 不提供 session 手动绑定

配置、HTTP API 与控制台 MUST NOT 提供 session 级的固定、绑定或范围字段。session 的归属只能由自动策略或 Agent 手动映射间接决定。

#### Scenario: 没有 session 选择器

- **WHEN** 用户打开设置页的多灯控制策略
- **THEN** 只能选择四种策略或在手动模式里分配 Agent，不能为单个 session 选择灯

### Requirement: 不按状态动态重分配

系统 MUST NOT 因为 session 状态变化（`idle`/`working`/`waiting`/`error`）而改变 Agent/session 与设备的归属。归属只能因 session/Agent 新增或过期、策略或手动映射变化、设备集合变化、显式重新分配而改变。

#### Scenario: 错误状态不抢灯

- **WHEN** 设备 B 上的 Codex session 变为 `error`
- **THEN** 归属不变，只有 B 的灯效变为 `alert`
