# PromLight 状态灯 BLE HID 控制协议

> 本文档只描述**设备侧**的硬件控制协议（设备识别、传输层、帧格式、命令、参数、应答、时序、电量），
> 不涉及厂商上位机的 hook 线协议、Web 控制台、自更新等上层内容。
>
> 依据：厂商 Windows 客户端（`PromLight.exe`，Nuitka onefile）内嵌的 `daemon.dll` 字符串与
> `hid_proto` / `hid_northbound` 模块结构；厂商 macOS 原生守护进程 `promlightd.c`（IOHID 实现）；
> 以及 Linux 侧真机逐字节验证的等效还原（设备固件 v1.0，2026-08-13）。
> 文中「推断」为尚未在真机逐条验证的结论。

- 文档版本：v1.0
- 整理日期：2026-11-10

---

## 目录

1. [概览](#1-概览)
2. [与 AgentCore-Light 文本协议的关系](#2-与-agentcore-light-文本协议的关系)
3. [设备识别](#3-设备识别)
4. [传输层](#4-传输层)
5. [帧格式](#5-帧格式)
6. [应答与错误](#6-应答与错误)
7. [命令表](#7-命令表)
8. [持久参数表](#8-持久参数表)
9. [灯效语义](#9-灯效语义)
10. [事务与时序](#10-事务与时序)
11. [电池电量](#11-电池电量)
12. [主机侧灯效映射（建议）](#12-主机侧灯效映射建议)
13. [完整示例](#13-完整示例)
14. [参考实现：帧构造与解析](#14-参考实现帧构造与解析)
15. [已知限制](#15-已知限制)
16. [待验证项](#16-待验证项)
17. [参考资料](#17-参考资料)

---

## 1. 概览

PromLight 是一块 BLE 状态灯（红/绿/黄三色），主机通过 **BLE HID（HOGP）Vendor 报文**
下发**二进制帧**控制灯效与持久参数。设备同时把电量暴露为标准 BLE Battery Service。

| 项目 | 值 |
| --- | --- |
| 通信方式 | BLE HID（HOGP），Vendor 自定义 usage page |
| USB Type-C | **仅充电，无数据线**（不能走串口/USB HID） |
| Report | 单一 Vendor Input/Output report，**Report ID = 0x02**，数据 64 字节 |
| 帧格式 | `5E 5E LEN CODE DATA… FCS`（二进制，非文本） |
| 校验 | FCS = 帧内前置字节逐字节异或（1 字节） |
| 应答 | 有：成功 `0x80+cmd`，失败 `0x40+cmd` + 错误码 |
| 分包 | **无**，一帧必须放进单个 64 字节 report |
| 状态查询 | 只可读取 4 个持久参数；电量走 BLE Battery Service，不在 HID 协议内 |

能力矩阵：

| 能力 | 支持 | 说明 |
| --- | --- | --- |
| 三色灯常亮/熄灭 | ✅ | 单色、多色组合、全灯 |
| 闪烁 | ✅ | 次数、周期、渐变（呼吸）可配 |
| 独占模式 | ✅ | 先灭其他通道再执行 |
| 亮度 | ✅ | 持久参数，1–100 % |
| 闪烁周期/渐变时间 | ✅ | 持久参数，可单次覆盖 |
| 广播超时（自动关机） | ✅ | 持久参数，0 = 不自动关机 |
| 电池电量 | ✅ | 标准 BLE Battery Service `0x180F/0x2A19` |
| 固件升级 | ❌ | 未见任何 OTA 命令（推断：无用户可用的升级入口） |
| 蜂鸣器 | ❌（协议层） | 命令表与参数表中**没有任何蜂鸣/声音/静音项**（官方实现里也没找到）。硬件是否配备蜂鸣器**未知**——协议不暴露不等于硬件没有，见 §16 |

---

## 2. 与 AgentCore-Light 文本协议的关系

两者是**同厂商的不同产品线，协议完全不通用**：

| | AgentCore-Light | PromLight |
| --- | --- | --- |
| 链路 | USB CDC / 蓝牙 SPP / BLE(NUS) | 仅 BLE HID（HOGP） |
| 报文 | ASCII 文本行 + `\n` | 二进制帧 `5E 5E …` |
| 命令 | `IDLE` / `BUSY` / `STATUS` / `SOUND:…` | `READ` / `WRITE` / `LED` 三命令 + TLV 选项 |
| 应答 | 文本回显 | 二进制 ACK/ERR |
| 电量 | 协议内 `STATUS` 上报 | 标准 BLE Battery Service |
| 协议文档 | [AgentCore-Light-serial-protocol.md](AgentCore-Light-serial-protocol.md) | 本文 |

厂商把 PromLight 归在 **AgentCore-Light v2** 名下（Linux 侧文档用此名），但两者在
「灯效模型」上是对齐的（黄常亮=处理中、黄闪=等确认、红闪=出错、绿常亮=空闲），
**不能复用同一个链路层实现**，只能复用上层的「状态→灯效」映射（见 §12）。

---

## 3. 设备识别

| 项目 | 值 |
| --- | --- |
| BLE 配对 | 需先在系统蓝牙里配对（HOGP 设备），Linux 内核自动暴露 HID 设备 |
| VID / PID | `0x07D7` / `0x6B01` |
| BlueZ Modalias | `bluetooth:v07D7p6B01…`（用于 udev / D-Bus 匹配设备） |
| HID usage page | `0xFF00`（Vendor），usage `0x01` |
| Report ID | `0x02`（Vendor collection 内唯一的 Input+Output report） |
| Report 长度 | 64 字节 |
| Linux 节点 | `/dev/hidrawN`（**节点号会变**，必须按 VID/PID 在 sysfs 里重新发现） |

Linux 权限：需要给 hidraw 节点授权。HID 父设备**没有** `name` sysfs 属性，不能按
`ATTRS{name}` 匹配，必须按 modalias：

```
ACTION=="add|change", SUBSYSTEM=="hidraw", \
  ATTRS{modalias}=="hid:b0005g0001v000007D7p00006B01", MODE="0660", TAG+="uaccess"
```

Windows/macOS 由系统 HID 栈按 usage page 提供 `IOHIDDeviceSetReport` / `HidD_SetOutputReport`，
不需要放行规则。

---

## 4. 传输层

### 4.1 写缓冲布局（关键差异）

帧本体**不含** Report ID；Report ID 由平台的 HID API 承载，各平台写法不同：

| 平台 | API | 缓冲内容 | 长度 |
| --- | --- | --- | --- |
| Linux | `write(/dev/hidrawN)` | `[0x02] + 帧 + 补 0x00` | 65 |
| Windows | `HidD_SetOutputReport`（hidapi） | `[0x02] + 帧 + 补 0x00` | 65 |
| macOS | `IOHIDDeviceSetReport(dev, kIOHIDReportTypeOutput, 2, buf, 64)` | `帧 + 补 0x00`（**不含** ID，ID 作为单独参数） | 64 |

即：hidapi 的 `write()` 需要自己把 `0x02` 放在首字节；IOKit 把 ID 作为参数传，缓冲里不能再放。

### 4.2 读应答

- IOHID / hidraw 读到的数据**可能**带 `0x02` 前导。解析规则：
  若 `raw[0] == 0x02 && raw[1:3] == 5E 5E` → 丢掉首字节。
- 尾部补零需按 `LEN` 截断，不能按 report 长度解析。

### 4.3 约束

- 一帧必须一次写完、落在单个 64 字节 report 内，**设备不支持跨帧分包**。
- 因此可用 `DATA` 上限为 **59 字节**（见 §5.3）。

---

## 5. 帧格式

### 5.1 布局

```
5E 5E <LEN> <CODE> <DATA...> <FCS>
└─┬─┘       └──┬──┘ └──┬───┘ └┬┘
  magic     命令码    载荷   校验
```

| 字段 | 长度 | 说明 |
| --- | --- | --- |
| magic | 2 | 固定 `0x5E 0x5E` |
| LEN | 1 | **其后的字节数** = `1 (CODE) + len(DATA) + 1 (FCS)`，不含自身 |
| CODE | 1 | 命令码（`0x02` / `0x03` / `0x04`），应答见 §6 |
| DATA | 0–59 | 命令载荷，见 §7 |
| FCS | 1 | 校验，见 §5.2 |

帧总长 = `3 + LEN` ≤ 64。

### 5.2 FCS

FCS = **帧内其之前所有字节的异或**（含两个 `0x5E`）：

```
FCS = 0x5E ^ 0x5E ^ LEN ^ CODE ^ DATA[0] ^ … ^ DATA[n]
    = LEN ^ CODE ^ DATA[0] ^ … ^ DATA[n]        # 因 5E^5E = 0
```

两种写法等价。厂商 macOS 实现只 XOR `LEN/CODE/DATA` 部分，Linux 实现 XOR 整段，结果相同。

- 发送方 FCS 填 `0x00` 表示「不要求校验」，设备仍会正常执行（实测接受）。
- 设备返回的应答若 FCS 为 `0x00`，接收方可跳过校验。

### 5.3 载荷上限

`DATA ≤ 64 - 3 (magic+LEN+FCS) - 1 (CODE) = 59 字节`

### 5.4 非法帧

**设备静默丢弃非法帧，不回任何应答**（结构错、未知命令、LEN 与内容不符等）。
主机侧的超时是唯一的失败信号，超时 ≠ 一定能重试成功（见 §10.1 陈旧 ACK 问题）。

---

## 6. 应答与错误

### 6.1 分类规则

```
kind = CODE & 0xC0
base = CODE & 0x3F

kind == 0x80  →  成功，base 应等于请求命令码
kind == 0x40  →  失败，DATA[0] = 错误码
其它 / base 不匹配  →  视为错位（很可能是上一笔事务的残留应答，见 §10.1）
```

| 请求 | 成功应答 | 失败应答 |
| --- | --- | --- |
| READ `0x02` | `0x82` + `[addr, value]` | `0x42` + `[err]` |
| WRITE `0x03` | `0x83` + `[addr]` | `0x43` + `[err]` |
| LED `0x04` | `0x84` + `[channel]` | `0x44` + `[err]` |

### 6.2 错误码

| 错误码 | 含义 |
| --- | --- |
| 1 | 命令无效（含帧结构/参数个数错） |
| 2 | 地址无效（READ/WRITE 的 addr 不存在） |
| 3 | 校验错（FCS 不符） |
| 4 | 设备忙 |
| 5 | 参数越界（值超出允许范围，**不会写入**） |

命令出错只影响当次操作，设备配置不会被写坏。

---

## 7. 命令表

### 7.1 READ `0x02` — 读持久参数

| 方向 | DATA |
| --- | --- |
| 请求 | `[addr]` |
| 成功应答 | `[addr, value]` |

### 7.2 WRITE `0x03` — 写持久参数

| 方向 | DATA |
| --- | --- |
| 请求 | `[addr, value]` |
| 成功应答 | `[addr]` |

落盘保存（掉电不丢）。参数范围见 §8，越界返回错误码 5。

### 7.3 LED `0x04` — 灯效控制

| 方向 | DATA |
| --- | --- |
| 请求 | `[channel, action, TLV…]` |
| 成功应答 | `[channel]` |

`channel` / `action` / `TLV` 的定义见 §9。

---

## 8. 持久参数表

| addr | 参数 | 原始值 → 物理量 | 允许范围 | 出厂值 | 备注 |
| --- | --- | --- | --- | --- | --- |
| 1 | `led.brightness` 亮度 | 1 byte = 百分比 | 1–100 | 50 | |
| 2 | `led.frequency` 闪烁周期 | ×100 ms | 2–50（200–5000 ms） | 10（1 s） | 未带 `--freq` 时的默认周期 |
| 3 | `led.fadetime` 渐变时间 | ×100 ms | 0–50（0–5000 ms） | 2（200 ms） | 厂商另一份文档写 500 ms，与实测不符 |
| 4 | `ble.advtimeout` 广播超时 | 秒 | 0，或 30–240 | 60 | **0 = 不自动关机** |

说明：

- 参数是**全局持久设置**，不是单次灯效的一部分。想让某次灯效使用非默认周期/渐变，
  用 LED 命令的 TLV 选项（§9.3）覆盖，不要为此改写持久参数。
- `ble.advtimeout` 是省电自动关机：长时间静置后设备自行断电，需要重新按键开机。
  桌面常驻场景建议写 `0`（不自动关机）或 ≥120 s。
- 参数 2/3 的写入值是「100 ms 的整数倍」，主机侧要做 `ms / 100` 换算并四舍五入到整数。

### 8.1 官方实现里的其余地址（推断，未验证）

厂商 Windows 端的 `daemon.dll` 里，`hid_proto` 模块的常量集合除上述 4 个外还有两个：

| 常量名 | 推测含义 | 佐证 |
| --- | --- | --- |
| `ADDR_INFO` | 设备信息（型号 / 固件版本 / 硬件版本 / SN / 特性位） | 同模块存在 `version` / `hw_rev` / `serial_number` / `feature_flags` / `capabilities.describe()`；上层产出的 `capabilities.json` 即由此而来 |
| `ADDR_NAME` | BLE 广播名（可读，可能可写） | 官方命令行的参数表里除 `brightness/frequency/fadetime/advtimeout` 外还有 `info`、`name` 两项 |

它们的**地址值与返回编码未知**（未在真机探测过），因此本文不作为规范给出。
实现时建议：先 `READ` 扫一遍 `addr = 0x05…0x20`，记录哪些返回 `82`、返回多少字节，再决定是否使用。

---

## 9. 灯效语义

### 9.1 channel（位掩码，可组合）

| 值 | 灯 |
| --- | --- |
| `0x01` | 绿 |
| `0x02` | 黄 |
| `0x04` | 红 |
| `0x07` | 全（`all` = 绿\|黄\|红） |

> 厂商 macOS 实现使用 `0xFF` 也能工作（固件按位判断，忽略未知位），
> **新实现建议统一用 `0x07`**。

### 9.2 action

| 值 | 含义 |
| --- | --- |
| `0x00` | 灭 |
| `0x01` | 亮（常亮；配 TLV 4 则从暗渐变到亮） |
| `0x02` | 闪（配 TLV 3/4 决定周期与呼吸感） |

### 9.3 TLV 选项（类型 1 字节 + 值 1 字节，可多个串联）

| 类型 | 含义 | 值 | 说明 |
| --- | --- | --- | --- |
| `0x01` | 独占 `only` | `0xFF` = 开 | 先熄灭**其他**通道再执行本条；官方用 `0xFF`，实测 `0x00/0x01` 也接受 |
| `0x02` | 闪烁次数 `count` | 0–255 | `0` = 持续闪烁（默认） |
| `0x03` | 闪烁周期 `freq` | ×100 ms，2–50 | 覆盖持久参数 addr 2 |
| `0x04` | 渐变时长 `fade` | ×100 ms，0–50 | 覆盖持久参数 addr 3 |

语义细节：

- `freq` 是**完整周期**（亮+灭），`fade` 是单次过渡时长。
- `blink` + `fade` 表现为**呼吸**效果；`fade` 上限受周期约束（不超过周期一半），
  写大了设备会自行裁剪。
- `count` 只在 `blink` 下有意义；`count=1` 为「闪一下就停」。
- `only` 的实现是「先灭其他通道」，因此 `led yellow on --only` 等价于
  「灭绿灭红，黄常亮」；不带 `only` 时，同一通道的新命令覆盖该通道旧状态，
  不同通道各自保持（多灯同时亮/闪是允许的）。

---

## 10. 事务与时序

### 10.1 陈旧 ACK（最容易踩的坑）

设备是**一问一答**模型。若上一次事务的应答没被读走（进程被 kill、蓝牙断开、写超时），
这条 ACK 会留在输入队列里，导致之后**每一笔**事务都错位消费上一笔的应答，且**永不自愈**。

必须在发送前**排空输入队列**：

```
open() → flush 输入（读到空/超时为止）→ 发帧 → 等应答（超时 1.5 s）→ 校验 code 与 base
```

若收到的应答 `base` 与请求命令码不一致，同样按「错位」处理：再排空一次并重发。

> 官方 Windows 实现也把这类异常显式建模成状态机（应答状态 `ok` / `error` / `no-ack` / `bad-ack` / `no-device`，
> 带 `ACK_TIMEOUT_MS` 超时、`_no_ack_streak` 连续无应答计数、`_maybe_hint_no_ack` 提示），
> 反向印证了「排空 + 按 base 校验」是必须的。

### 10.2 串行化

- 所有命令**必须串行**：一次只允许一个未完成事务。
- 多条命令并发时按「最后状态生效」处理（丢弃过期的中间态），避免灯效排队跟不上会话节奏。
- 多条灯效需要按顺序演示（如「三灯闪两下 → 转绿」）时，由**主机侧**加延时逐条发送；
  协议里没有 `wait`/延时命令，设备也不缓存脚本。

### 10.3 断线与重连

- BLE 断开后 hidraw 节点消失；重连后节点号可能变化，需按 VID/PID 重新发现。
- 设备重连后**持久参数仍然保留**，灯效状态不恢复（重新下发即可）。
- 蓝牙配对信息保留在系统侧，重连不需要重新配对。

### 10.4 未验证的时序项

最小帧间隔、连续写入的上限、`busy`（错误码 4）的触发条件均未系统测试，
建议实现时保留 ≥1 个应答往返的间隔，并对 `busy` 做退避重试。

---

## 11. 电池电量

电量**不在** HID 协议里。设备以标准 BLE Battery Service 暴露：

| 项目 | 值 |
| --- | --- |
| Service | `0x180F` Battery Service |
| Characteristic | `0x2A19` Battery Level（0–100，Notify） |

各平台读法：

- **Linux（BlueZ）**：系统总线上 `org.bluez` → 按 `Device1.Modalias` 含 `v07D7p6B01` 找设备
  → 读 `org.bluez.Battery1.Percentage`（BlueZ 缓存 GATT 通知值），并检查 `Device1.Connected`。
- **Windows / macOS**：系统蓝牙栈同样以标准 Battery Service 暴露，走各自的 GATT 客户端读取即可。
- 建议本地缓存 30 s，避免频繁查询。

⚠️ 设备断开蓝牙时该值不可信，必须结合「连接状态」判断，不要用过期的电量做告警。

---

## 12. 主机侧灯效映射（建议）

以下是厂商 `events.json` 中的默认映射（**主机侧语义**，非设备协议），可直接作为
AgentBeacon 的 `idle / working / waiting / error` 四态映射参考：

| 状态 | 北向命令（主机侧 DSL） | 帧 |
| --- | --- | --- |
| 空闲 / `idle` | `led green on --only` | `5E 5E 06 04 01 01 01 FF FC` |
| 处理中 / `working` | `led yellow on --only` | `5E 5E 06 04 02 01 01 FF FF` |
| 等待确认 / `waiting` | `led yellow blink --only` | `5E 5E 06 04 02 02 01 FF FC` |
| 出错 / `error` | `led red blink --only` | `5E 5E 06 04 04 02 01 FF FA` |
| 新会话开始 | `led all blink --only --count 2` → 延时 → `led green on` | `5E 5E 08 04 07 02 01 FF 02 02 F7` |
| 会话结束 | `led green blink --freq 4000 --fade 2000` | `5E 5E 08 04 01 02 03 28 04 14 34` |
| 关闭 | `led all off` | `5E 5E 04 04 07 00 07` |

> 「北向命令 DSL」（`led <color> <action> --only/--count/--freq/--fade`、`write <param> <value>`、
> `read <param>`、`wait <ms>`）是**厂商主机的内部命令语言**，设备不认识。它的唯一作用是把
> 文本命令编译成上面的二进制帧；`wait` 纯粹是主机侧延时不产生任何帧。
> 自行实现时可以直接构造帧，不必照搬 DSL——但若要复用厂商的 `events.json`/宏配置，
> 就需要实现这套 DSL 的解析。

---

## 13. 完整示例

以下帧的 FCS 均已验算，并与厂商文档实测值一致。

### 13.1 常亮

```
led green on --only      5E 5E 06 04 01 01 01 FF FC
                         应答 84 01
led red on               5E 5E 04 04 04 01 05       # 不带 only，绿/黄保持原状
                         应答 84 04
led all off              5E 5E 04 04 07 00 07
                         应答 84 07
```

### 13.2 闪烁 / 呼吸

```
led green blink --freq 5000 --fade 2500   5E 5E 08 04 01 02 03 32 04 19 23
                                          len=8  绿 闪 T3=50(5000ms) T4=25(2500ms)
                                          应答 84 01

led red on --fade 1000                    5E 5E 06 04 04 01 04 0A 09
                                          len=6  红 亮 T4=10(1000ms)，即 1s 渐亮

led red blink -o -c 3 -f 300              5E 5E 0A 04 04 02 01 FF 02 03 03 03 F7
                                          len=10 红 闪 独占 次数=3 周期=300ms
```

### 13.3 参数读写

```
read  led.brightness     5E 5E 03 02 01 00        应答 82 01 32    # 读回 50%
read  led.fadetime       5E 5E 03 02 03 02        应答 82 03 02    # 读回 200ms
read  ble.advtimeout     5E 5E 03 02 04 05        应答 82 04 3C    # 读回 60s

write led.brightness 80  5E 5E 04 03 01 50 56     应答 83 01
write led.frequency 1000 5E 5E 04 03 02 0A 0F     应答 83 02
write led.fadetime 0     5E 5E 04 03 03 00 04     应答 83 03
write ble.advtimeout 60  5E 5E 04 03 04 3C 3F     应答 83 04
write ble.advtimeout 0   5E 5E 04 03 04 00 03     应答 83 04    # 不自动关机
```

### 13.4 错误

```
write led.brightness 200   5E 5E 04 03 01 C8 CE   应答 43 05   # 参数越界，未写入
read  led.unknown(addr 9)  5E 5E 03 02 09 08      应答 42 02   # 地址无效
```

---

## 14. 参考实现：帧构造与解析

Python（与传输无关，仅帧层）：

```python
REPORT_ID, REPORT_LEN = 0x02, 64
CMD_READ, CMD_WRITE, CMD_LED = 0x02, 0x03, 0x04

def build_frame(code: int, data: bytes = b"") -> bytes:
    body = bytes([0x5E, 0x5E, 1 + len(data) + 1, code]) + data
    fcs = 0
    for b in body:
        fcs ^= b
    return body + bytes([fcs])

def parse_frame(raw: bytes):
    """raw 为一次 input report；非法帧返回 None。"""
    if raw and raw[0] == REPORT_ID and raw[1:3] == b"\x5e\x5e":
        raw = raw[1:]                      # 剥掉 Report ID 前导
    if len(raw) < 5 or raw[:2] != b"\x5e\x5e":
        return None
    length = raw[2]
    if length < 2 or len(raw) < 3 + length:
        return None
    frame = raw[:3 + length]               # 按 LEN 截断，忽略尾部补零
    fcs = 0
    for b in frame[:-1]:
        fcs ^= b
    return {"code": frame[3], "data": frame[4:-1],
            "fcs_ok": frame[-1] in (0, fcs)}

# 写：hidapi 需要自己带 Report ID，并补零到 65 字节
# dev.write(bytes([REPORT_ID]) + build_frame(CMD_LED, b"\x07\x02\x01\xff").ljust(REPORT_LEN, b"\x00"))
```

应答判定：

```python
resp = parse_frame(dev.read(REPORT_LEN + 1))
if resp is None:                       raise TimeoutError("无应答")
kind, base = resp["code"] & 0xC0, resp["code"] & 0x3F
if   kind == 0x40:                     raise DeviceError(resp["data"][0])   # 1..5
elif kind != 0x80 or base != cmd:       raise Desync("应答错位，排空后重发")
```

---

## 15. 已知限制

1. **能力/版本信息未公开编码**：官方实现内部有 `ADDR_INFO`、`capabilities.describe()`、
   `serial_number`/`hw_rev`/`feature_flags` 等符号（见 §8.1），说明固件**很可能**通过
   `READ(ADDR_INFO)` 暴露版本与特性位，但帧内编码未知，本文不给出具体格式。
   在摸清之前，主机侧不要依赖版本协商；WebUI 里显示的「SN」实际就是 MAC（`'SN ' + mac`）。
2. **无分包**：`DATA` 上限 59 字节，复杂灯效必须拆成多条帧由主机侧编排。
3. **无时间戳/序列号**：应答不含事务号，只能靠「串行 + 排空 + base 校验」保证不错位。
4. **写参数走 flash**：`WRITE` 掉电保存，不要高频写（例如每帧都改亮度），否则有寿命风险；
   单次灯效的差异请用 TLV 覆盖。
5. **厂商 macOS 实现不读应答**（只写不读），因此错误码语义主要来自 Linux 侧实测与
   厂商 Windows 端 `daemon.dll` 内嵌文档；早期固件是否完全一致未验证。
6. **`all` 的编码**在厂商两份实现里不一致（`0x07` vs `0xFF`），新实现用 `0x07`。
7. **无 OTA**：未见升级命令，固件升级不可行（推断）。

---

## 16. 待验证项

真机到手后按此清单逐项确认（每项都影响实现细节）：

- [ ] `channel = 0xFF` 与 `0x07` 行为是否完全一致（未知位是否被忽略）
- [ ] 请求帧 FCS 填 `0x00` 是否真的豁免校验（设备是否执行、是否回 OK）
- [ ] `fade = 0` 与「不带 TLV 4」是否有区别（0 是"无渐变"还是"用持久值"）
- [ ] `fade > freq/2` 时设备的实际裁剪行为
- [ ] `count` 在 `on`/`off` 动作下的效果（忽略？还是立即熄灭？）
- [ ] `only` 在多通道命令（`channel=0x03` 绿+黄）下的语义
- [ ] 应答 `DATA` 长度是否严格按 §6.1（尤其 LED 是否总回 `[channel]`）
- [ ] 最小帧间隔、连续写上限、错误码 4（busy）的触发条件
- [ ] 参数 addr 5 及以上是否隐藏存在（逐个 READ 扫一遍看是否回错误码 2）
- [ ] 断电重启后持久参数是否保持、灯效是否复位
- [ ] 同时连两个主机（多配对）时的行为
- [ ] Input report 在 Windows/macOS 上是否也带 `0x02` 前导
- [ ] 电量：Battery Level 的通知周期、低压时是否有额外提示（如红灯告警）
- [ ] **扫隐藏地址**：`READ` 遍历 `addr = 0x05…0x20`，记录哪些返回 `82`（非 `42 02`）；
      重点确认 `ADDR_INFO`（疑似版本/型号/特性位/SN）与 `ADDR_NAME`（疑似广播名）的地址与返回长度
- [ ] `WRITE(ADDR_NAME, <字符串>)` 是否能改广播名、是否掉电保存、长度上限多少（受 `DATA ≤ 59` 约束）
- [ ] **蜂鸣器**：设备随状态/开机时是否有声音输出（听/看），以及是否存在未公开的蜂鸣参数
      （同族 v1 AgentCore-Light 有蜂鸣器，v2 未确认）

---

## 17. 参考资料

| 来源 | 内容 |
| --- | --- |
| 厂商 Windows 客户端 `PromLight.exe` | Nuitka onefile；内嵌 `daemon.dll`（`hid_proto` / `hid_northbound` / `hid_win` / `hid_darwin`），cython-hidapi 0.15 + CPython 3.13 |
| 厂商 macOS 守护进程 `promlightd.c` | IOHID 实现，帧封装/参数换算/DSL 编译，**不读应答** |
| `piaomiaoguying/PromLight` | 上述 macOS 版 + `events.json` 宏表 |
| `DiGuStudent/promlight-linux` | 纯 Python 等效实现 + 逆向文档 `PROTOCOL.md`（应答/错误码/参数表/工程坑） |
| `PromLight-win/PromLight/agent_hook.py` | 厂商 hook 客户端（上层线协议，不在本文范围） |

固件实测基准：设备固件 v1.0（2026-08-13 验证）。