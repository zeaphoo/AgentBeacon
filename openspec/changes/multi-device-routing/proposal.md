# Proposal

## Why

当前实现已经能同时发现并驱动多台状态灯，但所有灯共享同一个聚合状态：`SessionRegistry::aggregate()` 把全部 agent 会话压成一个 `Aggregate`，再映射成唯一 `Effect` 下发给每台设备。只有一台灯时这是正确的；桌面上出现第二台灯以后，用户无法回答「哪台灯代表哪个 agent」，多设备只剩重复显示同一状态的用途。

最初考虑过「每台灯绑定一个 session」，但 session 不适合作为配置单位：

- session id 经常是随机 UUID，agent 重启、开新窗口、切换任务都会变化；
- 同一 agent 的会话数量不固定，用户无法提前为「未来的会话」绑定灯；
- 物理灯的数量通常少于 session 数量，用户无法为每个 session 预留一盏灯；即使灯足够，session 也会不断新建和过期，手动绑定很快就会过期。

更稳定的手动单位是 **agent**；自动分配的粒度则有两种选择：按 Agent 或按 session。因此本变更把多灯策略收敛为四个全局单选：

- **统一显示**：所有灯显示同一个全局状态（默认，兼容现有行为）；
- **按 Agent 自动分配**：每个 Agent 自动分到一台灯，一台灯可以显示多个 Agent；
- **按 Agent 手动分配**：用户手动把 Agent 分到灯；只有这个策略需要详细配置；
- **按 Session 自动分配**：同一个 Agent 的多个 session 自动分散到不同灯。

多灯策略放在**设置页的独立卡片**，不改变灯具页原有的手动灯效与设备型号管理。session 不提供手动绑定；离线灯会保留已有的 Agent 配置，用户可以在设置页删除已经不在的灯。

## What Changes

- 新增 `RoutingStrategy`：`unified` / `auto_agent` / `manual_agent` / `auto_session`。
- 新增 `routing.strategy` 与 `routing.manual`（设备 id → Agent 列表）；删除逐设备的 `assignment` / `pins` / `device_modes` / 空分配回退。
- 实现四种分配算法：
  - `unified`：所有设备 mirror 全局聚合；
  - `auto_agent`：Agent 级最少负载 + 粘性，一个 Agent 一台灯；
  - `auto_session`：session 级最少负载 + 粘性，一个 Agent 的多个 session 可以分散；
  - `manual_agent`：按 `routing.manual` 把 Agent 分到灯。
- 设置页新增「多灯控制策略」卡片：四策略单选；只有手动模式展开灯/Agent 编辑。
- 手动模式列出已检测、离线和配置已失效的灯；未检测到的灯保留配置并可删除。
- 保留设备在线宽限（60 秒）与孤儿迁移；没有删除/停用设备操作。
- 改造期望状态：从全局 effect 改为每设备 `DeviceDesired`。
- 扩展快照与 SSE：`routing`、`unassigned_agents`、`config.routing`；新增 `POST /api/v1/routing/rebuild`。
- 保持手动覆盖语义：全局覆盖压过策略结果，单设备覆盖只压过该设备。

## Capabilities

### New Capabilities

- `device-routing`: 四种全局策略、Agent/session 自动分配、按灯手动 Agent 映射、离线灯保留/删除、每设备目标状态与分配可观测性。

### Modified Capabilities

- `agent-status`: 新增「按 session 集合聚合」与 `created_at_ms`，与现有全局聚合并存；全局聚合仍是 `GET /api/v1/agents` 与状态卡片的输入。
- `light-control`: 设备 actor 读取每设备目标状态；手动覆盖说明扩展到策略结果；设备查询增加标签与最后连接时间；支持每设备亮度/蜂鸣覆盖。
- `beacon-service`: 状态快照增加 `routing`、`unassigned_agents` 与 `config.routing`；配置校验覆盖策略与手动映射；配置版本保持向后兼容。
- `web-console`: 设置页新增独立的多灯控制策略卡片；灯具页保持原样；不提供 session 绑定。

## Impact

- **代码**：`agent-beacon-core`（`RoutingStrategy`、配置、按 session 集合聚合、`created_at_ms`）、`agent-beacon`（四种分配算法、`BeaconState`、快照、API）、`agent-beacon-lights`（`DesiredState`/`DeviceDesired`、设备 actor）、`web/`（设置页策略卡片）。
- **配置**：只增字段、不改既有字段语义，`version` 保持 `1`；旧配置缺少新字段时按默认值加载，所有设备继续跟随全局聚合。
- **API**：`GET /api/v1/state`、`GET /api/v1/stream`、`GET /api/v1/devices` 的响应增加字段；旧客户端可忽略未知字段。`PUT /api/v1/config` 接受新增字段。
- **兼容性**：默认行为不变；不涉及设备发现、传输协议、桌面壳与打包流程。
- **不在本变更内**：按 session 手动绑定、按状态优先级动态换灯、按设备权重/容量平衡、单台三灯珠设备的「每个灯珠一个 Agent」、灯组/区域、跨机器/远程控制。
