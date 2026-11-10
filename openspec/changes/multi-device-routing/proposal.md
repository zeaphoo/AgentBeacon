# Proposal

## Why

当前实现已经能同时发现并驱动多台状态灯，但所有灯共享同一个聚合状态：`SessionRegistry::aggregate()` 把全部 agent 会话压成一个 `Aggregate`，再映射成唯一 `Effect` 下发给每台设备。只有一台灯时这是正确的；桌面上出现第二台灯以后，用户无法回答「哪台灯代表哪个 agent」，多设备只剩重复显示同一状态的用途。

最初考虑过「每台灯绑定一个 session」，但 session 不适合作为配置单位：

- session id 经常是随机 UUID，agent 重启、开新窗口、切换任务都会变化；
- 同一 agent 的会话数量不固定，用户无法提前为「未来的会话」绑定灯；
- 物理灯的数量通常少于 session 数量，用户无法为每个 session 预留一盏灯；即使灯足够，session 也会不断新建和过期，手动绑定很快就会过期。

更稳定的手动单位是 **agent**：用户关心的是「Claude 现在什么状态」「Codex 现在什么状态」，而不是某个具体 session。但自动分配不能只按 agent 做：如果只有 1 个 agent、3 个 session、3 台灯，按 agent 分只能让一台灯亮。因此本变更采用：

- **手动单位是 agent**：可以把某个 agent 固定到某台灯；固定后它的所有 session 都去那台灯；
- **自动单位是 session**：没有被固定的 session 由算法逐条分配，同一 agent 的多个 session 可以分散到不同灯；
- 当 session 数量多于灯时，**一台灯聚合多个 session，可能来自多个 agent**；
- 默认按确定性算法自动分配，同时允许 agent 固定；
- 默认行为仍是「所有灯镜像全局聚合」，未开启自动分配时完全不变。

## What Changes

- 新增设备分配模型 `DeviceAssignment`：`mirror`（镜像全局，默认）、`auto`（参与自动分配）与 `manual`（只接收固定 agent）。
- 新增确定性分配算法：活动 session 按创建时间排序，未被固定的 session 依次分配到当前 session 数最少的 `auto` 设备；已有分配保持粘性，新增 session 不会挤走已有 session。
- 新增 `routing.pins`：可把某个 agent 固定到某台 `auto` 或 `manual` 设备；固定后该 agent 的全部 session 都去目标设备，其余 session 继续自动分配。
- 删除「按 session 绑定」的设计：不提供 `SessionScope::Session`，控制台不出现 session 选择器。
- 新增 `routing.empty_fallback`：设备没有可分到的 session 时的回退方式。
- 改造期望状态：从「一个全局灯效 + 按设备覆盖」改为「每台设备一份 `DeviceDesired`（灯效、亮度、蜂鸣）+ 新设备兜底」。
- 扩展快照与 SSE：新增 `routing`（每台设备的分配方式、分到的 agent/session、聚合状态、最终灯效、固定/回退信息）、`unassigned_agents` 与 `unresolved_pins`。
- 扩展控制台：状态页展示每台灯分到的 agent，灯具页提供「镜像 / 自动分配 / 仅手动固定」开关与 Agent 固定面板，设置页配置空分配回退。
- 保持手动覆盖语义：全局覆盖压过所有分配结果，单设备覆盖只压过该设备。

## Capabilities

### New Capabilities

- `device-routing`: session 到设备的自动分配、agent 固定（pins）、mirror/auto/manual 设备模式、空分配回退、每设备目标状态与分配可观测性。

### Modified Capabilities

- `agent-status`: 新增「按 session 集合聚合」与 `created_at_ms`，与现有全局聚合并存；全局聚合仍是 `GET /api/v1/agents` 与状态卡片的输入。
- `light-control`: 设备 actor 读取每设备目标状态；手动覆盖说明扩展到分配结果；设备查询增加标签与分配视图；支持每设备亮度/蜂鸣覆盖。
- `beacon-service`: 状态快照增加 `routing` 与 `unassigned_agents`；配置校验覆盖分配字段；配置版本保持向后兼容。
- `web-console`: 状态页、灯具页、设置页增加 agent 分配相关展示与编辑，不提供 session 绑定。

## Impact

- **代码**：`agent-beacon-core`（`DeviceAssignment`、配置、按 session 集合聚合、`created_at_ms`）、`agent-beacon`（最少会话优先分配、pins 解析、`BeaconState`、快照、API）、`agent-beacon-lights`（`DesiredState`/`DeviceDesired`、设备 actor）、`web/`（状态、灯具、设置三页）。
- **配置**：只增字段、不改既有字段语义，`version` 保持 `1`；旧配置缺少新字段时按默认值加载，所有设备继续跟随全局聚合。
- **API**：`GET /api/v1/state`、`GET /api/v1/stream`、`GET /api/v1/devices` 的响应增加字段；旧客户端可忽略未知字段。`PUT /api/v1/config` 接受新增字段。
- **兼容性**：默认行为不变；不涉及设备发现、传输协议、桌面壳与打包流程。
- **不在本变更内**：按 session 绑定、按状态优先级动态换灯、按设备权重/容量平衡、单台三灯珠设备的「每个灯珠一个 agent」、灯组/区域、跨机器/远程控制。
