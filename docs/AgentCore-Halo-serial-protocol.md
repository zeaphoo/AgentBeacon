# AgentCore-Halo 控制协议（与 AgentCore-Light 的差异）

AgentCore-Halo 与 AgentCore-Light 共用同一套 ASCII 文本行协议：`HELLO` 握手、状态词
（`IDLE` / `BUSY` / `SUCCESS` / `WAIT_CONFIRM` / `ERROR` / `OFF`）、`STATUS:` / `SETTINGS:` /
`SOUND:` / `CHG:` 上报都一致，命令与上报的通用格式见
[`AgentCore-Light-serial-protocol.md`](./AgentCore-Light-serial-protocol.md)。本文只记录两者
不同的地方，以及 AgentBeacon 对 Halo 的接入方式。

> 来源：对厂商上位机 **AgentCoreHalo v2.5.2（Linux）** 的反编译（`agentcore_light_app.py` 与
> `PYZ.pyz` 中的 `service.agentcore_serial_service`）。厂商没有公开协议文档，`LC:` 一节尚未在
> AgentBeacon 中实测。

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

一次把所有状态的灯效配置拼成一行，用 `;` 分隔、以 `;` 结尾：

```
LC:<state_index>,<effect>,<r>,<g>,<b>,<period_ms>,<on_ms>,<off_ms>;...
```

| 字段 | 取值 |
| --- | --- |
| `state_index` | 0 `IDLE`、1 `BUSY`、2 `SUCCESS`、3 `WAIT_CONFIRM`、4 `ERROR`、5 `OFF` |
| `effect` | 0 `default`、1 `solid`、2 `blink`、3 `breathe`、4 `off` |
| `r,g,b` | 0–255 |
| `period_ms` | blink：`on+off`；breathe：周期（slow 4000 / medium 2400 / fast 1200）；其它为 0 |
| `on_ms`,`off_ms` | blink 时序：slow 1000/1000、medium 500/500、fast 200/200；其它为 0 |

默认颜色：`IDLE` / `SUCCESS` `#00FF00`，`BUSY` / `WAIT_CONFIRM` `#FFD400`，`ERROR` `#FF0000`，
`OFF` `#000000`。

AgentBeacon 只下发语义状态词（`IDLE` / `BUSY` / …），不下发 `LC:`，因此不会覆盖用户在厂商
上位机里配置的灯效。

## 4. 电量遥测 `CHG:`

```
设备 → CHG:1,VBAT:387,PER:82,CUR:100     （每 5 秒一次）
```

字段与 Light 完全相同：`CHG` 是充电输入标志、`VBAT` 是电池电压（0.01 V）、`PER` 是电量计
百分比、`CUR` 是固件配置的目标充电电流（mA）。

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
- 电量徽章：`电量 82% · 充电中`（充电绿色、≤20% 警示色；鼠标悬停可看电压与充电电流）

`agentcore-light` 使用同一个 `CHG:` 上报，因此同样显示电量与固件信息。