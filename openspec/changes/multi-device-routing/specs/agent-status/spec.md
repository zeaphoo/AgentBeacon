# Spec Delta

## Purpose

在现有全局会话聚合之外，增加「按 session 集合聚合」与稳定的会话创建时间，供多设备自动分配使用。全局聚合仍然是状态卡片、`GET /api/v1/agents` 与 mirror 设备的输入。

## ADDED Requirements

### Requirement: 会话创建时间

`SessionView` SHALL 包含 `created_at_ms`，表示该 session 第一次被服务看到的时间。同一 `(agent, session)` 后续上报 MUST 保留原 `created_at_ms`，只更新 `updated_at_ms`、状态、消息与过期时间。

`created_at_ms` SHALL 用于自动分配的稳定排序，MUST NOT 因重复上报而改变。

#### Scenario: 重复上报保留创建时间

- **WHEN** `claude-code/7f3a` 在 t1 第一次上报，t2 再次上报
- **THEN** `created_at_ms` 仍为 t1，`updated_at_ms` 更新为 t2

#### Scenario: 会话过期后重建

- **WHEN** 同一 `(agent, session)` 过期后被移除，之后再次上报
- **THEN** `created_at_ms` 取新的上报时间

### Requirement: 按 session 集合聚合

系统 SHALL 支持对任意 `SessionView` 集合计算聚合状态。集合聚合 MUST 复用全局聚合的优先级规则：`error > waiting > working > idle`，同优先级取最近更新，空集合为 `none`。

会话集合 MUST 由分配层提供；聚合函数本身 MUST NOT 按 agent、设备或连接状态做额外过滤。

#### Scenario: 两台设备不同集合

- **WHEN** 设备 A 的集合为 `claude-code/working`，设备 B 的集合为 `codex/error`
- **THEN** A 的聚合为 `working`，B 的聚合为 `error`

#### Scenario: 同一集合多优先级

- **WHEN** 设备 A 的集合包含 `working` 与 `waiting`
- **THEN** A 的聚合为 `waiting`

#### Scenario: 空集合

- **WHEN** 设备 A 的集合为空
- **THEN** A 的聚合为 `none`，进入空分配回退

### Requirement: 全局聚合保持不变

`SessionRegistry::aggregate()`、`GET /api/v1/agents` 的 `aggregate` 字段与 `GET /api/v1/state` 顶层的 `aggregate`/`effect` SHALL 继续表示全部活动会话的聚合，不受任何设备分配影响。

#### Scenario: 多设备不改变全局卡片

- **WHEN** 三台设备分别显示不同的 session 集合
- **THEN** 状态页顶部的全局聚合仍按所有 session 计算
