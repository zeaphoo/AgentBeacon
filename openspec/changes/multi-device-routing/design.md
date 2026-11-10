# Design

## Context

现状（`multi-device-routing` 变更前）：

- `SessionRegistry::aggregate()` 把全部活动会话压成一个 `Aggregate`（`error > waiting > working > idle`），空集合为 `none`。
- `Mapping` 把 `Aggregate` 映射成唯一 `Effect`，`BeaconState::refresh_desired()` 把它写进 `DesiredState.effect`。
- 每台设备的 actor 读取同一个 `DesiredState`，只有手动覆盖能按设备 id 写入 `overrides`。
- 发现层与 actor 层**已经支持 N 台设备**（`worker.rs` 用 `HashMap<String, ActorEntry>` 管理，每设备一个 tokio 任务），缺的不是「发现更多设备」，而是「让不同设备表达不同语义」。

多设备的核心难点不是「怎么连更多灯」，而是「哪些状态归哪台灯」。本变更把这部分拆成两层：

1. **分配层**：把活动会话分配到灯。分配单位是 session，约束/手动单位是 agent。
2. **执行层**：每台灯把自己的会话集合聚合成一个灯效，驱动自己的设备 actor。

## Goals / Non-goals

**Goals:**

- 默认行为完全不变：未开启自动分配时，所有灯继续镜像全局聚合。
- 以 agent 为手动单位：可以把某个 agent 固定到某台灯；不提供 session 手动绑定。
- 以 session 为自动单位：同一 agent 的多个 session 可以自动分散到多台灯。
- agent 数量多于灯时，一台灯可以聚合多个 agent 的多个 session。
- 分配确定性、可解释、尽量稳定：新增 session 不移动已有 session；设备减少只迁移孤儿 session；新增设备不自动搬走旧 session。
- 快照能解释「这台灯为什么是这个颜色」：分到哪些 agent/session、聚合状态、最终灯效、是否固定/被覆盖/回退。
- 旧配置、旧 API 客户端继续可用；新字段只增不改。

**Non-goals:**

- 不做 session 手动绑定。session 生命周期短、id 不稳定，暴露给用户只会增加维护成本。
- 不做「按状态优先级换灯」：状态在 `working`/`error` 之间变化时，灯的主人不能跟着变，否则颜色失去可解释性。
- 不做按设备权重/容量/温度的复杂调度；v1 用「最少会话优先」这一条规则。
- 不做单台三灯珠设备的「每颗灯珠一个 session」；需要独立的通道编排模型，留待后续 `channel-routing`。
- 不做灯组/区域（zone）、跨机器同步。
- 不改设备发现规则、传输协议或桌面 App 生命周期。

## 多设备的语义

| 模式 | 每台灯显示什么 | 适用 | 本变更 |
| --- | --- | --- | --- |
| 镜像（mirror） | 所有灯显示同一个全局聚合 | 单灯、只想看一眼 | **默认保留** |
| 自动分配（auto） | 活动 session 自动分到各台 auto 灯；每台灯聚合自己分到的 session | 多灯、agent/session 数量不固定 | **新增** |
| agent 固定（pin） | 某个 agent 的全部 session 强制放到指定 auto/manual 灯 | 「Claude 必须在左边这盏」 | **新增** |
| 动态按状态换灯 | 按 `error > waiting > working` 重新分配 | 无 | 不做 |

结论：**默认镜像 + 可选自动分配 + agent 级固定**。用户不需要知道 session 的存在，也不需要为每个 session 选灯。

## Decisions

### D1. 四种全局策略

多灯策略是**全局单选**，不按设备分别配置：

| 策略 | 行为 | 适用 |
| --- | --- | --- |
| `unified`（统一显示） | 所有灯显示同一个全局聚合 | 单灯、只想看一眼 |
| `auto_agent`（按 Agent 自动分配） | 每个 Agent 自动分到一台灯；Agent 多于灯时一台灯显示多个 Agent | Agent 数量不稳定，想按 Agent 区分 |
| `manual_agent`（按 Agent 手动分配） | 用户手动把 Agent 分到灯 | 「Claude 必须在左边这盏」 |
| `auto_session`（按 Session 自动分配） | 同一 Agent 的多个 session 自动分散到不同灯 | 一个 Agent 开多个会话，想用满多盏灯 |

选择策略是用户在「设置 → 多灯控制策略」里唯一必须做的操作。只有 `manual_agent` 需要展开详细配置。

### D2. 策略决定设备角色

服务内部仍然给每台设备推导一个角色，但它由全局策略决定，不写进配置：

```rust
match strategy {
    Unified => DeviceAssignment::Mirror,
    AutoAgent | AutoSession => DeviceAssignment::Auto,
    ManualAgent => DeviceAssignment::Manual,
}
```

- `Mirror`：显示全部活动 session。
- `Auto`：参与自动分配（具体按 Agent 还是按 session 由策略决定）。
- `Manual`：只显示 `routing.manual` 里分配给它的 Agent。

### D3. `auto_agent`：Agent 级最少负载 + 粘性

- 活动 Agent 按名字排序。
- 已有有效归属的 Agent 保持不动；失去设备的 Agent 重新选择。
- 新 Agent 选择**当前 Agent 数最少**的可用灯，数量相同按设备 id 升序。
- 一个 Agent 的所有 session 都去同一台灯；一台灯可以有多个 Agent。
- 新增 session 不移动已有 Agent；session 过期只影响该 Agent 的聚合。

### D4. `auto_session`：Session 级最少负载 + 粘性

- 活动 session 按 `(created_at_ms, agent, session)` 排序。
- 已有有效归属的 session 保持不动。
- 新 session 选择**当前 session 数最少**的可用灯，数量相同按设备 id 升序。
- 同一个 Agent 的多个 session 可以落在不同灯；一台灯可以混合多个 Agent 的 session。
- 这是为了处理「只有一个 Agent、但有多个 session、又有多台灯」的场景。

### D5. `manual_agent`：按灯配置 Agent

配置结构：

```rust
pub struct RoutingConfig {
    pub strategy: RoutingStrategy,
    /// 设备 id -> Agent 列表；保留离线/已失效灯的配置。
    pub manual: HashMap<String, Vec<String>>,
}
```

- 设置页列出所有灯：已检测到的灯 + 配置中的离线灯 + `manual` 里保留但当前不存在的灯。
- 每台灯可以添加/移除 Agent；同一个 Agent 只能分到一台灯。
- 未检测到的灯保留其配置并标记；用户可以显式删除该灯（同时删除对应的 `devices` 绑定）。
- 不存在于设备列表里的 `manual` 条目仍保留在配置和 UI 中，但其 Agent 视为未分配，直到灯重新出现。
- 空灯（没有分到任何 session）显示 `mapping.none`，默认熄灭；不再单独配置空分配回退。

### D6. 在线状态与孤儿迁移

设备只有**检测到**和**检测不到**两种状态，没有删除/停用设备。

- `connected: true`：actor 已打开传输。
- `connected: false`：打不开或失去连接；记录 `last_seen_ms`（断开时间）。
- 短暂离线（≤ `MISSING_GRACE_SECS`，默认 60 秒）保留归属，避免拔插抖动导致重排。
- 超过 grace：
  - `auto_agent` / `auto_session`：只把该设备上的 Agent/session 变为孤儿并重新分配，其他设备不动。
  - `manual_agent`：该灯的配置保留，Agent 仍属于它；如果灯已不在设备列表里，这些 Agent 视为未分配，并在 UI 中可删除。
- 设备恢复：重新加入；已有归属保持原样；需要立即平衡时点「重新分配」。
- 没有「删除/停用」设备操作；删除策略里的灯只会移除 `manual` 条目（以及必要的绑定）。

### D6b. 聚合与空灯

每台设备的 session 集合：

- `Mirror`：全部活动 session。
- `Auto`：分配表中属于该设备的 session。
- `Manual`：`routing.manual` 分配给该设备的 Agent 的 session。

聚合复用 `error > waiting > working > idle`，同优先级取最近更新；空集合为 `none`，灯效为 `mapping.none`（默认 off）。全局聚合仍是全部 session 的聚合，不受策略影响。

### D7. 每设备目标状态（DeviceDesired）

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceDesired {
    pub effect: Effect,
    pub brightness: u8,
    pub buzzer_on_alert: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredState {
    /// 已注册设备的目标状态，键为设备 id。
    pub devices: HashMap<String, DeviceDesired>,
    /// 新设备在下一轮刷新前使用的默认状态（全局聚合 + 全局亮度/蜂鸣）。
    pub default: DeviceDesired,
    /// 退出关灯：忽略分配与覆盖，所有设备强制 off。
    pub forced_off: bool,
}

impl DesiredState {
    pub fn for_device(&self, id: &str) -> DeviceDesired {
        if self.forced_off {
            return DeviceDesired {
                effect: Effect::Off,
                ..self.default
            };
        }
        self.devices.get(id).copied().unwrap_or(self.default)
    }
}
```

- 计算由 `BeaconState` 负责：它同时持有 session 表、配置、设备注册表与分配表，是唯一能正确解析分配的地方。
- actor 只消费 `for_device(id)`，不再理解分配或聚合；`watch` 的 latest-wins 语义不变。
- 设备新插入时，worker 通过 `BeaconState::register_device()` 注册并立即刷新一次 desired，避免新设备先用 `default` 闪一下错误颜色。
- `brightness` / `buzzer_on_alert` 的解析顺序：设备绑定覆盖 > 全局配置。这样「三台灯只有一台响」可以通过 `buzzer_on_alert: false` 实现。

### D8. 快照与 API

`GET /api/v1/state` 与 SSE 快照新增：

```json
{
  "aggregate": "error",
  "effect": "alert",
  "sessions": [
    {
      "agent": "claude-code",
      "session": "claude-code:7f3a",
      "state": "working",
      "created_at_ms": 1762700000000,
      "updated_at_ms": 1762700100000,
      "expires_at_ms": 1762701000000
    }
  ],
  "devices": ["…既有字段不变…"],
  "routing": [
    {
      "device": "serial:E6614C03",
      "label": "左灯",
      "assignment": "auto",
      "agents": ["claude-code", "codex"],
      "sessions": ["…分配到这台灯的 session…"],
      "aggregate": "error",
      "auto_effect": "alert",
      "effect": "alert",
      "overridden": false,
      "connected": true,
      "last_seen_ms": 1762700100000
    }
  ],
  "unassigned_agents": []
}
```

字段说明：

- `routing`：每台已配置或已发现的设备一条，包括当前未检测到的设备；同一设备 id 只出现一次；带 `connected` 与 `last_seen_ms`。
- `agents`：当前在这台灯上有活动 session 的 agent 去重列表。
- `auto_effect`：分配后聚合映射的灯效；`effect`：叠加手动覆盖后的最终灯效。
- `unassigned_agents`：没有出现在任何设备 session 集合中的 agent；统一显示模式下为空。
- 配置里的 `routing.strategy` 与 `routing.manual` 通过 `config` 字段返回，设置页据此渲染四种策略。

`SessionView` 增加 `created_at_ms`，用于自动分配的稳定排序；`updated_at_ms` 仍表示最近一次事件时间。

`GET /api/v1/devices` 保持既有字段，增加 `label`；详细分配信息通过 `GET /api/v1/state` 的 `routing` 提供。

新增 `POST /api/v1/routing/rebuild`：清空粘性分配并按当前策略重新计算，供「重新分配」按钮使用。该操作不修改配置。

### D9. 控制台

**状态页：**

- 顶部「当前聚合状态」始终表示全部会话的聚合，与当前策略无关。
- 每台灯一张状态卡片，展示该灯自己的 `routing[].aggregate` / `routing[].effect`、正在显示的 Agent、连接状态与设备信息；视觉结构参考顶部聚合卡片（状态图标、状态名称、灯效 badge、状态色调）。
- 离线灯显示为未连接，并显示其最近一次分配/回退状态。

**设置页：多灯控制策略**

多灯配置**不放在灯具页**。灯具页保持原有的手动灯效与设备型号管理；设置页新增一张独立的「多灯控制策略」卡片：

1. 四个策略按钮，单选：
   - 统一显示
   - 按 Agent 自动分配
   - 按 Agent 手动分配
   - 按 Session 自动分配
2. 每个按钮下方显示一句简短说明；选中后卡片底部显示该策略的详细说明。
3. 只有「按 Agent 手动分配」才展开详细配置：
   - 列出所有灯：已检测到的灯、配置中的离线灯、`routing.manual` 里保留但当前不存在的灯。
   - 每台灯显示在线/离线/配置已失效，以及已分配的 Agent。
   - 每台灯提供「添加 Agent」选择器；同一个 Agent 只能出现在一台灯上。
   - Agent 以 chip 展示，可单独移除。
   - 未检测到的灯显示「删除这台灯的配置」；删除会移除 `routing.manual` 条目和对应的 `devices` 绑定。
   - 没有分到 Agent 的灯显示为未分配；空灯使用 `mapping.none`（默认熄灭）。
4. 自动策略（`auto_agent` / `auto_session`）只显示说明、当前未分配 Agent 和「重新分配」按钮。
5. 统一显示不提供额外配置。

### D10. 诊断

- `journal` 的 `Config` 记录追加摘要（例如 `strategy=manual_agent devices=3 manual_lights=2`）。
- 设备 actor 日志补充角色、分到的 Agent 数与最终 `effect`。
- `POST /api/v1/routing/rebuild` 记录一条 `Config` journal，detail 注明「routing rebuilt」。
- 快照的 `routing`、`unassigned_agents`、`config.routing.manual` 是最强的诊断入口。

### D11. 兼容与迁移

- 旧配置（没有 `routing`）：按 `strategy = unified`、`manual = {}` 加载，行为与现在一致。
- `Config` 顶层已有 `#[serde(default)]`；`version` 保持 `1`。
- 旧前端/旧 API 客户端忽略 `routing`、`unassigned_agents`、`created_at_ms` 即可。
- 旧的 `DesiredState` 字段只在 crate 内部使用，可以安全替换；`agent-beacon-lights` 的测试同步更新。
- 旧的手动覆盖 API 不变；全局覆盖仍然影响所有设备。
- 本变更不提供 session 手动绑定；session 只作为自动分配的输入。

### D12. 分阶段实施

| 阶段 | 内容 | 可独立验收 |
| --- | --- | --- |
| P1 核心 | `RoutingStrategy`、`routing.manual`、四种分配算法、`created_at_ms`、`DeviceDesired`、快照 `routing`/`unassigned_agents`、测试 | 四种策略可切换，session/Agent 分配正确 |
| P2 设置页 | 四策略单选、手动模式灯/Agent 编辑、离线灯保留与删除、重新分配 | 不碰配置文件也能完成手动映射 |
| P3 增强 | 每设备亮度/蜂鸣 UI、分配历史/诊断、权重、设备离线提示优化 | 按需 |
| 后续 | 单设备多通道、灯组、按权重调度 | 不在本变更 |

## 配置示例

```json
{
  "version": 1,
  "mapping": { "none": "off", "idle": "calm", "working": "busy", "waiting": "attention", "error": "alert" },
  "routing": {
    "strategy": "manual_agent",
    "manual": {
      "serial:E6614C03A1B2C3D4": ["claude-code", "codex"],
      "ble:af:c3:a4:e1:d8:95": ["pi"]
    }
  },
  "devices": [
    {
      "serial": "E6614C03A1B2C3D4",
      "profile": "mlight-3led",
      "label": "左灯",
      "brightness": 80,
      "buzzer_on_alert": null
    },
    {
      "id": "ble:af:c3:a4:e1:d8:95",
      "port": "ble:af:c3:a4:e1:d8:95",
      "profile": "agentcore-light",
      "label": "右灯",
      "buzzer_on_alert": false
    },
    {
      "port": "/dev/ttyUSB0",
      "profile": "yh50-rgy-j",
      "label": "总览"
    }
  ]
}
```

## 算法示例

**场景：1 个 agent、3 个 session、3 台 auto 灯**

```
sessions: C1(t1), C2(t2), C3(t3)
devices:  A, B, C（按 id 升序）

C1: 负载 A0 B0 C0 → A
C2: 负载 A1 B0 C0 → B
C3: 负载 A1 B1 C0 → C
结果：A=C1, B=C2, C=C3
```

**场景：同一 agent 新增第 4 个 session**

```
C4(t4): 负载 A1 B1 C1 → 平手，按 id 升序 → A
结果：A=C1+C4, B=C2, C=C3
已有 C1/C2/C3 不动
```

**场景：按 Agent 手动分配**

```
strategy = manual_agent
manual: { A: [claude], B: [codex] }
Claude 的 C1/C2/C3 全部 → A
Codex 的 X1/X2 → B
C 没有分配 Agent → 空灯，显示 off
删除 A 的配置 → Claude 变为未分配
```

**场景：按 Agent 自动分配**

```
strategy = auto_agent
A/B/C 三台灯
Claude(2 sessions), Codex(1), Pi(1)
→ Claude → A, Codex → B, Pi → C
Claude 的 2 个 session 都在 A
新增 Agent Q → 当前 Agent 数最少的灯（平手按 id）
```

**场景：按 Session 自动分配**

```
strategy = auto_session
A/B/C 三台灯
Claude 的 C1/C2/C3
→ C1→A, C2→B, C3→C
新增 C4 → 当前 session 数最少的灯（平手按 id）
```

**场景：统一显示**

```
strategy = unified
所有灯显示全部会话的全局聚合
```

## 备选方案

1. **按 session 手动绑定**
   - 用户明确反对：session 易变、数量不定，无法提前配置。
   - 结论：不提供。

2. **纯 rendezvous / 一致性哈希按 session 分配**
   - 稳定、无状态，但少量 session 时不保证分散；3 个 session 可能全落一台灯。
   - 结论：作为理论备选，不采用。

3. **轮询（round-robin）**
   - 分散完美，但删掉一个 session 会让后续 session 全部错位。
   - 结论：不采用。

4. **按状态优先级动态分配**
   - `error` 的 session 自动换到最显眼的灯；用户无法建立稳定认知。
   - 结论：不采用。

5. **agent 级哈希 + 一个 agent 一台灯**
   - 实现简单，但 1 个 agent 3 个 session 时三台灯只有一台亮，无法满足本需求。
   - 结论：不采用；本设计用 session 级分配解决。

6. **单台三灯珠每颗一个 session**
   - MLight 协议允许并发常亮，但动画命令只有一条全局模式，颜色只有红/黄/绿。
   - 结论：需要独立通道编排模型，后续再做。

## 风险与待确认

1. **粘性分配的「历史依赖」**：同一组 session 在服务重启后可能得到与之前不同的归属（因为内存分配表清空，按 `created_at_ms` 重新排序）。用户通常不会感知；如果需要跨重启完全一致，可以把分配表持久化到配置，但会引入写放大。
2. **设备增加不会自动搬走旧 session**：新增一盏灯后，已有 session 仍留在原灯，新 session 才会优先使用新灯；这是为了稳定。需要立即平衡时点「重新分配」。设备长时间离线时只迁移原本落在它上面的 session，其他灯不动。
3. **离线设备有 60 秒宽限**：短暂拔插/蓝牙抖动不会立即引发重排；超过 `MISSING_GRACE_SECS` 后，auto 模式下的 Agent/session 才迁移到其他在线灯。manual 模式保留离线灯的配置；如果灯已不在设备列表里，其 Agent 视为未分配，可在设置页删除该灯。
4. **手动分配会让未分配 Agent 的灯空闲**：这是手动约束的本意；切换到自动策略即可恢复。
5. **少量 session 的负载差最大为 1**：这是「最少会话优先」的保证；如果用户希望按权重分配，后续再加 `weight`。
6. **agent 名字大小写/拼写变化**：会被视为新 agent，pin 不生效。UI 从活动值中选择，手输只校验非空。
7. **session id 默认值**：不传 session 的 agent 只有一条 `{agent}:default`，无法分散；需要 agent 侧提供稳定/唯一的 session id 才能利用多灯。
8. **端口绑定的稳定性**：没有序列号的设备按端口绑定，重插后端口变化会导致设备 id 变化；旧 id 上的 session 会成为孤儿并迁移到剩余 auto 设备。UI 提示优先按序列号/id 绑定。
9. **手动模式下没有 Agent 的灯会一直空**：这是手动模式的本意；UI 会显示「还没有分配 Agent」，空灯按 `mapping.none` 处理。
10. **同一个 Agent 只能属于一台灯**：手动映射是分区；把 Agent 添加到另一台灯会从原灯移除。想同时显示在多个灯，使用自动策略或后续的灯组能力。
11. **空灯使用 `mapping.none`**：不再提供单独的空分配回退设置；需要强制熄灭时把 `mapping.none` 设为 `off`。
12. **OpenSpec 归档顺序**：当前 `openspec/specs/` 为空，`bootstrap-agent-beacon` 尚未归档；本变更对 `light-control` / `web-console` / `beacon-service` 使用 MODIFIED 增量，需先归档 bootstrap（或把其基线 specs 物化到 `openspec/specs/`）再归档本变更。`openspec validate --strict` 会因此给出 INFO 提示，但不影响本变更本身的校验。

## 测试计划

- **core**：`SessionView.created_at_ms` 保留与刷新；`aggregate_of` 覆盖优先级、同优先级最近更新、空集合。
- **config**：默认值、旧配置（无 `routing`）行为不变、非法策略/亮度/空 Agent/重复 Agent 被拒绝。
- **assignment**：统一显示；`auto_agent` 一个 Agent 一台灯、4 Agent/3 灯 → 2/1/1；`auto_session` 3 session/3 灯 → 1/1/1、4/3 → 2/1/1；`manual_agent` 按灯配置、一个 Agent 只属于一台灯、失效灯 Agent 未分配；新增 session/Agent 不移动已有；短暂离线保留归属，超过 grace 后只迁移原设备上的内容。
- **state**：每设备 `DeviceDesired` 不同；统一显示跟随全局；auto/manual 空集合显示 `mapping.none`；全局/单设备覆盖与分配优先级；`forced_off` 压过一切；新设备注册后立即拿到正确效果；离线 grace 结束后 auto 设备退出分配集合。
- **lights**：actor 读取每设备亮度/蜂鸣；`forced_off` 下发 off；设备重连重放各自效果。
- **API**：快照包含 `routing`、`unassigned_agents`、`config.routing`；`POST /api/v1/routing/rebuild` 触发重新分配；`PUT /api/v1/config` 保存策略/manual 后立即生效；旧客户端字段不受影响。
- **web**：设置页四种策略单选；只有手动模式显示灯/Agent 编辑；离线灯保留与删除；没有 session 选择器；灯具页保持原样。
- **e2e**：四种策略切换；`auto_session` 下 3 台 mock 灯各拿到一个 session；`auto_agent` 下同一 Agent 的多个 session 落在同一台灯；`manual_agent` 下离线灯配置保留、删除后 Agent 未分配。
