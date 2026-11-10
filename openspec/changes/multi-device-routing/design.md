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

### D1. 手动单位是 agent，自动单位是 session

- **session 不适合作为配置单位**：id 常是随机 UUID，agent 重启/开新窗口/切换任务都会变化；用户无法提前为未来的 session 绑定灯。
- **agent 是稳定身份**：`claude-code`、`codex`、`pi` 这些名字跨 session、跨重启基本不变。
- **自动分配必须落到 session**：只有一个 agent、多个 session、多台灯时，如果按 agent 分，所有 session 只能挤在一台灯；只有按 session 分，才能把多盏灯都用起来。

因此：

- `routing.pins` 的键是 **agent**，值为设备 id；固定一个 agent = 它的所有 session 都去那台灯。
- 没有固定的 agent，其 session 进入自动池，由算法逐 session 分配。
- 控制台不出现 session 绑定控件；session 只作为 agent 下的只读明细展示。

### D2. 每台设备是 `mirror`、`auto` 或 `manual`

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAssignment {
    /// 跟随全部会话（默认，向后兼容）。
    #[default]
    Mirror,
    /// 参与自动分配：接收算法分到的 session。
    Auto,
    /// 只接收被 pin 到这台设备的 agent，不参与自动分配。
    Manual,
}
```

- 默认 `Mirror`，旧配置不写 `assignment` 时行为与现在一致。
- `Mirror` 设备不参与自动分配，也不占用 session；它永远显示全局聚合。
- `Auto` 设备参与自动分配；可以接收自动 session，也可以接收 pin 到它的固定 agent。
- `Manual` 设备只接收 pin 到它的固定 agent，不接收自动 session；适合「这盏灯只给 Claude」的专用灯。
- 可以同时存在三种设备；为什么不做全局 `mode`：一台灯想镜像全局、一台想自动分摊、一台想专用是常见组合，逐设备设置更灵活。

### D3. 自动算法：最少会话优先 + 粘性

自动分配给**每个活动 session** 选一台 `auto` 设备。规则：

1. 活动 session 按 `(created_at_ms, agent, session)` 升序排序。
2. 被固定的 agent，其全部 session 先强制放到 pin 目标设备（目标必须是启用的 `auto` 或 `manual` 设备）。
3. 未被固定的 session，按排序依次选择**当前已分配 session 数最少**的 `auto` 设备。`manual` 设备不进入自动池。
4. 数量相同时按设备 id 升序决胜，保证确定性。
5. 已有有效分配的 session **保持不动**（粘性）；只有新 session、失去设备的孤儿 session、或显式重建时才重新选灯。

**为什么用「最少会话优先」而不是纯哈希：**

- 纯 rendezvous/一致性哈希是稳定的，但**不保证分散**。3 个 session、3 台灯时，3 个 session 有约 1/9 的概率全落到同一台灯，另外两台全灭，看起来像坏了。
- 最少会话优先在少量 session 时也能保证「能分就分」：3 个 session、3 台灯一定 1/1/1；4 个 session、3 台灯一定 2/1/1。
- 粘性避免了「每来一个 session 就把已有归属全部重排」：新 session 只占一个空位/最少位，已有 session 不动。

**为什么不用轮询（round-robin）：**

- 轮询在 session 增删时会整体错位：删掉一个前面的 session，后面所有 session 的归属都会变。
- 最少会话优先 + 粘性只在必要时移动。

**为什么不用状态优先级：**

- 状态每秒都可能变；按 `error` 优先重新分配会让同一盏灯在不同 agent 之间跳来跳去，用户无法建立「这盏灯是谁」的认知。

**分配示例：**

| 场景 | 结果 |
| --- | --- |
| 3 台灯，1 个 agent 3 个 session | 灯 A/B/C 各 1 个 session |
| 2 台灯，1 个 agent 3 个 session | 灯 A 2 个、灯 B 1 个 |
| 3 台灯，4 个 agent 各 1 个 session | 灯 A 2 个、B 1 个、C 1 个 |
| 1 台灯，1 个 agent 3 个 session | 灯 A 聚合 3 个 session |
| 2 台灯，Claude 2 个 session、Codex 2 个 session | 4 个 session 按 2/2 分，agent 可混合 |

### D4. agent 固定（pins）

```rust
pub struct RoutingConfig {
    /// 没有任何 session 可显示时的回退方式。
    pub empty_fallback: EmptyAssignmentFallback,
    /// agent -> 设备 id；固定后该 agent 的全部 session 都去这台设备。
    pub pins: HashMap<String, String>,
}
```

- 固定目标必须是 `assignment: auto` 或 `assignment: manual` 的设备；`mirror` 设备不需要固定（它本来就显示全部）。
- 固定后，该 agent 的所有 session 都去目标设备，即使目标设备已经有其他 session；一台灯可以同时接收多个固定 agent。
- 固定到 `auto` 设备时，该设备的固定 session 计入负载，自动 session 会自然避开已经很忙的灯；固定到 `manual` 设备时，它不参与自动负载计算，只显示自己的固定 agent。
- 目标设备不存在、长时间离线或为 `mirror` 时，pin 视为**未解析**：该 agent 的 session 回到自动池，快照的 `unresolved_pins` 会列出它，控制台提示「固定目标不可用」。
- 没有 session 级 pin。用户如果想拆分一个 agent 的多个 session，应该取消固定，让自动算法处理。

### D5. 设备在线状态与孤儿迁移

当前实现没有「删除设备」或「停用设备」：设备只有**检测到**和**检测不到**两种状态。本变更不引入 `enabled` / delete，只使用设备是否在线。

- `connected: true`：actor 已打开传输，设备可用。
- `connected: false`：actor 尚未打开或失去连接；`DeviceInfo` 记录 `last_seen_ms`（最后一次成功打开的时间），`None` 表示从未连接。
- 配置绑定可以提前存在（例如先按序列号写好），但设备在首次连接前不参与自动分配，避免把 session 分到看不见的灯上。

自动分配集合 `D`：

- `assignment: auto` 且 `connected: true` 的设备；
- `assignment: auto`、`connected: false`，但 `last_seen_ms` 在 `MISSING_GRACE_SECS`（默认 60 秒）内的设备（短暂离线，保留归属）；
- 从未连接过的 auto 设备不进入 `D`；
- 超过 grace 仍未连接：视为不可用，其 session 变为孤儿并迁移。

**核心原则：只迁移孤儿 session，不做整体重排。**

| 变化 | 行为 |
| --- | --- |
| 设备短暂离线（≤ grace） | 保留已有归属，其他灯不动 |
| 设备长时间离线（> grace） | auto 设备上的 session 变为孤儿，迁移到剩余在线 auto 设备；manual 设备上的 pin 变为未解析，session 回到自动池 |
| 离线设备恢复 | 重新加入 `D`；已有 session 保持原归属；新 session 可使用它；需要立即回迁/平衡时点「重新分配」 |
| 配置 `auto`→`mirror` | 该设备上的自动 session 变为孤儿并迁移；其他 session 不动 |
| 配置 `auto`→`manual` | 该设备上的自动 session 变为孤儿并迁移；pin 到它的 session 保留；其他 session 不动 |
| 配置 `manual`→`auto` | 设备加入 `D`；pin 到它的 session 保留；其他 session 不动 |
| 设备首次被检测到 | 加入 `D`；已有 session 不动；新 session 可使用它 |
| pin 新增/改目标 | 该 agent 的 session 迁移到新目标；其他 session 不动 |
| pin 删除或目标长时间不可用 | 该 agent 的 session 进入自动池；其他 session 不动 |
| 显式「重新分配」 | 清空分配表并全量重算 |

没有「删除/停用」操作；如果不想让某台设备参与自动分配，把它的 `assignment` 改为 `mirror`，或者直接拔掉。配置绑定仍可编辑（profile/label/assignment/pins），但 UI 不提供「删除设备」按钮。

实现上用一张 `session -> device` 的粘性表；每次变化只校验有效性并迁移孤儿，不重建整张表。`MISSING_GRACE_SECS` 先用常量（60 秒），后续需要再做成设置。

**为什么设备减少时不整体重排：** 一台设备不可用只需要安顿它原来的 session；把其他灯上的 session 也搬走会让用户看到「明明没动的灯突然换内容」，而且没有任何收益。整体平衡留给显式的「重新分配」。

### D6. 聚合与回退

每台设备的会话集合：

- `Mirror`：全部活动 session。
- `Auto`：分配表中属于该设备的 session（固定 + 自动）。
- `Manual`：只包含 pin 到该设备的 agent 的 session，不接收自动 session。

聚合：

- 对集合内的 session 取 `error > waiting > working > idle`，同优先级取最近更新；空集合为 `None`。
- 每台设备独立聚合；全局聚合仍是全部 session 的聚合，不受分配影响。
- `auto_effect = mapping.effect_for(device_aggregate)`。
- 空集合时按 `routing.empty_fallback`：
  - `none`（默认）：`mapping.effect_for(None)`，默认 `off`；
  - `global`：跟随全局聚合；
  - `off`：强制 `off`，不受 `mapping.none` 影响。

为什么默认 `none` 而不是 `global`：一台被自动分配但暂时没有 session 的灯显示别人的 `error` 会误导；`global` 留给明确想要「空闲时看全局」的用户。

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
      "pinned_agents": ["claude-code"],
      "sessions": ["…分配到这台灯的 session…"],
      "aggregate": "error",
      "auto_effect": "alert",
      "effect": "alert",
      "overridden": false,
      "fallback": null,
      "connected": true,
      "last_seen_ms": 1762700100000
    }
  ],
  "unassigned_agents": [],
  "unresolved_pins": []
}
```

字段说明：

- `routing`：每台已配置或已发现的设备一条，包括当前未检测到的设备；同一设备 id 只出现一次；带 `connected` 与 `last_seen_ms`。
- `agents`：当前在这台灯上有活动 session 的 agent 去重列表。
- `pinned_agents`：配置里固定到这台灯的 agent（即使当前没有活动 session，也保留，供 UI 显示「等待 Claude」）。
- `auto_effect`：分配后聚合映射的灯效；`effect`：叠加手动覆盖后的最终灯效。
- `fallback`：空集合时给出 `none` / `global` / `off`，非空为 `null`。
- `unassigned_agents`：没有出现在任何启用设备 session 集合中的 agent；存在 `mirror` 设备时为空（mirror 覆盖全部 agent）。
- `unresolved_pins`：目标不存在、超过 grace 未检测到或为 `mirror` 的 pin，每项 `{ agent, device }`。

`SessionView` 增加 `created_at_ms`，用于自动分配的稳定排序；`updated_at_ms` 仍表示最近一次事件时间。

`GET /api/v1/devices` 保持既有字段，增加 `label`；详细分配信息通过 `GET /api/v1/state` 的 `routing` 提供。

新增 `POST /api/v1/routing/rebuild`：清空粘性分配并按当前 session 重新计算，供控制台「重新分配」按钮使用。该操作不修改配置。

### D9. 控制台

**状态页：**

- 顶部全局聚合卡片保持不变。
- 设备列表显示：标签、分配方式（镜像/自动/手动）、分到的 agent、session 数、当前灯效、连接状态、最近错误。
- `unassigned_agents` 非空时显示「没有被任何灯分配的 agent」区块。
- `unresolved_pins` 非空时显示「固定目标不可用」提示。

**灯具页：**

- 设备列表合并 `snapshot.devices` 与 `config.devices`：已发现设备可编辑；已配置但当前未检测到的绑定显示为「未连接」，不提供删除/停用按钮。把 `assignment` 改为 `mirror` 可让它退出自动分配，或直接拔掉设备。
- 每台设备一行：标签、profile、分配方式（`镜像全局` / `自动分配` / `仅手动固定`）、亮度/蜂鸣（仅能力支持时）。
- **Agent 分配面板**（不再有 session 选择器）：
  - 列出当前活动 agent，显示它有几个 session、这些 session 当前落在哪些灯上。
  - 每个 agent 一个「自动 / 固定到设备」选择器；固定写入 `routing.pins`，取消固定从 pins 删除。
  - 固定后显示「所有 session 固定到 左灯」；自动时显示「由算法分配到当前 session 最少的灯」。固定前提示：固定会把该 agent 的所有 session 放在同一台灯，取消固定可恢复自动分散。
  - 只允许固定到 `auto` 或 `manual` 设备；没有这两类设备时提示先开启自动分配或创建手动灯。
  - `manual` 设备只显示 pin 到它的 agent；如果它没有 pin，会一直处于空分配回退。
- 「全部设为自动分配」按钮：把当前所有已发现设备写成 `assignment: auto`（需要时创建绑定）。
- 「重新分配」按钮：调用 `POST /api/v1/routing/rebuild`，用于新增灯后立即平衡。
- 分配预览：每台灯显示分到的 agent/session、聚合状态、灯效；空灯显示回退方式。
- 说明文案：「同一 agent 的多个 session 会自动分散到不同的灯；固定 agent 会让它的所有 session 留在同一台灯；设备长时间离线（超过 grace）时只迁移原本在该设备上的 session，其他灯不动；设备恢复在线不会自动搬走旧 session，需要立即平衡时点「重新分配」。」

**设置页：**

- 「空分配时」：`使用无会话映射（默认关闭）` / `跟随全局聚合` / `强制熄灭`。
- 现有映射、默认覆盖时长、退出关灯、全局亮度、全局蜂鸣保持不变；说明可被每设备覆盖。

### D10. 诊断

- `journal` 的 `Config` 记录追加分配摘要（例如 `routing: 3 devices, 2 auto, pins=1`）。
- 设备 actor 日志补充 `assignment`、分到的 agent 数与最终 `effect`。
- `POST /api/v1/routing/rebuild` 记录一条 `Config` 或专门的 journal 记录，detail 注明「routing rebuilt」。
- 不新增 `HookKind`；分配是派生状态，不是 hook 事件。
- 快照的 `routing`、`unassigned_agents`、`unresolved_pins` 是最强的诊断入口。

### D11. 兼容与迁移

- 旧配置（没有 `routing`、`devices[]` 没有 `assignment`）：`empty_fallback = none`、`pins = {}`、`assignment = mirror`，行为与现在逐字节一致。
- `Config` 顶层已有 `#[serde(default)]`；新增字段也带默认值，因此不需要 `config_version` 迁移，`version` 保持 `1`。
- 旧前端/旧 API 客户端忽略 `routing`、`unassigned_agents`、`unresolved_pins`、`created_at_ms` 即可。
- 旧的 `DesiredState` 字段只在 crate 内部使用，可以安全替换；`agent-beacon-lights` 的测试同步更新。
- 旧的手动覆盖 API 不变；全局覆盖仍然影响所有设备。
- 本变更**不保留**上一轮设计里的 `SessionScope` / session 绑定字段；如果那份设计尚未实现，直接采用本设计；如果已经实现，需要一次配置迁移：删除 `scope`，按 agent 生成 `pins` 或 `assignment`。

### D12. 分阶段实施

| 阶段 | 内容 | 可独立验收 |
| --- | --- | --- |
| P1 核心 | `DeviceAssignment`、`RoutingConfig`、`created_at_ms`、最少会话优先分配、pins、`DeviceDesired`、快照 `routing`/`unassigned_agents`/`unresolved_pins`、测试 | 3 台 mock 灯 + 1 个 agent 3 个 session，自动分成 1/1/1 |
| P2 控制台 | 灯具页分配方式、agent 固定面板、重新分配、状态页分配视图、设置页回退 | 不碰配置文件也能完成「Claude 固定左灯，其余自动」 |
| P3 增强 | 每设备亮度/蜂鸣 UI、分配历史/诊断、权重、设备离线提示优化 | 按需 |
| 后续 | 单设备多通道、灯组、按权重调度 | 不在本变更 |

## 配置示例

```json
{
  "version": 1,
  "mapping": { "none": "off", "idle": "calm", "working": "busy", "waiting": "attention", "error": "alert" },
  "routing": {
    "empty_fallback": "none",
    "pins": {
      "claude-code": "serial:E6614C03A1B2C3D4"
    }
  },
  "devices": [
    {
      "serial": "E6614C03A1B2C3D4",
      "profile": "mlight-3led",
      "label": "左灯",
      "assignment": "auto",
      "brightness": 80,
      "buzzer_on_alert": null
    },
    {
      "id": "ble:af:c3:a4:e1:d8:95",
      "port": "ble:af:c3:a4:e1:d8:95",
      "profile": "agentcore-light",
      "label": "右灯",
      "assignment": "auto",
      "buzzer_on_alert": false
    },
    {
      "port": "/dev/ttyUSB0",
      "profile": "yh50-rgy-j",
      "label": "总览",
      "assignment": "mirror"
    }
  ]
}
```

- 左灯固定 Claude 的所有 session；Codex、Pi 的 session 在左灯与右灯之间按「最少会话」自动分配；总览灯始终显示全局聚合。
- 如果 Claude 有 3 个 session，它们都在左灯聚合；如果用户希望 Claude 的 session 也分散，删除 pin 即可。
- 如果希望某台灯只服务固定 agent，把它的 `assignment` 设为 `manual`，再把对应 agent pin 到它；`manual` 设备不接收自动 session。例如把总览设备改成 `manual` 并把 `codex` pin 到它，Codex 就只在那台灯上显示。

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

**场景：固定 Claude 到 A**

```
pins: { claude-code: A }
C1/C2/C3 全部 → A
B、C 没有 session → 按 empty_fallback 处理
取消固定 → Claude 的 session 进入自动池，重新按 1/1/1 分配
```

**场景：2 个 agent、各 2 个 session、2 台灯**

```
C1(t1) → A
X1(t2) → B
C2(t3) → 负载 A1 B1 → A（平手按 id）
X2(t4) → 负载 A2 B1 → B
结果：A=C1+C2, B=X1+X2
agent 自然聚在同一台灯，但不保证；如需保证请 pin
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
3. **离线设备有 60 秒宽限**：短暂拔插/蓝牙抖动不会立即引发重排；超过 `MISSING_GRACE_SECS` 后，auto 设备上的 session 才迁移到其他在线灯，manual 设备上的 pin 才变成未解析。UI 用 `connected: false` 和 `last_seen_ms` 提示；想立即排除一台离线设备，把它的 `assignment` 改为 `mirror`。
4. **固定 agent 会让其他灯空闲**：这是手动约束的代价；取消固定即可恢复自动分散。
5. **少量 session 的负载差最大为 1**：这是「最少会话优先」的保证；如果用户希望按权重分配，后续再加 `weight`。
6. **agent 名字大小写/拼写变化**：会被视为新 agent，pin 不生效。UI 从活动值中选择，手输只校验非空。
7. **session id 默认值**：不传 session 的 agent 只有一条 `{agent}:default`，无法分散；需要 agent 侧提供稳定/唯一的 session id 才能利用多灯。
8. **端口绑定的稳定性**：没有序列号的设备按端口绑定，重插后端口变化会导致设备 id 变化；旧 id 上的 session 会成为孤儿并迁移到剩余 auto 设备。UI 提示优先按序列号/id 绑定。
9. **`manual` 设备没有 pin 会一直空**：这是手动模式的本意；UI 应提示「仅手动固定」设备需要至少一个 pin，否则按空分配回退。
10. **`auto` 设备会同时收到 pin 和自动 session**：如果希望某台灯只服务固定 agent，应使用 `manual`，而不是 `auto` + pin。
11. **`mapping.none` 被改成非 off**：空分配回退 `none` 会跟着变；文档说明 `none` 是「无会话映射」，需要强制熄灭时选 `off`。
12. **OpenSpec 归档顺序**：当前 `openspec/specs/` 为空，`bootstrap-agent-beacon` 尚未归档；本变更对 `light-control` / `web-console` / `beacon-service` 使用 MODIFIED 增量，需先归档 bootstrap（或把其基线 specs 物化到 `openspec/specs/`）再归档本变更。`openspec validate --strict` 会因此给出 INFO 提示，但不影响本变更本身的校验。

## 测试计划

- **core**：`SessionView.created_at_ms` 保留与刷新；`aggregate_of` 覆盖优先级、同优先级最近更新、空集合。
- **config**：默认值、旧配置（无 `routing`、无 `assignment`）行为不变、非法 `assignment`/亮度/空 agent pin 被拒绝。
- **assignment**：3 session/3 灯 → 1/1/1；4 session/3 灯 → 2/1/1；1 agent 3 session/1 灯 → 全在一台；新增 session 不移动已有；过期 session 只移除自己；pin 后全部去目标设备；取消 pin 后进入自动池；unresolved pin 回到自动池；manual 设备只收 pin 到的 agent；短暂离线保留归属，超过 grace 后只迁移原设备上的 session；设备恢复后重新加入分配集合；从未连接的设备不参与自动分配。
- **state**：每设备 `DeviceDesired` 不同；mirror 设备跟随全局；auto 设备空集合三种回退；manual 设备只显示 pin 到的 agent；全局/单设备覆盖与分配优先级；`forced_off` 压过一切；新设备注册后立即拿到正确效果；离线 grace 结束后 auto 设备退出分配集合。
- **lights**：actor 读取每设备亮度/蜂鸣；`forced_off` 下发 off；设备重连重放各自效果。
- **API**：快照包含 `routing`、`unassigned_agents`、`unresolved_pins`；`POST /api/v1/routing/rebuild` 触发重新分配；`PUT /api/v1/config` 保存 pin 后立即生效；旧客户端字段不受影响。
- **web**：灯具页分配方式与 agent 固定；没有 session 选择器；状态页分配视图；设置页回退选择。
- **e2e**：3 台 mock 灯 + 1 个 agent 3 个 session，断言三台灯各拿到一个 session、mock 命令不同；把该 agent pin 到灯 A 后，A 聚合 3 个 session，B/C 回退 off；取消 pin 后恢复 1/1/1。
