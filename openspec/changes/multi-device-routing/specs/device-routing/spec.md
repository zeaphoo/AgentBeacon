# Spec Delta

## Purpose

把活动 session 自动分配到多台状态灯：手动单位是 agent，自动单位是 session。每台灯聚合自己分到的 session，驱动自己的设备 actor；不向用户暴露 session 绑定。

## ADDED Requirements

### Requirement: 设备分配方式

每台设备 SHALL 具有分配方式 `assignment`，取值为 `mirror`、`auto` 或 `manual`：

- `mirror`：跟随全部会话，显示全局聚合（默认）；
- `auto`：参与自动分配，显示算法分到的 session，也可接收 pin 到它的固定 agent；
- `manual`：只接收 pin 到它的固定 agent，不参与自动分配、不接收自动 session。

缺少 `assignment` 的旧配置 MUST 按 `mirror` 加载，所有设备继续镜像全局聚合。三种设备 SHALL 允许同时存在。

#### Scenario: 旧配置升级

- **WHEN** 服务加载一份 `devices` 中没有 `assignment` 字段的旧配置
- **THEN** 所有设备按 `mirror` 运行，灯效与升级前相同

#### Scenario: 混合模式

- **WHEN** 一台设备 `assignment: mirror`，一台 `auto`，一台 `manual` 并 pin 了 Codex
- **THEN** mirror 设备显示全局聚合，auto 设备按自动分配显示，manual 设备只显示 Codex

### Requirement: 自动分配算法

系统 SHALL 对活动 session 执行确定性自动分配：

1. 活动 session 按 `(created_at_ms, agent, session)` 升序排序；
2. 被固定 agent 的全部 session 先强制分配到 pin 目标设备；
3. 其余 session 按顺序选择当前自动分配集合中已分配 session 数最少的 `auto` 设备（负载只统计分配到 `auto` 设备的 session）；
4. 数量相同时按设备 id 升序决胜；
5. 已有有效分配的 session MUST 保持原设备，只有新 session、失去设备的孤儿 session、或显式重新分配时才重新选灯。

算法 MUST 不依赖 session 状态的优先级，MUST NOT 在状态变化时重新分配。

#### Scenario: 三个 session 三台灯

- **WHEN** 1 个 agent 有 3 个活动 session，存在 3 台 `auto` 设备
- **THEN** 每台设备各分到 1 个 session

#### Scenario: 四个 session 三台灯

- **WHEN** 4 个活动 session，存在 3 台 `auto` 设备
- **THEN** 三台设备分别分到 2、1、1 个 session

#### Scenario: 新增 session 不移动已有

- **WHEN** 已有 3 个 session 分别落在 A/B/C，新增第 4 个 session
- **THEN** A/B/C 的归属不变，第 4 个 session 进入当前 session 数最少的设备

#### Scenario: session 过期只移除自己

- **WHEN** 灯 A 上的一个 session 过期
- **THEN** 只移除该 session 的分配，其他灯的 session 归属不变

#### Scenario: 状态变化不换灯

- **WHEN** 一个 session 从 `working` 变为 `error`
- **THEN** 它仍留在原设备，只有该设备的聚合灯效变化

### Requirement: agent 固定

系统 SHALL 支持 `routing.pins`，把 agent 映射到设备 id。被固定 agent 的全部 session MUST 分配到目标设备，无论目标设备当前负载如何；同一台 `auto` 设备 MAY 接收多个固定 agent 和自动分配的 session，`manual` 设备只接收固定 agent。

pin 目标 MUST 是 `assignment: auto` 或 `assignment: manual` 的设备。设备可以暂时离线；超过 `MISSING_GRACE_SECS` 仍未检测到、目标不存在或为 `mirror` 时，pin 视为未解析：该 agent 的 session 回到自动池，服务 SHALL 在快照的 `unresolved_pins` 中报告。

固定到 `auto` 设备时，固定 session 计入该设备的负载；固定到 `manual` 设备时，该设备不参与自动分配，只显示自己的固定 agent。

系统 MUST NOT 提供 session 级固定或绑定。

#### Scenario: 固定 agent

- **WHEN** `routing.pins` 为 `{"claude-code":"serial:A"}`，Claude 有 3 个 session
- **THEN** 3 个 session 全部落在设备 A，A 聚合它们的状态

#### Scenario: 取消固定

- **WHEN** 用户删除 Claude 的 pin
- **THEN** Claude 的 session 进入自动池，按最少会话优先重新分散到各 auto 设备

#### Scenario: 固定目标不可用

- **WHEN** pin 指向的设备不存在、超过 grace 未检测到或为 `mirror`
- **THEN** 该 agent 的 session 参与自动分配，`unresolved_pins` 列出该 pin

#### Scenario: 固定多个 agent 到同一台灯

- **WHEN** Claude 与 Codex 都 pin 到设备 A
- **THEN** A 同时聚合两个 agent 的 session，其他 auto 设备只接收未固定 session

#### Scenario: 固定到手动专用灯

- **WHEN** 设备 B 的 `assignment: manual`，Claude pin 到 B，Codex 未固定
- **THEN** B 只显示 Claude 的 session，Codex 的 session 由 auto 设备接收；如果没有任何 auto 设备，Codex 的 agent 出现在 `unassigned_agents`

### Requirement: 设备集合与粘性分配

参与自动分配的设备集合 SHALL 包含：

- `assignment: auto` 且 `connected: true` 的设备；
- `assignment: auto`、`connected: false`，但 `last_seen_ms` 在 `MISSING_GRACE_SECS` 内的设备；
- 从未连接过的 `auto` 设备 MUST NOT 进入集合；
- 超过 grace 未连接的 `auto` 设备 MUST NOT 进入集合。

`manual` 设备不参与自动分配，只接收 pin 到它的 session。

系统 MUST 维护粘性分配：已有有效归属的 session MUST NOT 因为设备增加、pin 变化、session 增删或状态变化而移动，除非它成为孤儿或用户显式请求重新分配。短暂离线 MUST NOT 改变已有归属。

#### Scenario: 短暂离线保留归属

- **WHEN** 设备 A 被拔掉，但仍在 `MISSING_GRACE_SECS` 内且 `assignment: auto`
- **THEN** 原本分到 A 的 session 不会被转移，A 在快照中显示 `connected: false`

#### Scenario: 超过 grace 后迁移

- **WHEN** 设备 A 持续未检测到超过 `MISSING_GRACE_SECS`
- **THEN** A 退出自动分配集合，原本分到 A 的 session 成为孤儿并迁移到剩余在线 auto 设备

#### Scenario: 从未连接的设备不参与

- **WHEN** 配置里有一台 `assignment: auto` 但从未连接过的设备
- **THEN** 它不进入自动分配集合，session 不会被分配到它上面

#### Scenario: 新增 session 不移动已有

- **WHEN** A/B 各有 1 个 session，新增第 3 个 session
- **THEN** A/B 的 session 保持不动，新 session 进入当前负载较少的设备

#### Scenario: 状态变化不换灯

- **WHEN** 一个 session 从 `working` 变为 `error`
- **THEN** 它仍留在原设备，只有该设备的聚合灯效变化

### Requirement: 设备减少与孤儿迁移

设备超过 `MISSING_GRACE_SECS` 未检测到、`auto`→`mirror` 或 `auto`→`manual` 时，系统 SHALL 只迁移因此失去有效归属的 session；其他 session MUST 保持原设备。

- 设备超过 grace 未检测到/`auto`→`mirror`：该设备上的自动 session，以及 pin 到它但目标不再有效的 session 变为孤儿并重新分配。
- `auto`→`manual`：该设备上的自动 session 变为孤儿并重新分配；pin 到它的 session MUST 保留。
- `manual`→`auto`：设备加入自动池；pin 到它的 session MUST 保留。

#### Scenario: 设备长时间离线只迁移孤儿

- **WHEN** 3 台 `auto` 灯 A/B/C，C 上有 2 个 session，A/B 各有 1 个，C 超过 grace 未检测到
- **THEN** C 的 2 个 session 迁移到 A/B 中当前负载较少的设备，A/B 原有 session 保持不动

#### Scenario: 设备恢复后重新加入

- **WHEN** 长时间离线的设备 C 重新被检测到
- **THEN** C 重新加入自动分配集合，已有 session 保持原归属，新 session 可以使用 C

#### Scenario: auto 改为 manual

- **WHEN** 设备 C 从 `auto` 改为 `manual`，C 上有 1 个自动 session 和 1 个 pin 到它的 session
- **THEN** 自动 session 迁移到 A/B，pin 到 C 的 session 保留在 C，A/B 其他 session 不动

### Requirement: 设备增加与 pin 变化

设备首次被检测到、设备恢复在线，或 `routing.pins` 变化时，系统 SHALL 只影响相关 session；其他 session MUST 保持原设备。

- 设备首次检测/恢复在线：已有 session MUST 保持原设备；新 session MAY 使用新 `auto` 设备；系统 MUST NOT 自动搬走旧 session。
- pin 新增/改目标：该 agent 的 session SHALL 迁移到新目标。
- pin 删除或目标失效：该 agent 的 session SHALL 进入自动池。

#### Scenario: 设备恢复不搬旧 session

- **WHEN** A/B 各有 1 个 session，离线设备 C 重新被检测到
- **THEN** A/B 的 session 保持不动，下一个新 session 优先落到 C

#### Scenario: pin 删除只影响该 agent

- **WHEN** Claude 从固定 A 改为自动，Codex 的 session 仍在 B
- **THEN** Claude 的 session 进入自动池重新分配，Codex 的 session 保持不动

### Requirement: 重新分配

服务启动时分配表为空，系统 SHALL 按 `created_at_ms` 顺序全量分配。用户显式请求重新分配时，系统 SHALL 清空分配表并全量重算，不修改配置。

#### Scenario: 显式重新分配

- **WHEN** 用户调用 `POST /api/v1/routing/rebuild`
- **THEN** 服务清空粘性分配并全量重算，不修改配置

#### Scenario: 启动后首次分配

- **WHEN** 服务重启后收到 3 个 session
- **THEN** 按 `created_at_ms` 顺序全量分配，不依赖上一次运行的内存状态

### Requirement: 聚合与回退

每台设备的 session 集合 SHALL 为：

- `mirror`：全部活动 session；
- `auto`：分配表中属于该设备的 session；
- `manual`：只包含 pin 到该设备的 agent 的 session。

设备聚合 MUST 复用全局优先级：`error > waiting > working > idle`，同优先级取最近更新；空集合为 `none`。

空集合时，系统 SHALL 按 `routing.empty_fallback` 决定灯效：

- `none`（默认）：使用 `mapping.effect_for(none)`；
- `global`：跟随全局聚合；
- `off`：强制 `off`，不受 `mapping.none` 影响。

设备聚合 MUST NOT 修改全局聚合或影响其他设备。

#### Scenario: 多 agent 聚合到一台灯

- **WHEN** 设备 A 分到 Claude 的 `working` session 与 Codex 的 `error` session
- **THEN** A 的聚合状态为 `error`，灯效为 `alert`

#### Scenario: 空分配默认熄灭

- **WHEN** 设备 A 没有分到任何活动 session，`empty_fallback = none`，`mapping.none = off`
- **THEN** A 显示 `off`

#### Scenario: 空分配跟随全局

- **WHEN** 设备 A 没有 session，但全局聚合为 `error`，`empty_fallback = global`
- **THEN** A 显示全局 `alert`

### Requirement: 每设备目标状态

系统 SHALL 为每台已注册设备计算独立的 `DeviceDesired`，包含 `effect`、`brightness` 与 `buzzer_on_alert`。设备 actor MUST 只读取自身的 `DeviceDesired`，不得读取其他设备的灯效或重新计算分配。

亮度/蜂鸣解析顺序 SHALL 为：设备绑定覆盖 > 全局配置。profile 不支持该能力时 MUST 忽略对应设置。服务退出关灯时，所有设备 MUST 收到 `off`，优先级高于分配与手动覆盖。

#### Scenario: 不同设备不同灯效

- **WHEN** 设备 A 分到 `working` session，设备 B 分到 `error` session
- **THEN** A/B 分别收到 `busy`/`alert` 命令，互不等待

#### Scenario: 每设备蜂鸣

- **WHEN** 全局 `buzzer_on_alert = true`，设备 B 的绑定设置 `buzzer_on_alert: false`
- **THEN** A 在 `alert` 时蜂鸣，B 不蜂鸣

#### Scenario: 退出关灯

- **WHEN** 某设备正被手动覆盖为 `alert`，服务收到退出信号且 `shutdown.turn_off_lights = true`
- **THEN** 该设备收到 `off` 命令

### Requirement: 手动覆盖与分配

手动覆盖 SHALL 压过自动分配结果与空分配回退：

- 未指定 `device` 的覆盖作用于全部设备；
- 指定 `device` 的覆盖只作用于该设备；
- 覆盖到期后，设备 MUST 回到自己的分配结果，而不是全局聚合结果。

#### Scenario: 单设备覆盖

- **WHEN** 用户只对设备 A 应用 `alert` 30 秒
- **THEN** A 显示 `alert`，其他设备继续按分配结果；30 秒后 A 回到自己的分配结果

### Requirement: 分配可观测性

`GET /api/v1/state` 与 SSE 快照 SHALL 包含 `routing` 数组，每项至少包含：`device`、`label`、`assignment`、`agents`、`pinned_agents`、`sessions`、`aggregate`、`auto_effect`、`effect`、`overridden`、`fallback`、`connected`、`last_seen_ms`。

快照 SHALL 包含：

- `unassigned_agents`：没有出现在任何设备 session 集合中的 agent；存在 `mirror` 设备时该数组为空（所有 agent 都可被 mirror 设备显示）；
- `unresolved_pins`：目标不存在、超过 grace 未检测到或为 `mirror` 的 pin，每项包含 `agent` 与 `device`。

`GET /api/v1/devices` 的既有字段 MUST 保持不变；路由信息通过状态快照提供。

#### Scenario: 解释灯效来源

- **WHEN** 客户端请求 `GET /api/v1/state`
- **THEN** `routing` 中每台设备都能看到分配方式、分到的 agent/session、聚合状态与最终灯效

#### Scenario: 发现未分配 agent

- **WHEN** 没有任何设备，但存在活动 session
- **THEN** `unassigned_agents` 列出这些 session 的 agent

#### Scenario: mirror 覆盖所有 agent

- **WHEN** 至少一台设备 `assignment: mirror`
- **THEN** `unassigned_agents` 为空数组

#### Scenario: 发现无效固定

- **WHEN** pin 指向的设备超过 grace 未检测到
- **THEN** `unresolved_pins` 包含该 `agent` 与目标 `device`

### Requirement: 配置校验

配置校验 SHALL 覆盖：`assignment` 只能为 `mirror`/`auto`/`manual`、pin 的 agent 与目标设备 id 非空、每设备亮度在 `0..=100`、每个绑定至少提供 `id`/`serial`/`port` 之一、`label` 长度不超过 64 字符。非法配置 MUST 返回 400，且 MUST NOT 写入磁盘或改变运行中的分配。

#### Scenario: 拒绝非法分配方式

- **WHEN** 配置提交 `"assignment": "split"`
- **THEN** 服务返回 400，运行中的设备与配置文件均不变

#### Scenario: 拒绝空 pin

- **WHEN** 配置提交 `{"pins":{"":"serial:A"}}`
- **THEN** 服务返回 400 并提示 agent 不能为空

### Requirement: 不提供 session 手动绑定

配置模型、HTTP API 与控制台 MUST NOT 提供 session 级的固定、绑定或范围字段。session 的归属只能由自动算法决定；手动控制只能作用于 agent。

#### Scenario: 没有 session 选择器

- **WHEN** 用户打开灯具页的 Agent 分配面板
- **THEN** 只能选择「自动 / 固定到设备」，不能为单个 session 选择灯

#### Scenario: session 归属自动变化

- **WHEN** 同一 agent 新增一个 session
- **THEN** 新 session 由算法分配，用户不需要也不可以手动指定它的灯

### Requirement: 不按状态动态重分配

系统 MUST NOT 因为 session 状态变化（`idle`/`working`/`waiting`/`error`）而改变 session 与设备的归属关系。归属只能因 session 新增/过期、分配方式或 pin 变化、设备集合变化、显式重新分配而改变。

#### Scenario: 错误状态不抢灯

- **WHEN** 设备 B 上的 Codex session 变为 `error`，设备 A 上的 Claude session 仍为 `working`
- **THEN** 两个 session 的归属不变，只有 B 的灯效变为 `alert`
