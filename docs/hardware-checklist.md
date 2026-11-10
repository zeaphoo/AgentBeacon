# 硬件验收清单

用于发布前在真机上逐项验证 MLight 与 YH50-RGY-J。

## 通用

- [x] 插入设备后 3 秒内出现在控制台「设备」列表
- [x] 设备端口、profile、连接状态正确
- [ ] 拔掉设备后显示未连接，重新插入后自动恢复并重放最后灯效
- [ ] 服务退出（SIGTERM）时灯效关闭
- [ ] 无硬件时不崩溃，`last_error` 可读

## MLight（`2E8A:0003/000A`）

- [ ] 自动识别，`0x101F` 温度显示设备被忽略
- [ ] `calm`：`GREEN_ON`
- [x] `busy`：三灯珠为 `GREEN_ON` + 300ms + `YELLOW_BREATH`；单灯珠只发 `YELLOW_ON` + 80ms + `YELLOW_BREATH`
- [ ] `attention`：`YELLOW_BLINK`
- [x] `alert`：`RED_BLINK`
- [ ] 状态切换前先 `OFF`，间隔 ≥350ms
- [ ] 相同灯效不重复下发，呼吸动画不重启
- [ ] 亮度设置下发 `BRIGHTNESS_n`（仅 `LF` 结尾）
- [ ] 在 1003 单灯珠上验证组合命令只保留最后一条
- [ ] 按协议文档 §9 验证：CRLF 是否兼容、未知命令行为、亮度是否分通道、去除 300ms 间隔是否丢命令

## YH50-RGY-J（`1A86:7523`）

- [x] Linux/macOS 自动走 USB Bulk；占用接口失败时回退串口（本次 ch341 占用，验证了串口回退）
- [ ] Windows 仅串口，COM 口正常打开
- [x] 帧字节正确：`calm FF 02 01 01 AA`、`busy FF 03 01 01 AA`、`attention FF 03 01 03 AA`、`alert FF 04 01 03 AA`
- [x] 蜂鸣按用户要求不启用，跳过该项
- [x] 不读取回显也不因回显阻塞
- [ ] 拔插后重连并重放最后灯效
- [ ] 验证闪光频率是否同时作用于蜂鸣器、上电默认状态、未知字段容错（协议文档 §9）

## AgentCore-Light（`4C4A:4155` / 蓝牙）

- [x] USB CDC 自动识别，`HELLO:OK` 握手后挂 actor
- [x] BLE（NUS）扫描发现，名称前缀命中，通知去重生效
- [x] **已连接（不广播）时仍能发现**：`known_links()` 从 BlueZ 直接列出，日志出现
      `detected AgentCore-Light from known devices (no scan)`
- [x] **SPP 免 root 直连**：`spp:AA:BB:CC:DD:EE:FF` 绑定后 `connected: true`，
      `POST /api/v1/control` 的效果能下发（`--example spp_probe` 单独验证）
- [ ] BLE 与 SPP 同一台设备同时可达时，只连 BLE（SPP 不抢经典蓝牙链路）
- [ ] 设备断开后 actor 自动重连（SPP 断开约 1.5 s 内重连会 `EBUSY`，退避应覆盖）
- [ ] 拔掉 USB 后 ≤15 s 内切到蓝牙链路；插回 USB 后切回 USB
- [ ] 两个实例同时跑时第二个实例的报错可读（`Operation already in progress`），不崩溃

## 跨平台

- [ ] Linux：`dialout` 权限、udev 规则（libusb 直连）
- [ ] macOS：10.14+ 免驱；安装过旧 WCH kext 的机器上不出现双端口冲突
- [ ] Windows：未装 CH340 驱动时给出可读错误，安装后正常
- [ ] 桌面应用：splash → 主窗口、托盘、开机自启动、退出清理

## 记录

| 日期 | 设备 | 系统 | 结果 | 备注 |
| --- | --- | --- | --- | --- |
| 2026-11-03 | MLight `2E8A:0003` / `5055207940A3461C` | Ubuntu 24.04 | MLight 通过 | profile `mlight-3led`；`working→busy`、手动 `alert`、取消覆盖后回 `busy`；串口无错误。 |
| 2026-11-03 | YH50-RGY-J `1A86:7523` / `/dev/ttyUSB0` | Ubuntu 24.04 | YH50 通过 | 串口回退路径（`fuser` 确认服务持有 ttyUSB0）；`idle/working/waiting/error` → `calm/busy/attention/alert`，清空会话 → `off`；无错误。肉眼确认灯效正常；蜂鸣按用户要求跳过；拔插、协议 §9 未测。 |
