# AgentCore-Light 状态灯串口控制协议

> 本文档只描述**设备侧**协议（链路参数、帧格式、命令、上报、时序），不涉及厂商上位机内部接口。
>
> 依据：厂商 Linux/Windows 程序中 `service/agentcore_serial_service.py`（PyInstaller 解包 + pycdc 反编译）
> 的串口收发逻辑，以及 Linux 实机（`/dev/ttyACM0`，固件 `v0.5.13-jl-ota`）逐条验证。
> 文中「推断」为尚未在真机逐条验证的结论。
>
> 配套测试工具：`scripts/agentcore_probe.py`。

> **AgentBeacon 内的实现**：profile `agentcore-light`（`crates/agent-beacon-lights/src/agentcore.rs`
> 协议与命令计划、`transport.rs` 的 `AgentCoreTransport` 负责 `DTR=1/RTS=0` + `HELLO` 握手与上报读取）。
> 灯效映射：`off → OFF`、`calm → IDLE`、`busy → BUSY`、`attention → WAIT_CONFIRM`、`alert → ERROR`；
> 亮度按百分比 ×3 发 `BRIGHTNESS_LEVEL`。自动识别规则见 `discovery.rs`：
> 杰理 CDC（`4C4A:4155`）直接认定；Espressif `303A` 需产品名/序列号命中 `agentcore`/`jieli`/`ac696`；
> 蓝牙 SPP 端口（`bluetooth`/`jl_spp`/`rfcomm`/`标准串行` 等，含 `SerialPortType::BluetoothPort`）
> 一律先做 `HELLO` 握手确认，握手不通过就当普通串口忽略。也可用配置绑定
> `devices[{ port: "COM5", profile: "agentcore-light" }]` 指定 Windows 上的蓝牙 COM 口。
>
> BLE（NUS）走 `ble.rs`：按名称前缀扫描 → 连 NUS Service → 订阅 TX 通知 → 按 MTU 分片写 RX
> （不支持无响应写时降级为带响应写）。实测要点（Linux/BlueZ）：
> * 同一台设备会广播两条记录，经典蓝牙那条名字正好是 `AgentCore-Light` 且没有 NUS 特征，
>   只认 `AgentCore-Light-<suffix>` 或广播了 NUS Service 的那条；
> * 同一条通知会被 BlueZ 投递两次，300 ms 内的重复行已去重；
> * `StartDiscovery` 会打断已有的 LE 连接，所以已有 BLE 设备时后台不再扫描
>   （设备掉线由它自己的 actor 重连；控制台“重新扫描”会释放 BLE 设备再重新发现）；
> * 诊断：`cargo run -p agent-beacon-lights --example ble_scan`、
>   `--example ble_status -- name:AgentCore-Light-BLE`。

- 设备方案：杰理 JieLi **AC696N**（USB `0x4C4A:0x4155`，早期版本为 Espressif `0x303A`）
- 文档版本：v1.0
- 整理日期：2026-11-09

---

## 目录

1. [概览](#1-概览)
2. [链路与串口参数](#2-链路与串口参数)
3. [帧格式](#3-帧格式)
4. [会话流程](#4-会话流程)
5. [主机 → 设备 命令表](#5-主机--设备-命令表)
6. [设备 → 主机 上报](#6-设备--主机-上报)
7. [错误与校验](#7-错误与校验)
8. [时序要求](#8-时序要求)
9. [固件升级入口](#9-固件升级入口)
10. [手动控制示例](#10-手动控制示例)
11. [已知限制与待验证项](#11-已知限制与待验证项)

---

## 1. 概览

AgentCore-Light 是一块 AI 状态灯：主机把「AI 在干什么」写成状态名发给设备，
设备负责灯效、蜂鸣器提示音，并回传电量。三种链路共用同一套文本协议：

| 链路 | 说明 |
| --- | --- |
| USB CDC（虚拟串口） | 主链路，Linux `/dev/ttyACM0`，Windows `COMx` |
| 蓝牙 SPP（经典蓝牙串口） | 端口名含 `bluetooth` / `jl_spp` / `标准串行` 等 |
| BLE（Nordic UART Service） | 无 `HELLO` 握手，连上直接 `STATUS` |

| 项目 | 值 |
| --- | --- |
| 帧格式 | 8 数据位 / 无校验 / 1 停止位（8N1）/ 无流控 |
| 波特率 | 115200（CDC 下不影响吞吐，但两端都按此配置） |
| 编码 | ASCII / UTF-8 |
| 行结束符 | `LF`（`\n`，**不是 CRLF**；读取端兼容 `\r\n`） |
| 校验/加密 | 无 |
| 通信方向 | 双向：命令 + 回显 + 周期上报 |

---

## 2. 链路与串口参数

### 2.1 USB CDC

```
端口    : /dev/ttyACM0（按 VID 0x4C4A / 0x303A 过滤，再按关键字 agentcore / jieli / ac696 加分）
波特率  : 115200
数据格式: 8N1
DTR/RTS : 打开后置 DTR=1、RTS=0
时序    : 打开 → 等 1.5 s → 清空接收缓冲 → 发 HELLO
```

Linux 权限：当前用户需在 `dialout` 组（`sudo usermod -aG dialout $USER` 后重新登录）。

### 2.2 蓝牙 SPP

- 端口名关键字：`bluetooth`、`jl_spp`、`serial over bluetooth`、`标准串行`
- 参数同上；**连接后必须 `HELLO` 握手**，因为同名的其它蓝牙串口也会占用 COM 口。
- **通道号 = 2**（实测；通道 1 返回 `ECONNREFUSED`）
- SPP 下设备输出是 **CRLF**（`HELLO:OK\r\n`），串口/BLE 是 LF；按 `\n` 切行后 strip `\r` 即可
- 两种接法：
  1. **免 root（推荐）**：直接开蓝牙 socket `AF_BLUETOOTH / BTPROTO_RFCOMM` 连 `(MAC, 2)`。
     这是普通 socket 操作，不创建内核 TTY，不需要 `CAP_NET_ADMIN` 也不需要 `dialout`。
  2. 传统接法：`bluetoothctl pair <MAC>` → `sudo rfcomm bind 0 <MAC> 2` → 当普通串口 `/dev/rfcomm0` 用。
     这条路径**必须 root**，因为 `rfcomm` 走 `RFCOMMCREATEDEV` ioctl 让内核建 TTY 设备，
     内核里用 `capable(CAP_NET_ADMIN)` 检查，且建出来的节点是 `root:dialout 660`。
- 前提都是设备**已配对**（`bluetoothctl pair`，这一步走 D-Bus/polkit，不需要 root）。

### 2.3 BLE（NUS）

| 用途 | UUID |
| --- | --- |
| Service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` |
| RX（主机 → 设备，Write） | `6e400002-b5a3-f393-e0a9-e50e24dcca9e` |
| TX（设备 → 主机，Notify） | `6e400003-b5a3-f393-e0a9-e50e24dcca9e` |

- 广播名前缀 `AgentCore-Light`（可用 `SET_BT_NAME` 改名）
- 写入：整行 UTF-8 + `\n`；默认 MTU 下 20 字节分片，链路不支持无响应写时降级为带响应写
- BLE 不发 `HELLO`，连上先 `STATUS`
- 实测两条链路的地址/名字不同（同一台设备）：
  经典蓝牙 `<MAC>`（名 `AgentCore-Light`，SPP 用）与
  BLE `<MAC+1>`（广播名 `AgentCore-Light-BLE`，NUS 用），第三个字节起不同 —— 需要分别记录。
- 已验证：BLE 上 `STATUS` / 设置类命令 / `SOUND:` / 报错 / `CHG:` 心跳全部与串口一致，
  主机**不需要** `HELLO`，直接 `STATUS` 即可。

> **重要（实测）**：设备插着 USB 时**不广播**——`STATUS` 里 `BLE_ADV:0`，`hcitool lescan`
> 与经典蓝牙 inquiry 都扫不到它（`bluetoothctl devices` 里的条目是历史缓存）。
> 固件也没有开广播的命令（`BLE_ADV:1` / `BLE:ON` / `ADV:1` 均返回 `ERR UNKNOWN`）。
> 所以测蓝牙必须**拔掉 USB**，设备用电池供电后才会进入可发现/可连接状态。

---

## 3. 帧格式

```
主机 → 设备:  <COMMAND>[:<arg>[,<arg>...]]\n
设备 → 主机:  <KEY>:<VALUE>[,<KEY>:<VALUE>...]\n
```

- 命令名大小写不敏感（设备侧一律大写匹配）。
- 多字段行按 `,` 切分，每段按**第一个** `:` 切成 key/value。
- `STATUS:IDLE,FW:v0.5.13-jl-ota,...` 中第一段没有 `:`，它是状态值本身。
- 数值十进制；布尔用 `ON`/`OFF`（设置查询）或 `0`/`1`（告警开关、充电标志）。

---

## 4. 会话流程

```
主机 → HELLO
设备 → HELLO:OK

主机 → GET_SETTINGS
设备 → SETTINGS:BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,LOW_BATTERY_ALERT:1,STORE:VM
设备 → SLEEP_SETTINGS:IDLE_SLEEP_SEC:600,BT_DISCONNECT_OFF_SEC:300,SLEEP:OFF,STORE:VM

主机 → GET_BT_NAME
设备 → BT_NAME:CURRENT:AgentCore-Light

主机 → STATUS
设备 → STATUS:IDLE,FW:v0.5.13-jl-ota,BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,BLE_STATE:5,BLE_ADV:0,BLE_CONN:0

主机 → IDLE
设备 → STATE:IDLE
设备 → STATUS:IDLE,FW:...            (~100 ms 内主动推送)

设备 → CHG:1,VBAT:311,PER:0,CUR:100  (每 5 s)
```

`STORE:VM` = 这组值来自设备 Flash（VM），即已持久化。

---

## 5. 主机 → 设备 命令表

### 5.1 握手与查询

| 命令 | 响应 | 说明 |
| --- | --- | --- |
| `HELLO` | `HELLO:OK` | 串口/SPP 握手，1.5 s 超时 |
| `STATUS` | `STATUS:...` | 运行状态（灯效、亮度、蜂鸣器、BLE 状态） |
| `GET_SETTINGS` | `SETTINGS:...`（`SLEEP_SETTINGS:` 不保证随应答返回，见 §8） | 读设备 VM 配置 |
| `GET_BT_NAME` | `BT_NAME:CURRENT:<name>` | 读蓝牙名 |

### 5.2 灯效状态（核心控制）

直接发送状态名，无前缀、无参数：

| 命令 | 语义 |
| --- | --- |
| `IDLE` | 空闲 |
| `BUSY` | AI 工作中（上位机的 `THINKING`/`AI`/`WRITING`/`RUNNING` 归一到 `BUSY`） |
| `SUCCESS` | 成功（上位机 `DONE` → `SUCCESS`） |
| `ERROR` | 出错 |
| `WAIT_CONFIRM` / `CONFIRM` / `WAITING` / `WAIT` | 等待/待确认 |
| `OFF` | 关闭灯效 |

响应：`STATE:<state>`，随后一条 `STATUS:`。

### 5.3 硬件设置

| 命令 | 范围 | 响应 |
| --- | --- | --- |
| `BRIGHTNESS_LEVEL:<0-300>` | 亮度等级（百分比 × 3） | `STATUS:` |
| `BRIGHTNESS:<0-100>` | 旧版亮度百分比（等价 × 3） | `STATUS:` |
| `BUZZER:ON` / `BUZZER:OFF` | 蜂鸣器总开关 | `STATUS:` |
| `BUZZER_VOLUME:<0-100>` | 蜂鸣器音量 | `STATUS:` |
| `LOW_BATTERY_ALERT:<0\|1>` | 低电量待机提示音 | `STATUS:` |

设置生效后约 **1.1 s** 会再补一组 `SETTINGS:` + `SLEEP_SETTINGS:`（`STORE:VM`，表示已写入 VM）。

### 5.4 音效

```
SOUND:<NAME>:<once|loop>
```

设备回显**整条命令**（如 `SOUND:DONE:once`）。

可用音效名（固件内置 32 种）：

```
OFF  OK  DONE  POP  SOFT  UP  DOWN  ALERT  CHIME  CLASSIC
MELODY_SCALE  MELODY_JUMP  MELODY_COIN  MELODY_LEVELUP  MELODY_READY
MELODY_STAGE  MELODY_BONUS  MELODY_ARCADE  MELODY_ARCADE2  MELODY_CHIPTUNE
MELODY_GAMEOVER  MELODY_FANFARE  MELODY_HAPPY  MELODY_CALM  MELODY_ODE
MELODY_FRERE  MELODY_TWINKLE  MELODY_TURKISH  MELODY_FUR_ELISE
MELODY_MINUET  MELODY_PUZZLE  MELODY_RISE
```

厂商默认绑定：`SUCCESS → DONE:once`、`WAIT_CONFIRM/CONFIRM/WAITING/WAIT → CLASSIC:once`，其余 `OFF`。
音效与状态解耦：主机可在任意时刻单独触发。

### 5.5 蓝牙名

| 命令 | 响应 |
| --- | --- |
| `SET_BT_NAME:<name>` | 成功 `BT_NAME:OK:<name>`；格式非法 `ERR BT_NAME_FORMAT` |

约束（实测 + 厂商侧校验）：必须以 `AgentCore-Light-` 开头，后缀 1–15 个 `[A-Za-z0-9_-]`，总长 ≤ 31。
因此 `SET_BT_NAME:AgentCore-Light`（无后缀）会被拒绝。

### 5.6 省电参数

`SLEEP_SETTINGS:` 里的字段可以单独下发（**厂商上位机源码未使用，实测有效**）：

| 命令 | 说明 | 默认 |
| --- | --- | --- |
| `IDLE_SLEEP_SEC:<秒>` | 空闲多久进休眠 | 600 |
| `BT_DISCONNECT_OFF_SEC:<秒>` | 蓝牙断开多久后关灯 | 300 |

响应：`SETTINGS:` + `SLEEP_SETTINGS:`。
（`SLEEP:ON/OFF` 不是命令，只是上报里的当前休眠标志。）

---

## 6. 设备 → 主机 上报

| 报文 | 时机 | 说明 |
| --- | --- | --- |
| `HELLO:OK` | 收到 `HELLO` | 握手确认 |
| `STATUS:<STATE>,FW:<ver>,BRIGHTNESS_LEVEL:<0-300>,BUZZER:<ON\|OFF>,BUZZER_VOLUME:<0-100>,BLE_STATE:<n>,BLE_ADV:<0\|1>,BLE_CONN:<0\|1>` | 收到 `STATUS`；状态/设置变更后 ~50 ms 主动推送 | `BLE_STATE`: 串口→`5`（广播中）、SPP→`2`、BLE 已连→`33`；`BLE_CONN=1` 表示有 BLE 中心连着 |
| `STATE:<STATE>` | 收到状态命令 | 回显 |
| `SETTINGS:BRIGHTNESS_LEVEL:n,BUZZER:ON\|OFF,BUZZER_VOLUME:n,LOW_BATTERY_ALERT:0\|1,STORE:VM` | 收到 `GET_SETTINGS`；设置变更后 ~1.1 s | VM 权威配置 |
| `SLEEP_SETTINGS:IDLE_SLEEP_SEC:n,BT_DISCONNECT_OFF_SEC:n,SLEEP:ON\|OFF,STORE:VM` | 紧跟 `SETTINGS:` | 省电参数 |
| `BT_NAME:CURRENT:<name>` | 收到 `GET_BT_NAME` | 当前蓝牙名 |
| `SOUND:<NAME>:<mode>` | 收到 `SOUND:` | 回显整条命令 |
| `CHG:<0\|1>,VBAT:<0.01V>,PER:<0-100>,CUR:<mA>` | **每 5 s** | 充电中 / 电池电压 / 芯片百分比 / 充电电流目标 |
| `ERR UNKNOWN:<原命令>` | 未知命令 | 同时会补推一组 `SETTINGS:` |
| `ERR <FIELD>` | 字段越界/格式错 | 见下节 |

实测样例：

```
CHG:1,VBAT:311,PER:0,CUR:100    # 充电中 3.11 V，芯片报 0%（充电初期正常）
```

厂商上位机的电量换算（不在协议内，供参考）：`VBAT` 0.01 V 为单位，
2.80 V→0%、4.02 V→100% 线性映射，容量按 500 mAh，`CUR` 为充电电流目标。

---

## 7. 错误与校验

设备侧校验很简单，错误都走同一行文本：

| 输入 | 返回 |
| --- | --- |
| 未知命令 `HELP` | `ERR UNKNOWN:HELP` |
| `BRIGHTNESS_LEVEL:301` / `BRIGHTNESS:101` | `ERR BRIGHTNESS_LEVEL` / `ERR BRIGHTNESS` |
| `BUZZER:MAYBE` | `ERR BUZZER` |
| `BUZZER_VOLUME:101` | `ERR BUZZER_VOLUME` |
| `LOW_BATTERY_ALERT:2` | `ERR LOW_BATTERY_ALERT` |
| `IDLE_SLEEP_SEC:99999` | `ERR IDLE_SLEEP_SEC` |
| `SET_BT_NAME:AgentCore-Light` | `ERR BT_NAME_FORMAT` |
| 命令后附加 `,STORE:VM` | `ERR <FIELD>`（`STORE` 只能出现在上报里） |

注意：命令名本身不区分大小写；值里的布尔必须按各命令约定（`BUZZER` 用 `ON/OFF`，`LOW_BATTERY_ALERT` 用 `0/1`）。

---

## 8. 时序要求

| 场景 | 要求 |
| --- | --- |
| 打开 USB 串口 | 置 `DTR=1 / RTS=0`，等 1.5 s，清缓冲，再 `HELLO` |
| `HELLO` 应答 | 1.5 s 超时；收不到就换下一个串口候选 |
| 发送间隔 | 主机轮询 100 ms 一次；同一时刻只有一条命令在飞 |
| 写超时 | 3 s；连续 3 次失败视为断链并重连 |
| 反馈 | 设置类命令回 `STATUS:`；查询类回对应行；错误回 `ERR ...` |
| 设置类命令的应答节奏（实测） | `+0.05 s` 回 `STATUS:`；`+1.0 s` 再推 `SETTINGS:` + `SLEEP_SETTINGS:`（VM 落盘确认） |
| `GET_SETTINGS` | 只回 `SETTINGS:`；`SLEEP_SETTINGS:` 只在「设置类命令后」或「连接初期」偶尔出现，不能指望它随查询返回 |
| 想读休眠参数 | 发一条同值的设置命令（如当前的 `BRIGHTNESS_LEVEL:n`），等 ~1.1 s 收 `SLEEP_SETTINGS:` |
| 电量心跳 | 设备每 5 s 主动发 `CHG:`，主机不需要轮询 |

---

## 9. 固件升级入口

厂商上位机切下载模式的方式（**不要日常乱发**，会让固件退出并重启进 BootROM）：

```
串口: 115200 8N1, DTR=1, RTS=1, write_timeout=2 s
写入: b"U,1;"          # 分号结尾，不是换行
```

之后设备 USB 重枚举为杰理下载模式，由厂商下载器刷机；
刷机完成后上位机重新等 CDC 口出现，并用 `STATUS` 里的 `FW:` 校验版本。

---

## 10. 手动控制示例

```python
import serial, time
s = serial.Serial('/dev/ttyACM0', 115200, timeout=0.2)
s.dtr, s.rts = True, False
time.sleep(1.5)
s.reset_input_buffer()

def tx(line):
    s.write((line + '\n').encode())
    s.flush()
    time.sleep(0.15)
    print(s.read(s.in_waiting or 1).decode(errors='ignore').strip())

tx('HELLO')                 # HELLO:OK
tx('STATUS')                # STATUS:IDLE,FW:...
tx('BUSY')                  # STATE:BUSY
tx('BRIGHTNESS_LEVEL:120')  # 亮度 40%
tx('SOUND:CLASSIC:once')
tx('LOW_BATTERY_ALERT:1')
tx('IDLE')
```

或者用本仓库的测试工具（`scripts/agentcore_probe.py`，纯标准库；BLE 需要 `bleak`）：

```bash
# USB 串口（需要厂商后台服务已停止）
python3 scripts/agentcore_probe.py status              # HELLO + STATUS + SETTINGS
python3 scripts/agentcore_probe.py state BUSY
python3 scripts/agentcore_probe.py brightness 60
python3 scripts/agentcore_probe.py sound DONE
python3 scripts/agentcore_probe.py monitor 10          # 被动看 CHG/STATUS
python3 scripts/agentcore_probe.py demo                # 全流程演示并还原设置

# 厂商后台服务在跑（占用串口）→ 走它的本地 HTTP 透传
python3 scripts/agentcore_probe.py --service demo

# 蓝牙 BLE（NUS）：先拔掉 USB，让设备广播
python3 scripts/agentcore_probe.py --ble status
python3 scripts/agentcore_probe.py --ble demo
python3 scripts/agentcore_probe.py --ble --address AA:BB:CC:DD:EE:FF state BUSY

# 蓝牙 SPP：先配对，然后免 root 直连（通道 2，用系统 python3，uv 的解释器没有 AF_BLUETOOTH）
bluetoothctl pair AA:BB:CC:DD:EE:FF
/usr/bin/python3 scripts/agentcore_probe.py --spp AA:BB:CC:DD:EE:FF status
/usr/bin/python3 scripts/agentcore_probe.py --spp AA:BB:CC:DD:EE:FF demo

# 想当普通串口用（要 root 建 TTY）
sudo rfcomm bind 0 AA:BB:CC:DD:EE:FF 2
python3 scripts/agentcore_probe.py -p /dev/rfcomm0 status

# 只切换厂商服务用的链路（不改协议，验证它的蓝牙链路是否通）
python3 scripts/agentcore_probe.py --service transport bluetooth
python3 scripts/agentcore_probe.py --service transport serial
```

直连模式在检测到厂商服务运行时**默认拒绝**（Linux tty 允许多次 open，两个进程会抢读同一份字节），
确认要抢加 `--force`。

---

## 11. 已知限制与待验证项

已实测**否定**的猜测（不必再试）：

- `GET_SLEEP_SETTINGS`、`SET_SLEEP_SETTINGS:...`、`SLEEP:ON` → `ERR UNKNOWN`
- `STORE`、`SAVE`、`VER`、`BATT`、`HELP`、`INFO` → `ERR UNKNOWN`
- 命令尾附加 `,STORE:VM` → `ERR <FIELD>`

仍未知 / 推断：

- `BLE_STATE` 只采到 3 个值：串口链路 `5`、SPP 链路 `2`、BLE 已连接 `33`；其余未知。
- `IDLE_SLEEP_SEC` / `BT_DISCONNECT_OFF_SEC` 的合法上下界（`0`、`600`、`300` 接受，`99999` 拒绝）。
- `PER` 由电量计芯片给出，充电初期长期为 0，低电量保护策略未暴露。
- `once` / `loop` 在长音效上的实际行为（`loop` 未长时间验证）。
- 固件可能还有未文档化命令，可用「是否返回 `ERR UNKNOWN`」二分试探（读命令安全）。
- USB 连接时设备不广播 BLE/经典蓝牙，也没有协议命令能打开广播；蓝牙测试需拔掉 USB。
  缓存地址（如 `AA:BB:CC:DD:EE:FF`）只在设备可见时才能连上。
- SPP 已实测通过（通道 2，`--spp` 免 root）。`rfcomm bind` 的 root 需求只是"建 TTY 节点"这一步。
- 厂商 Linux 版上位机**不支持 BLE**（其 BLE 路径是 Windows WinRT / 蓝牙助手），
  Linux 上拔掉 USB 后只有 SPP（需先 `rfcomm bind`）可用。
