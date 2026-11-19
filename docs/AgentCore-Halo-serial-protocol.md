# AgentCore-Halo 控制协议（与 AgentCore-Light 的差异）

AgentCore-Halo 与 AgentCore-Light 共用同一套 ASCII 文本行协议：`HELLO` 握手、状态词
（`IDLE` / `BUSY` / `SUCCESS` / `WAIT_CONFIRM` / `ERROR` / `OFF`）、`STATUS:` / `SETTINGS:` /
`SOUND:` / `CHG:` 上报都一致，命令与上报的通用格式见
[`AgentCore-Light-serial-protocol.md`](./AgentCore-Light-serial-protocol.md)。本文只记录两者
不同的地方，以及 AgentBeacon 对 Halo 的接入方式。

> 来源：对厂商上位机 **AgentCoreHalo v2.5.2（Linux）** 的反编译（`agentcore_light_app.py` 与
> `PYZ.pyz` 中的 `service.agentcore_serial_service`）。厂商没有公开协议文档，`LC:` 一节按
> 反编译结果实现，AgentBeacon 连接时会整表下发一次（见 §3）。

## 1. 名称与识别

| | AgentCore-Light | AgentCore-Halo |
| --- | --- | --- |
| BLE(NUS) 广播名 | `AgentCore-Light-<suffix>`（默认 `AgentCore-Light-BLE`） | `AgentCore-Halo-<suffix>`（默认 `AgentCore-Halo-BLE`） |
| 经典蓝牙 / SPP 名 | `AgentCore-Light`（无后缀） | `AgentCore-Halo`（无后缀） |
| BLE 服务 | NUS `6e400001-b5a3-f393-e0a9-e50e24dcca9e` | 同左 |
| `SET_BT_NAME` 规则 | `AgentCore-Light-` + 1–15 个 `[A-Za-z0-9_-]`，总长 ≤ 31 | `AgentCore-Halo-` + 同样后缀规则，总长 ≤ 30 |

Halo 上位机扫描 BLE 时用 `name.lower().startswith("agentcore-halo")` **或** 广播里有 NUS
service UUID 两条件之一；SPP 侧则要求产品名以 `AgentCore-Halo` 开头。

AgentBeacon 按 **广播名 / USB 产品名** 选择 profile：

- 名字以 `agentcore-halo` 开头 → `agentcore-halo`
- 名字以 `agentcore-light` 开头 → `agentcore-light`
- 只有 NUS service、没有名字时按 `agentcore-light` 处理（协议相同，能力一致）

两者能力相同（亮度 + 蜂鸣器），只是型号与外观不同。

## 2. 设置项

```
设备 → SETTINGS:BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,LOW_BATTERY_ALERT:1
```

四个字段与 Light 的 `SETTINGS:` 一致；区别是 Light 还会另发一条
`SLEEP_SETTINGS:IDLE_SLEEP_SEC:..,BT_DISCONNECT_OFF_SEC:..,SLEEP:ON|OFF`，而 Halo 上位机只消费
上面四个字段（休眠相关设置不在它的界面里）。

## 3. 自定义灯效 `LC:`（Halo 上位机特性）

**一个状态一条命令**：

```
LC:<state_index>,<effect>,<r>,<g>,<b>,<period_ms>,<on_ms>,<off_ms>
```

> 反编译出来的厂商代码是把整表用 `;` 拼成一行发的，但**固件把 `;` 当成命令分隔符**：
> 实机发 `LC:0,...;1,...;2,...;` 时只有第一段被当成 `LC:`，其余每段都回
> `ERR UNKNOWN:<那一段>`，灯效自然不生效。所以必须一条 `LC:` 带一个状态。

| 字段 | 取值 |
| --- | --- |
| `state_index` | 0 `IDLE`、1 `BUSY`、2 `SUCCESS`、3 `WAIT_CONFIRM`、4 `ERROR`、5 `OFF` |
| `effect` | 0 `default`、1 `solid`、2 `blink`、3 `breathe`、4 `off` |
| `r,g,b` | 0–255 |
| `period_ms` | blink：`on+off`；breathe：周期（slow 4000 / medium 2400 / fast 1200）；其它为 0 |
| `on_ms`,`off_ms` | blink 时序：slow 1000/1000、medium 500/500、fast 200/200；其它为 0 |

默认颜色：`IDLE` / `SUCCESS` `#00FF00`，`BUSY` / `WAIT_CONFIRM` `#FFD400`，`ERROR` `#FF0000`，
`OFF` `#000000`。

### AgentBeacon 下发的灯效表

`BUSY` / `WAIT_CONFIRM` 的默认色 `#FFD400` 偏黄，实机上两者颜色一样、也看不出“正在跑”，
所以 AgentCore-Halo **连接建立后先下发 6 条 `LC:`**，把这两个状态都改成橙色，靠动画区分：

```
LC:0,0,0,255,0,0,0,0
LC:1,3,255,106,0,2400,0,0
LC:2,0,0,255,0,0,0,0
LC:3,1,255,106,0,0,0,0
LC:4,0,255,0,0,0,0,0
LC:5,0,0,0,0,0,0,0
```

| state_index | 动画 | 颜色 |
| --- | --- | --- |
| 0 `IDLE` | `default` | `#00FF00` |
| 1 `BUSY` | `breathe` 2400 ms | `#FF6A00`（橙色，厂商默认是 `#FFD400`） |
| 2 `SUCCESS` | `default` | `#00FF00` |
| 3 `WAIT_CONFIRM` | `solid` | `#FF6A00`（橙色常亮，厂商默认是 `#FFD400`） |
| 4 `ERROR` | `default` | `#FF0000` |
| 5 `OFF` | `default` | `#000000` |

**`effect = 0 (default)` 会忽略 `r,g,b`**：固件直接用它自己的内置灯效（所以只改颜色、
动画写 `default` 时，状态依旧是原来的黄）。想换颜色必须显式写一个动画（`breathe` /
`solid` / `blink`）：这就是 `BUSY` 写 `3` + `2400`（呼吸）、`WAIT_CONFIRM` 写 `1`（常亮）
的原因。

这张表是**覆盖式**的：下发后厂商上位机里保存的自定义灯效会被替换。因此只在连接建立时发一次
（每次重连都会重发），之后的运行过程仍然只下发语义状态词（`IDLE` / `BUSY` / …），不会因为
状态切换反复覆写灯效表。AgentCore-Light 固件没有 `LC:` 命令，不受影响。

### 实机调试

改灯表不用重编译：`AGENT_BEACON_HALO_LC` 里按上面的格式写 6 段（`;` 分隔，带不带 `LC:`
前缀都行），会整表替换内置灯表；不合法时忽略并记一条 warn。连上设备后日志里能看到实际
下发的每一行：

```bash
AGENT_BEACON_HALO_LC='0,0,0,255,0,0,0,0;1,1,255,106,0,0,0,0;2,0,0,255,0,0,0,0;3,0,255,212,0,0,0,0;4,0,255,0,0,0,0,0;5,0,0,0,0,0,0,0' \
  agent-beacon --port /dev/ttyACM0
```

想手工试一条，直接发单行（厂商后台服务要停掉，否则会抢读串口）：

```bash
python3 scripts/agentcore_probe.py -p /dev/ttyACM0 raw 'LC:3,1,255,106,0,0,0,0'   # 需要关注：橙色常亮
python3 scripts/agentcore_probe.py -p /dev/ttyACM0 state BUSY                     # 触发重绘
```

单行试效果时按顺序试：`effect=3`（呼吸，2400）/ `1`（常亮）/ `2`（闪烁，`500,500`）；
若颜色不对但动画生效，则是 RGB 通道顺序或取值区间与预期不同，把实测结论补回本节。

## 4. 电量遥测 `CHG:`

```
设备 → CHG:1,VBAT:387,PER:82,CUR:100     （每 5 秒一次）
```

字段与 Light 完全相同：`CHG` 是充电输入标志、`VBAT` 是电池电压（0.01 V）、`PER` 是电量计
百分比、`CUR` 是固件配置的目标充电电流（mA）。

`CHG:1` **不等于正在充电**：固件的检测标志在充满后仍为 1，悬空检测脚也可能误报，所以厂商
上位机把它当成 `usb_online`，再用电压变化/接近满电等证据去抖 20 s 才确认 `charge_active`。

Halo 上位机的电量模型（用于它的续航/充满时间估算）：

- 电池容量 500 mAh，空电 2.80 V、满电 4.02 V（0.01 V 单位：280 / 402）
- 电压取最近 5 个采样的中值，至少 3 个样本才生效；充电时用百分比模型与实测增长做校正
- `CHG=1` 只代表充电输入在线，真正的“充电中”还要结合电压抬升与稳定性去抖

AgentBeacon 展示 `PER`、`CHG`、`VBAT`、`CUR` 原始读数（见 §6）。

## 5. `STATUS:` 与固件版本

```
设备 → STATUS:IDLE,FW:v0.5.12-jl-bt-name,BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,BLE_STATE:33,BLE_ADV:0,BLE_CONN:1
```

格式与 Light 一致；Halo 上位机用 `FW:` 字段在固件升级前后比对版本。AgentBeacon 会把
`FW:` 显示在设备卡片上。

## 6. AgentBeacon 的展示

profile `agentcore-halo` 的设备卡片会显示：

- 型号 `AgentCore-Halo` 与设备自报名（BLE 广播名 / USB 产品名，如 `AgentCore-Halo-BLE`）
- 固件版本（来自 `STATUS:` 的 `FW:`）
- 电量与充电状态按厂商上位机（本仓库反编译的 **v2.5.2**）实现，代码见
  `crates/agent-beacon-lights/src/battery.rs`。与厂商的差异只在三处（都是因为 `CHG` 不可信）：
  `CHG` 不参与判定、`near_full` 不再单独当证据、上升证据只认 0.03–0.60 V：
  - 读数被充电扰动（`< 2.95 V` / 比静置基线低 ≥0.10 V / 比基线高 0.03–0.60 V）持续 20 秒 →
    充电；回到基线持续 30 秒 → 不在充电
  - 电量取最近 5 个电压样本的中值（≤4.20 V 的样本），极差 ≤0.20 V 才算稳定，不稳定保持上次值
  - 充电期间叠加模型增长（`CUR / 500 mAh × 0.5`，上限 99%），用模型值显示 `≈`
  - 充电中且电量 100% 才显示「已充满」；`PER` 与 `CHG` 只进悬停提示
`agentcore-light` 使用同一个 `CHG:` 上报，因此同样显示电量与固件信息。