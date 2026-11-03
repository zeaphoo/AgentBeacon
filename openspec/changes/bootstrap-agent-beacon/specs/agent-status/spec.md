# Spec Delta

## Purpose

定义各类 agent 如何上报状态、服务如何跟踪会话，并把多个 agent 聚合成单一「当前状态」，作为灯效决策的唯一输入。

## ADDED Requirements

### Requirement: HTTP 事件上报

系统 SHALL 接受 `POST /api/v1/events` 的 JSON 事件，字段为 `agent`（必填字符串）、`state`（必填，取值为 `idle`、`working`、`waiting`、`error` 之一）、`session`（可选）、`message`（可选）、`ttl_secs`（可选）。成功时返回 200 与更新后的会话及聚合状态。

#### Scenario: 接受合法事件

- **WHEN** 客户端提交 `{"agent":"claude-code","state":"working"}`
- **THEN** 服务返回 200，会话列表出现该 agent 的 `working` 会话，聚合状态按优先级更新

#### Scenario: 拒绝非法状态

- **WHEN** 客户端提交 `state` 为 `busy` 的事件
- **THEN** 服务返回 400 并说明合法取值，已存在的会话与聚合状态保持不变

### Requirement: 会话标识与默认会话

会话由 `agent` 与 `session` 共同标识。未提供 `session` 时，服务 SHALL 使用 `{agent}:default` 作为会话标识，使同一 agent 的重复上报更新同一会话。

#### Scenario: 缺省会话复用

- **WHEN** 同一 agent 先后两次不带 `session` 上报事件
- **THEN** 会话列表中该 agent 只有一条会话，状态为最后一次上报的值

#### Scenario: 多会话隔离

- **WHEN** 同一 agent 以 `session=a` 和 `session=b` 分别上报
- **THEN** 会话列表中同时存在两条独立会话，各自维护状态与时间戳

### Requirement: 会话过期

每个会话 SHALL 具有存活时限，默认 900 秒，可由事件的 `ttl_secs` 覆盖（允许范围 30–86400 秒）。超过时限未收到新事件的会话 MUST 从活动会话与聚合计算中移除。

#### Scenario: 超时会话降级

- **WHEN** `session=a` 处于 `working` 且超过其 TTL 没有新事件
- **THEN** 该会话从活动会话列表移除，聚合状态按剩余会话重新计算；若没有剩余会话则变为 `none`

#### Scenario: 重复事件刷新时限

- **WHEN** 会话在上报后未超时前再次上报相同状态
- **THEN** 该会话的时限被刷新，聚合状态不变

### Requirement: 聚合优先级

聚合状态 SHALL 取所有活动会话中优先级最高者，优先级从高到低为 `error` > `waiting` > `working` > `idle`；没有活动会话时为 `none`。同优先级时以最近更新的事件为准。

#### Scenario: 错误优先

- **WHEN** 一个会话为 `working`，另一个为 `error`
- **THEN** 聚合状态为 `error`

#### Scenario: 无会话

- **WHEN** 不存在任何活动会话
- **THEN** 聚合状态为 `none`，灯效映射为 `off`

### Requirement: 状态快照查询

系统 SHALL 提供 `GET /api/v1/agents`，返回活动会话列表与聚合状态；每条会话包含 agent、session、state、message、更新时间与过期时间。该接口 SHALL 在 200 毫秒内返回。

#### Scenario: 查询快照

- **WHEN** 客户端请求 `GET /api/v1/agents`
- **THEN** 返回 `{"aggregate":"working","sessions":[...]}`，字段与当前状态一致

### Requirement: 清空会话

系统 SHALL 提供 `DELETE /api/v1/sessions`，在一次原子操作中清空全部会话，随后聚合状态变为 `none`。

#### Scenario: 手动清零

- **WHEN** 客户端请求 `DELETE /api/v1/sessions`
- **THEN** 所有会话被移除，聚合状态为 `none`，正在执行的灯效按 `none` 重新映射

### Requirement: CLI 上报

`agent-beacon report` SHALL 以 `--agent`、`--state` 必填，`--session`、`--message`、`--ttl`、`--url` 可选的方式上报事件；成功时退出码为 0，服务不可达时必须在 1 秒内失败、向 stderr 输出原因并返回非 0 退出码。

#### Scenario: hook 中调用

- **WHEN** 执行 `agent-beacon report --agent codex --state waiting`
- **THEN** 服务收到事件并更新聚合状态，命令退出码为 0

#### Scenario: 服务未启动

- **WHEN** 本地服务不可达时执行 `agent-beacon report --state working`
- **THEN** 命令在 1 秒内以非 0 退出码结束，stderr 提示无法连接及配置的服务地址
