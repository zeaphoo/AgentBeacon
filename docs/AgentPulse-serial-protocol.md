# AgentPulse 状态灯（USB 串口 / BLE）控制协议

> 本文档只描述**设备侧**协议（设备识别、传输层、帧格式、命令、上报、时序、电量、升级），
> 不涉及厂商桌面端的 hook 线协议、Web 控制台、多设备路由等上层内容。
>
> 依据：厂商桌面端发行包 `AgentPulseSetup-0.5.1.exe`（Inno Setup 6.4，innoextract master 解包）中的
> `app/daemon/dist/cli.js`（`src/integrations/device-serial.ts`、`src/integrations/firmware-update.ts`
> 等处）与 `app/daemon/ble-bridge/agent-pulse-ble-bridge.exe`（PyInstaller + `bleak`，pyinstxtractor 解包）；
> 以及随包固件 `agentpulse-esp32c3-next-0.2.0-26.bin` 的字符串与小端 UUID 交叉校验。
> 文中「推断」为尚未在真机逐条验证的结论。
>
> **AgentBeacon 内的实现**：尚未实现（规划中的 profile 名建议 `agent-pulse`）。
> 接入要点：USB 走普通 `serialport` + 行读取；BLE 走 **GATT 客户端连 NUS 服务**（不是 SPP），
> 设备广播名以 `AgentPulse` / `AgentPulse Next` 开头；状态帧与串口完全一致，只是换了承载。
> 注意 ESP32-C3 **只有 BLE，没有经典蓝牙**，不能像 AgentCore-Light 那样用 RFCOMM/SPP 兜底。

- 文档版本：v1.0
- 整理日期：2026-11-10

---

## 目录

1. [概览](#1-概览)
2. [设备识别](#2-设备识别)
3. [传输层](#3-传输层)
4. [帧格式](#4-帧格式)
5. [命令表](#5-命令表)
6. [设备上报](#6-设备上报)
7. [灯效语义与映射建议](#7-灯效语义与映射建议)
8. [事务与时序](#8-事务与时序)
9. [电池电量](#9-电池电量)
10. [固件升级（OTA）](#10-固件升级ota)
11. [完整示例](#11-完整示例)
12. [手动测试](#12-手动测试)
13. [与开源固件 v0.2.x 的差异](#13-与开源固件-v02x-的差异)
14. [已知限制](#14-已知限制)
15. [待验证项](#15-待验证项)
16. [参考资料](#16-参考资料)

---

## 1. 概览

AgentPulse 是一块 ESP32-C3 三色（红/绿/黄）状态灯，主机通过**行分隔 JSON 文本**
控制灯效，同一套应用层协议跑在 USB CDC 与 BLE 两条传输上。

| 项目 | 值 |
| --- | --- |
| 设备类型 | 三色 RGB LED（红/绿/黄）+ 蜂鸣器 + 三个物理按键 + 可选电池 |
| MCU | ESP32-C3（RISC-V，**仅 BLE 5.0，无经典蓝牙**） |
| 固件 | ESP-IDF，本文对应 `0.2.0` build `26`（`hardwareId` = `agentpulse-esp32c3-next`） |
| 传输 | USB CDC（虚拟串口）+ BLE GATT（Nordic UART Service 透传） |
| USB 标识 | VID `0x303A`（Espressif 原生 USB）/ PID `0x1001` |
| BLE 名称 | `AgentPulse`、`AgentPulse Next`（前缀匹配） |
| 帧格式 | UTF-8 JSON，一行一帧，`LF`（`\n`）结尾 |
| 通信方向 | **双向**：主机下发命令；设备上报设备信息、OTA 状态、电量、按键、配网结果 |
| 校验 | 帧级无校验和；OTA 用 `sha256` + 序号流控 |
| 端口名 | Linux `/dev/ttyACM0`；macOS `/dev/cu.usbmodem*`；Windows `COMx` |

---

## 2. 设备识别

### 2.1 USB

| 项 | 值 |
| --- | --- |
| VID | `0x303A`（Espressif） |
| PID | `0x1001`（AgentPulse） |
| 类别 | CDC-ACM，无需 DTR/RTS 操作 |

厂商桌面端还接受以下桥接芯片 VID 作为「可能是 ESP32」的兜底（clone 板/外置 USB 转串口），
但它们**不能唯一确定是 AgentPulse**，只适合做候选列表，自动绑定应优先 `303A:1001`：

| VID | 芯片 |
| --- | --- |
| `1A86` | WCH CH340 / CH341 / CH343 / CH9102 |
| `10C4` | Silicon Labs CP2102 / CP2104 |
| `0403` | FTDI FT232 / FT2232 |

端口名不稳（Windows 重插会变）时，绑定建议用 USB 序列号。

### 2.2 BLE

| 项 | 值 |
| --- | --- |
| 广播名 | `AgentPulse`、`AgentPulse Next`（厂商侧按小写前缀 `agentpulse` / `agentpulse next` 匹配） |
| 角色 | 设备是 GATT **Server / Peripheral**，主机做 Central |
| 配对 | 可选。厂商桌面端有「自动 / 配对」两种模式，非配对模式也能连 |

> 设备被连上后是否继续广播：**未验证**。厂商桥接器用「持续扫描 + 重试用绑定地址连接」两条路径，
> 实现时建议同时支持按地址直连与扫描发现。

---

## 3. 传输层

### 3.1 USB CDC

| 项 | 值 |
| --- | --- |
| 波特率 | `115200`（CDC 下不参与实际速率，但两侧仍按此配置） |
| 帧格式 | 8 数据位 / 无校验 / 1 停止位（8N1）/ 无流控 |
| 行结束 | `LF`（`\n`） |
| 单帧长度 | 无固件侧声明；厂商主机侧读取缓冲为 8192 字节，溢出时丢弃头部 |

### 3.2 BLE GATT

**透传服务**：Nordic UART Service（NUS）

| 角色 | UUID | 属性 |
| --- | --- | --- |
| Service | `6E400001-B5A3-F393-E0A9-E50E24DCCA9E` | — |
| RX（主机 → 设备） | `6E400002-B5A3-F393-E0A9-E50E24DCCA9E` | Write / Write Without Response |
| TX（设备 → 主机） | `6E400003-B5A3-F393-E0A9-E50E24DCCA9E` | Notify |

**OTA 专用服务**（与 NUS 分离，避免升级流量阻塞状态帧）

| 角色 | UUID | 属性 |
| --- | --- | --- |
| Service | `7D5A0001-7A0C-4B23-8C61-5D8D2A7E1001` | — |
| Control | `7D5A0002-7A0C-4B23-8C61-5D8D2A7E1001` | Write（JSON 控制帧） |
| Data | `7D5A0003-7A0C-4B23-8C61-5D8D2A7E1001` | Write（**二进制**，非 JSON） |
| Status | `7D5A0004-7A0C-4B23-8C61-5D8D2A7E1001` | Notify（`{"type":"otaStatus",…}`） |

这七个 UUID 在固件镜像里以 128 位小端形式各出现一次，可据此做强特征识别。

### 3.3 BLE 分片

- 接入后先读 MTU，按协商后的有效载荷切片写 RX。
- 主机侧对**单条命令**有 220 字节上限的断言（配网帧会校验），因此任何一帧都应控制在 220 字节内（**推断**：这是 BLE 单包可用载荷的安全值）。
- 厂商桥接器对 BLE 写入做节流（`BLE_WRITE_INTERVAL`，**数值未取证**），并在链路未就绪时**直接丢弃**命令而不是排队。
- 部分通知可能被 BlueZ 重复投递，**推断**与 AgentCore-Light 相同，建议按 `(时间窗, 内容)` 去重。

---

## 4. 帧格式

```
{"c":"g","m":"s","p":1000,"q":2000,"n":0,"u":0}\n
```

- 一行一个 JSON 对象，`\n` 结尾；**不是** CRLF。
- 键名被压到单字母，原因是 BLE 默认 MTU 下单包只有 20 字节有效载荷。
- 设备按 JSON 字段取值，不像 0.2.x 开源固件那样做子串匹配；字段顺序无关。
- 未识别字段忽略；未识别命令**静默丢弃**（不回错误帧），只能靠超时判定。
- 主机 → 设备用 `command` 字段，或裸布尔键（`identify` / `connectSeq` / `getBattery`）。
- 设备 → 主机用 `type` 字段区分消息种类。

---

## 5. 命令表

### 5.1 灯光状态帧

```json
{"c":"g","m":"s","p":1000,"q":2000,"n":0,"u":0}
```

| 键 | 含义 | 取值 |
| --- | --- | --- |
| `c` | 颜色 | `g` 绿 / `y` 黄 / `r` 红 / `o` 关灯 |
| `m` | 模式 | 见下表 |
| `p` | 闪烁周期 | ms，厂商默认 `1000` |
| `q` | 呼吸周期 | ms，厂商默认 `2000` |
| `n` | 音乐槽位 | `0` 静音 / `1..N` 内置音效 / `128..135` 自定义槽位 |
| `u` | 音乐重复 | `0` 单次 / `1` 循环 |

**模式码**

| 码 | 模式 | 灯效 |
| --- | --- | --- |
| `s` | solid | 常亮 |
| `b` | blink | 按 `p` 周期闪烁 |
| `h` | breathe | 按 `q` 周期呼吸（LEDC PWM） |
| `ry` | alternate-red-yellow | 红黄交替 |
| `yg` | alternate-yellow-green | 黄绿交替 |
| `rg` | alternate-red-green | 红绿交替 |
| `ryg` | cycle-red-yellow-green | 红→黄→绿循环 |

固件同时接受长名称（`"solid"`、`"blink"`、`"breathe"`、`"alternate-red-yellow"`、
`"alternate-yellow-green"`、`"alternate-red-green"`、`"cycle-red-yellow-green"`），
但厂商主机在 USB 与 BLE 两条路径上一律发短码。**实现建议：只发短码。**

### 5.2 亮度与声音设置帧

```json
{"g":100,"y":100,"r":100,"v":1}
```

| 键 | 含义 | 取值 |
| --- | --- | --- |
| `g` / `y` / `r` | 三色亮度百分比 | `0..100` |
| `v` | 声音总开关 | `0` 关 / `1` 开 |

持久化到设备 NVS，断电保持。`v=0` 时设备不播放任何音乐。

### 5.3 单布尔控制帧

| 帧 | 作用 | 是否回包 |
| --- | --- | --- |
| `{"identify":true}` | 定位设备：闪一下便于在多灯中辨认 | 否 |
| `{"connectSeq":true}` | 播放「已连接」颜色流水 | 否 |
| `{"getBattery":true}` | 请求电量 | `battery` |
| `{"command":"deviceInfo"}` | 请求设备信息 | `deviceInfo` |

### 5.4 关闭 / 断连提示帧

这两帧用**长键**（固件里的旧分支），不要与 §5.1 的短键帧混用：

```json
{"status":"off","mode":"solid"}
{"status":"red","mode":"blink","intervalMs":500}
```

- 前者：正常关灯。
- 后者：主机退出时留下的「失联」红灯闪烁。

### 5.5 音乐上传与播放

音符序列，每音符 **4 字节**：

```
偏移 0: uint16 LE  frequencyHz    // 0 = 休止符
偏移 2: uint16 LE  durationMs
```

限制：单槽 ≤ **512 字节**；单曲 ≤ 64 音符、总时长 ≤ 30 s、频率 ≤ 4000 Hz、
单音符时长 20..2000 ms、间隔 ≤ 500 ms。设备提供 **8** 个自定义槽位（线缆槽位号 `128..135`）。

上传分三步（`slot` 为 **0 基索引** `0..7`）：

```json
{"command":"musicBegin","slot":0,"sessionId":"up-abc123","size":512,"sha256":"<hex>"}
{"command":"musicChunk","slot":0,"sessionId":"up-abc123","offset":0,"data":"<base64>"}
{"command":"musicCommit","slot":0,"sessionId":"up-abc123","sha256":"<hex>"}
{"command":"musicAbort","slot":0,"sessionId":"up-abc123"}
```

- `musicChunk` 建议每包 **64 字节**（16 音符）；`offset` 必须按 4 字节对齐，
  不足一个音符的尾部会被丢弃。
- `sha256` 是完整二进制内容的十六进制摘要，`musicCommit` 时校验。

```json
{"command":"musicPlay","slot":0,"n":128}
{"command":"musicDelete","slot":0}
{"command":"musicCapabilities"}
```

`musicPlay` 里 `slot` 是 0 基索引，`n` 是线缆槽位号（`128 + slot`）。

### 5.6 Wi-Fi / MQTT 配网（仅 BLE）

「暂存 → 应用」两阶段，字段以 base64 传输，避免 JSON 转义问题：

```json
{"command":"lanStage","field":"ssid","value64":"<base64>"}
```

`field` 取值：`ssid`、`wifiPassword`、`host`、`username`、`password`。

```json
{"command":"lanApply","port":1883,"site":"home","deviceId":"ABC123","tls":false}
{"command":"lanStatus"}
{"command":"lanDisable"}
```

- `ssid` ≤ 32 字节，`wifiPassword` ≤ 64 字节，MQTT 用户名/密码 ≤ 128 字节。
- 单帧必须 ≤ **220 字节**，超长厂商侧直接拒绝。
- 每个 `lanStage` 回一帧 `lanConfig`；`lanApply` 后回最终结果。

> **注意**：随 0.5.1 分发的固件 `0.2.0-26` 里能看到 `lanStage`/`lanApply`/`lanStatus`/`lanConfig`
> 字符串，但**未见 `esp_wifi` / `esp_mqtt` 符号**，Wi-Fi 栈可能未编入该构建；配网前应先发
> `lanStatus` 探测，收到 `{"type":"lanStatus","ok":false,"error":"lan-disabled"}` 即视为不支持。

---

## 6. 设备上报

### 6.1 `deviceInfo`（能力协商）

```json
{"type":"deviceInfo","hardwareId":"agentpulse-esp32c3-next","firmwareVersion":"0.2.0","buildNumber":26,"protocolVersion":2,"ota":true,"maxImageSize":1048576,"otaChunkPayloadMax":244,"otaStatusEvery":1}
```

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `hardwareId` | string | 硬件型号，OTA 包匹配用 |
| `firmwareVersion` | string | 语义化版本 |
| `buildNumber` | int | 构建号，单调递增，OTA 用它判断是否有更新 |
| `protocolVersion` | int | 协议版本 |
| `ota` | bool | 是否支持 OTA |
| `maxImageSize` | int | OTA 分区可写最大字节数 |
| `otaChunkPayloadMax` | int | 单块最大负载（决定串口/BLE 分块大小） |
| `otaStatusEvery` | int | 建议每 N 块回报一次状态 |

> 旧固件（≤ `0.1.x`）的 `deviceInfo` 可能被 BLE 通知长度截断，厂商侧对以 `{"type":"deviceInfo"`
> 开头但 JSON 解析失败的负载做了截断恢复。新固件不依赖此行为。

### 6.2 `otaStatus`

```json
{"type":"otaStatus","phase":"receiving","written":20480,"total":582192,"nextSequence":205,"testOnly":false}
```

| `phase` | 含义 |
| --- | --- |
| `receiving` | 接收中，`nextSequence` 为期望的下一块序号 |
| `ready` | 接收完成，等待提交 |
| `test-complete` | 仅测试传输，未写 Flash |
| `rebooting` | 已写入，设备即将重启 |
| `failed` | 失败，附 `message` |

`nextSequence` 是**唯一的流控依据**：只有收到 `nextSequence >= 已发送块数` 才继续。

### 6.3 `battery`

```json
{"type":"battery","supported":true,"voltage":3.92,"percent":76,"charging":false,"adcMv":3980}
```

`supported:false` 表示该硬件无电量计。OTA 前主机必须检查电量，过低时设备以
`battery-too-low` 拒绝升级。

### 6.4 `btn`（物理按键）

```json
{"type":"btn","id":0}
```

`id` ∈ `0..2`，对应三个可编程按键。旧固件可能用 `t` 而非 `type`（`{"t":"btn","id":0}`），
解析时两者都要认。

### 6.5 `lanConfig`（配网结果）

```json
{"type":"lanConfig","ok":true,"enabled":true,"wifiConnected":true,"mqttConnected":true,"deviceId":"ABC123"}
```

### 6.6 `musicCapabilities`

```json
{"type":"musicCapabilities","builtinSlots":5,"customSlots":8,"slotMaxBytes":512}
```

字段集合未取证（见 §15）。

---

## 7. 灯效语义与映射建议

设备只执行颜色/模式，语义由主机定义。厂商侧的语义约定：

| 颜色 | 厂商语义 |
| --- | --- |
| 绿 | 智能体空闲 / 等待用户审阅 |
| 黄 | 智能体正在运行（工具调用或思考） |
| 红 | 阻塞 / 等待授权 / 需要用户介入 |

**建议的 AgentBeacon 状态映射**（与其它灯型保持同一状态模型）：

| AgentBeacon | 建议帧 | 说明 |
| --- | --- | --- |
| `off` | `{"c":"o","m":"s"}` | 关灯 |
| `calm` | `{"c":"g","m":"s"}` | 空闲 |
| `busy` | `{"c":"y","m":"h","q":2000}` | 呼吸黄，比闪烁更不刺眼 |
| `attention` | `{"c":"y","m":"b","p":800}` 或 `{"c":"r","m":"b","p":500}` | 等待确认，按用户偏好 |
| `alert` | `{"c":"r","m":"b","p":500}` | 错误 |

亮度用 §5.2 帧下发，注意它与灯效帧是**两条独立命令**，切换状态时不要重复下发设置帧
（会多一次写），只在设置变更或重连重放时发。

---

## 8. 事务与时序

协议**没有帧级 ACK**（OTA 除外），可靠性全靠主机侧时序：

| 场景 | 要求 | 来源 |
| --- | --- | --- |
| 串口打开后 | 等待 **300 ms** 再写第一帧 | 厂商 `openDevice()` |
| 每次写完 | `write()` → `drain()` → 等待 **200 ms** | 厂商 `pushStatus()` |
| 串口写失败（`EBADF` / `destroyed` / `not currently writable`） | 重开串口重试，最多 **3** 次，间隔 **1000 ms** | 厂商 `pushStatusWithRetry()` |
| 串口重连后 | 重放「设置帧 + 当前状态帧」 | 厂商 `restoreCurrentDeviceState()` |
| BLE 断开 | 桥接器 1 s 后重启并重新扫描连接；状态在连上后重放 | 厂商 `startBleBridge()` |
| BLE 未就绪时的新命令 | **丢弃**，不排队 | 厂商 `send_ble()` |
| 心跳 | 无设备心跳；主机用周期性状态重推维持链路，空闲 **15 s** 视为无交互 | 厂商 `BLE_INTERACTION_TIMEOUT_MS` |

相同状态重复下发：厂商侧会重推（用于保活），但**闪烁/呼吸动画会因重推而重置相位**。
实现时建议对「状态未变」的情况跳过重发，仅在保活周期到达时重发一次。

---

## 9. 电池电量

- 主动查询：发 `{"getBattery":true}`，设备回 §6.3 的 `battery` 帧。
- 厂商桌面端在 BLE 链路上周期性轮询（约每 15 s 一次交互窗口内）。
- `percent` 用于控制台展示与 OTA 前置检查（`battery-too-low`）。
- **推断**：`charging` 仅在带充电管理的硬件上有效，否则恒为 `false`。

---

## 10. 固件升级（OTA）

两条通道，命令同名但承载不同。

### 10.1 串口 OTA

```json
{"command":"otaBegin","hardwareId":"agentpulse-esp32c3-next","size":582192,"testOnly":false}
{"command":"otaChunk","data":"<base64>"}
{"command":"otaCommit"}
{"command":"otaAbort"}
```

- `data` 是 **base64(4 字节 LE 序号 + 负载)**，序号从 `0` 递增——串口靠它显式排序。
- 分块 **100 字节**；每 **16** 块等一次 `otaStatus`；块间延时 **20 ms**。
- 串口物理写入需按 **32 字节切片**、片间 **2 ms**，否则 CDC 缓冲会丢数据。
- 整体失败重试 **3** 次，间隔 **1000 ms**。
- `testOnly:true` 只做传输自检，不写 Flash，最终 `phase` 为 `test-complete`。

### 10.2 BLE OTA

控制帧走 `Control`（`7D5A0002`），镜像走 `Data`（`7D5A0003`），状态从 `Status`（`7D5A0004`）通知：

```json
{"command":"otaBegin","hardwareId":"agentpulse-esp32c3-next","size":582192,"testOnly":false}
{"command":"otaCommit"}
{"command":"otaAbort"}
{"command":"deviceInfo"}
```

- `Data` 写入**裸二进制**，没有 4 字节序号（顺序由 GATT 写入顺序隐式确定）。
- 分块大小 = `deviceInfo.otaChunkPayloadMax`，钳制到 `1..512`。
- 每 `deviceInfo.otaStatusEvery` 块等一次状态确认（默认 1，即逐块确认）。
- 写入前校验：镜像 ≤ `maxImageSize`、`buildNumber` 高于当前、电量足够。

**前置条件**：设备型号 `hardwareId` 必须与镜像匹配，否则拒绝；升级期间状态灯由固件接管。

---

## 11. 完整示例

**首次连接**

```
→ {"command":"deviceInfo"}
← {"type":"deviceInfo","hardwareId":"agentpulse-esp32c3-next","firmwareVersion":"0.2.0","buildNumber":26,"protocolVersion":2,"ota":true,"maxImageSize":1048576,"otaChunkPayloadMax":244,"otaStatusEvery":1}
→ {"g":100,"y":80,"r":100,"v":1}
→ {"identify":true}
→ {"c":"g","m":"s","p":1000,"q":2000,"n":0,"u":0}
```

**任务开始 → 结束（黄呼吸 → 绿常亮）**

```
→ {"c":"y","m":"h","q":2000,"n":0,"u":0}
← {"type":"battery","supported":true,"voltage":3.92,"percent":76,"charging":false,"adcMv":3980}
→ {"c":"g","m":"s","p":1000,"q":2000,"n":0,"u":0}
```

**BLE 覆盖升级**

```
→ (Control) {"command":"otaBegin","hardwareId":"agentpulse-esp32c3-next","size":582192,"testOnly":false}
← (Status)  {"type":"otaStatus","phase":"receiving","written":0,"total":582192,"nextSequence":0}
→ (Data)    <244 字节二进制> × N
← (Status)  {"type":"otaStatus","phase":"receiving","written":582192,"total":582192,"nextSequence":2386}
→ (Control) {"command":"otaCommit"}
← (Status)  {"type":"otaStatus","phase":"rebooting"}
```

---

## 12. 手动测试

### 12.1 串口（pyserial）

```python
import json, time, serial

ser = serial.Serial('/dev/ttyACM0', 115200, timeout=0.5)  # Windows: 'COM3'
time.sleep(0.3)                     # 设备就绪

def send(obj, wait=0.2):
    ser.write((json.dumps(obj, separators=(',', ':')) + '\n').encode())
    ser.drain()
    time.sleep(wait)

send({"command": "deviceInfo"})
print(ser.readline())               # 应打印 {"type":"deviceInfo",...}
send({"identify": True})
send({"c": "y", "m": "h", "q": 2000, "n": 0, "u": 0})
time.sleep(2)
send({"c": "g", "m": "s", "p": 1000, "q": 2000, "n": 0, "u": 0})
send({"status": "off", "mode": "solid"})
ser.close()
```

### 12.2 BLE（bleak，NUS）

```python
import asyncio, json
from bleak import BleakScanner, BleakClient

NUS_RX = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E"
NUS_TX = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"

async def main():
    dev = await BleakScanner.find_device_by_filter(
        lambda d, ad: (d.name or "").lower().startswith(("agentpulse", "agentpulse next")), timeout=10)
    print("found:", dev)

    async with BleakClient(dev) as client:
        def on_notify(_, data: bytearray):
            print("<-", data.decode(errors="replace").strip())

        await client.start_notify(NUS_TX, on_notify)
        for frame in ({"command": "deviceInfo"},
                      {"getBattery": True},
                      {"c": "r", "m": "b", "p": 500, "n": 0, "u": 0}):
            cmd = (json.dumps(frame, separators=(",", ":")) + "\n").encode()
            await client.write_gatt_char(NUS_RX, cmd, response=False)
            await asyncio.sleep(0.3)

asyncio.run(main())
```

> 若只有 Windows/macOS 厂商程序在跑，它会独占设备；测试前先退出其后台服务。

---

## 13. 与开源固件 v0.2.x 的差异

厂商开源仓 `lzty634158-oss/agent-pulse` 停留在 `0.2.2`（2026-06-29），
其 `firmware/agent-pulse-lamp/agent-pulse-lamp.ino` 是 **Arduino sketch**，与出货固件不兼容：

| 维度 | 开源 sketch (0.2.2) | 出货固件 (ESP-IDF 0.2.0-26) |
| --- | --- | --- |
| 框架 | Arduino | ESP-IDF |
| 传输 | 仅 USB 串口 | USB + BLE（+ 条件编译的 Wi-Fi） |
| 状态帧 | `{"status":"green"}` | `{"c":"g","m":"s",…}` |
| 解析 | 字符串子串匹配 `"status":"green"` | JSON 字段取值 |
| 方向 | 单向，无上报 | 双向（`deviceInfo`/`otaStatus`/`battery`/`btn`） |
| 显示 | `digitalWrite` 硬切换 | LEDC PWM，支持闪烁/呼吸/交替 |
| 升级 | 无 | OTA（串口 + BLE） |
| 音乐 | 无 | 音符序列播放 |

旧协议的三帧仍被出货固件接受（兼容分支），但新实现**不应使用**：

```json
{"status":"green"}
{"status":"yellow"}
{"status":"red"}
```

> 不要在 AgentBeacon 里用 `status` 字段做探测握手：它和 §5.4 的长键帧同属旧分支，
> 语义有限（只有三色 + 蓝灯兜底），无法表达模式与周期。

---

## 14. 已知限制

1. **无帧级 ACK**：非 OTA 命令发出去就没有确认，设备丢弃非法命令也不报错，只能靠超时。
2. **无心跳**：设备不主动上报在线状态；「连着」只能由传输层（端口存在/链路建立）判断。
3. **BLE 单包约束**：一帧必须小到能塞进一个 MTU（厂商断言 220 字节），大载荷（音乐、OTA）必须走分块通道。
4. **ESP32-C3 无经典蓝牙**：不能像 AgentCore-Light 那样用 SPP/RFCOMM 兜底，USB 不可用时只剩 BLE 与（可能的）Wi-Fi。
5. **配网依赖 BLE**：`lanStage`/`lanApply` 只在 BLE 链路有意义，串口下无作用。
6. **音乐槽位是「线缆号」与「索引」两套编号**：状态帧 `n` 用 `128+slot`，上传命令用 0 基 `slot`，容易写错。
7. **同名竞品**：npm 上的 `agent-pulse` 是无关项目；本设备的包名是 `agent-traffic-light-monitor`。

---

## 15. 待验证项

| 项 | 验证方法 |
| --- | --- |
| `\r\n` 是否兼容 | 发 `{"identify":true}\r\n`，看是否仍闪灯 |
| 未知命令行为 | 发 `{"command":"foo"}`，确认是否静默丢弃、链路是否保持 |
| 单帧最大长度 | 逐步加长填充字段，找到设备开始丢帧的阈值（串口 vs BLE 分别测） |
| 模式码数值表 | 固件里存在 `LIGHT_MODE_CODES` 常量但值未取证；用长名称逐条下发比对灯效，确认短码与长名称等价 |
| BLE 写入节流值 | 无间隔连发 `identify` 20 次，统计实际执行的次数与耗时 |
| `musicCapabilities` 响应字段 | 发 `{"command":"musicCapabilities"}` 抓原始通知 |
| `maxImageSize` 默认值 | 从 `deviceInfo` 读取，与 OTA 分区表比对 |
| Wi-Fi 是否编入 0.2.0-26 | 发 `{"command":"lanStatus"}`，看是 `lan-disabled` 还是正常状态 |
| 电池硬件是否存在 | 发 `{"getBattery":true}`，看 `supported` 是否为 `true` |
| 按键上报去抖 | 连按三下，观察 `btn` 帧数量与 `id` 顺序 |
| 多灯并存时 BLE 发现 | 两台设备同时广播时按名称前缀是否会选错（厂商按 RSSI 推荐） |

---

## 16. 参考资料

- 开源快照（0.2.2，Arduino 固件与旧协议）：<https://github.com/lzty634158-oss/agent-pulse>
- 发行仓（桌面端与固件二进制）：<https://github.com/lzty634158-oss/agent-pulse-release>
- 桌面端协议实现：`app/daemon/dist/cli.js` 的 `src/integrations/device-serial.ts`、`src/integrations/firmware-update.ts`、`src/core/music-protocol.ts`
- BLE 桥接器：`app/daemon/ble-bridge/agent-pulse-ble-bridge.exe`（PyInstaller `ble_bridge.pyc`，`bleak`，TCP 客户端连 `127.0.0.1:47801`）
- 固件镜像：`app/daemon/firmware-updater/agentpulse-esp32c3-next-0.2.0-26.bin`
- 同族协议对比：[AgentCore-Light](AgentCore-Light-serial-protocol.md)（同样 JSON/文本行 + NUS BLE，但额外有 SPP 兜底）、[PromLight](PromLight-ble-hid-protocol.md)（BLE HID 二进制帧）