# Spec Delta

## Purpose

把聚合后的 agent 状态稳定地映射为物理灯效，并跨平台驱动不同型号的 USB 状态灯，包括设备发现、协议编码、时序保障与断线恢复。

## ADDED Requirements

### Requirement: 语义化灯效集合

系统 SHALL 对外暴露固定语义灯效：`off`、`calm`、`busy`、`attention`、`alert`。灯效含义与具体颜色/协议解耦，映射目标与手动控制都只能使用该集合中的值。

#### Scenario: 拒绝未知灯效

- **WHEN** 客户端请求手动控制时传入灯效 `purple`
- **THEN** 服务返回 400，并列出合法灯效，当前灯效不变

### Requirement: 默认状态映射

系统 SHALL 默认将聚合状态映射为：`none`→`off`、`idle`→`calm`、`working`→`busy`、`waiting`→`attention`、`error`→`alert`。映射 SHALL 可配置，且配置变更后对下一次状态变化立即生效。

#### Scenario: 状态驱动灯效

- **WHEN** 聚合状态从 `working` 变为 `error`
- **THEN** 已连接设备在 1 秒内切换到 `alert` 对应的物理灯效

#### Scenario: 自定义映射

- **WHEN** 用户把 `idle` 的映射改为 `off` 并保存
- **THEN** 之后聚合状态为 `idle` 时设备熄灭，直到下一次状态变化

### Requirement: 设备自动发现

服务 SHALL 周期性发现受支持的串口设备：MLight 匹配 VID `0x2E8A` 且 PID 为 `0x0003` 或 `0x000A`（排除 PID `0x101F`），YH50 匹配 VID `0x1A86` / PID `0x7523`。设备热插拔 SHALL 在 3 秒内反映到设备列表，未识别的串口设备 MUST NOT 被打开或写入。

#### Scenario: 热插入

- **WHEN** 插入一台 MLight 状态灯
- **THEN** 3 秒内设备列表出现该设备并标记为已连接，灯效按当前聚合状态下发

#### Scenario: 忽略无关串口

- **WHEN** 系统存在未被识别的串口设备
- **THEN** 服务不打开该端口、不写入数据，也不在设备列表中把它标为受支持设备

### Requirement: 设备型号与配置覆盖

系统 SHALL 按设备型号选择协议配置（profile）：`mlight-single`（单灯珠，如 `single-rgb-1003`）、`mlight-3led`（三灯珠）、`yh50-rgy-j`、`mock`。当自动识别不可靠时，用户 SHALL 能按 USB 序列号或端口名绑定 profile。

#### Scenario: 单灯珠组合命令

- **WHEN** profile 为 `mlight-single` 且灯效 `busy` 需要绿常亮叠加黄呼吸
- **THEN** 设备只收到最后一条动画命令 `YELLOW_BREATH`，不发送组合中的全部命令

#### Scenario: 三灯珠组合命令

- **WHEN** profile 为 `mlight-3led` 且灯效 `busy`
- **THEN** 设备先收到 `GREEN_ON`、间隔 300 毫秒后再收到 `YELLOW_BREATH`

### Requirement: MLight 协议编码

MLight 设备 SHALL 以 115200 8N1、ASCII、以 `LF`（`\n`）结尾的行发送命令。服务 MUST 只发送符合白名单的命令：`OFF`、`RED|YELLOW|GREEN` 与 `ON|OFF|BLINK|BREATH` 的组合、`BRIGHTNESS_0..100`，不得发送未知内容。

#### Scenario: 编码亮度

- **WHEN** 用户把亮度设为 60
- **THEN** 设备收到 `BRIGHTNESS_60\n`，且不会发送带 `\r` 的行结束符

### Requirement: MLight 时序保障

服务 SHALL 在状态切换前先发送 `OFF` 并等待至少 350 毫秒，再发送新灯效；组合命令逐条发送且间隔至少 300 毫秒；`mlight-single` 进入 `YELLOW_BREATH` 前先发 `YELLOW_ON` 并等待至少 80 毫秒；连续相同命令 MUST 被抑制，不重复下发。

#### Scenario: 切换时先清场

- **WHEN** 当前为 `GREEN_BREATH`，聚合状态要求切换到 `YELLOW_BREATH`
- **THEN** 设备先收到 `OFF`，至少 350 毫秒后才收到后续命令

#### Scenario: 抑制重复命令

- **WHEN** 聚合状态保持在 `working`，没有发生状态变化
- **THEN** 服务不重复发送相同灯效命令，呼吸动画不被打断

### Requirement: YH50 协议编码与传输

YH50-RGY-J 设备 SHALL 发送固定 5 字节帧 `0xFF | LED模式 | 蜂鸣器模式 | 闪光频率 | 0xAA`，并以二进制方式写入。Linux/macOS 上 SHALL 优先通过 USB Bulk 传输（config 1、interface 0、第一个 Bulk OUT 端点、1000 毫秒超时）发送；直接传输不可用时 MUST 回退到串口。Windows 上 SHALL 仅使用串口。

#### Scenario: 红灯闪烁

- **WHEN** 灯效 `alert` 映射为红灯 1.7 秒闪烁
- **THEN** 设备收到字节序列 `FF 04 01 03 AA`

#### Scenario: 直接传输失败回退

- **WHEN** Linux 上 USB Bulk 写失败，或设备接口被 ch341 内核驱动占用
- **THEN** 服务以 9600 8N1、1000 毫秒超时通过串口发送同一帧，且不报错退出

#### Scenario: 蜂鸣器开关

- **WHEN** 配置开启 `alert` 蜂鸣
- **THEN** 设备收到蜂鸣器字段为 `0x02` 的帧；关闭时该字段为 `0x01`

### Requirement: 断线与重连

串口写入失败或设备消失时，服务 SHALL 将设备标记为断开而不崩溃，并以 1 秒起步、上限 30 秒的退避重试。重连成功后 MUST 重放最后一次期望灯效；设备就绪前不得判定为已连接。

#### Scenario: 拔出重插

- **WHEN** 设备在 `alert` 状态下被拔出后重新插入
- **THEN** 设备先显示为断开，重连成功后自动恢复 `alert` 灯效，期间服务保持可用

### Requirement: 手动覆盖

系统 SHALL 允许手动设置灯效覆盖自动映射。覆盖具有时长（默认 60 秒，范围 1–3600 秒），到期后自动恢复按聚合状态驱动；覆盖期间聚合状态仍正常更新，覆盖可随时取消。覆盖可指定单个设备，未指定时作用于全部设备。

#### Scenario: 临时点亮

- **WHEN** 用户手动选择 `alert`、时长 30 秒
- **THEN** 设备立即显示 `alert`，30 秒后恢复为当前聚合状态对应的灯效

#### Scenario: 取消覆盖

- **WHEN** 覆盖期间用户点击「恢复自动」
- **THEN** 覆盖立即结束，设备在 1 秒内回到聚合状态对应的灯效

### Requirement: Mock 设备

系统 SHALL 提供无需硬件的 `mock` 设备，可由配置启用。mock 设备 SHALL 记录收到的灯效与命令序列，并通过设备查询接口暴露，供开发与自动化测试验证。

#### Scenario: 无硬件验证

- **WHEN** 配置启用 mock 设备并触发一次状态变化
- **THEN** mock 设备记录到对应命令，设备列表显示其 profile 为 `mock` 且已连接

### Requirement: 设备状态查询

系统 SHALL 提供 `GET /api/v1/devices`，返回每个设备的标识、端口、profile、连接状态、最近错误与当前灯效；`POST /api/v1/devices/scan` SHALL 触发一次立即扫描。

#### Scenario: 查看设备

- **WHEN** 客户端请求 `GET /api/v1/devices`
- **THEN** 返回设备数组，每项包含 `port`、`profile`、`connected`、`effect`、`last_error` 字段
