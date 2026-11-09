# 更新日志

本项目的重要变更记录在此文件中。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

[English](CHANGELOG.md) | 中文

## [Unreleased]

### 新增

- 新增 Claude Code、Codex CLI、Qoder CLI 以及 Gemini CLI 家族（Gemini / Qwen Code / iFlow）集成，可用 `agent-beacon integrate install <target>` 安装。
- 新增 `agent-beacon hook` 命令：从 stdin 或参数读取 agent 的 hook 载荷，按事件名推导状态并上报。它始终返回 0，服务未运行也不会阻塞 agent。
- 集成安装器改为合并写入：`settings.json` 的 hooks 与 `config.toml` 的 `notify` 会带标记保留，升级时原地更新，卸载时只删除自己的条目，不影响用户已有配置。
- 新增 hook 诊断日志：`agent-beacon hooks`、`GET/DELETE /api/v1/hooks`，并在服务日志中输出 `hook journal`。

### 修复

- Pi 集成在扩展热重载时不再于运行中误报 `idle`；上报会携带 Pi session id，并按顺序发送（latest-wins）。

## [0.1.0-rc.1] - 2026-11-04

### 新增

- 新增 `agent-beacon` 服务：会话注册、状态聚合、语义灯效、HTTP API 与 SSE 推送。
- 支持 MLight 单灯珠/三灯珠和 YH50-RGY-J USB 状态灯，包括串口/libusb 传输、设备发现和每设备 actor。
- 提供 mock 设备，可在无硬件时验证「事件上报 → 聚合 → 设备命令」完整链路。
- 内置 Pi、OpenCode、DeepSeek Harness 集成，支持受管安装、旧文件迁移和自动升级。
- 新增 React Web 控制台，用于状态查看、手动灯光控制和服务配置。
- 新增 Tauri 桌面应用：启动页、托盘菜单、开机自启动和服务托管。
- 新增设备能力元数据（亮度、蜂鸣器）和设备特性徽章。
- 支持中文/English UI，首次启动按系统语言初始化，并可在导航栏切换。
- 支持浅色/深色主题、跟随系统主题和主题偏好持久化，状态色适配两种主题。
- 支持 Linux（deb/AppImage）、macOS（arm64/x86_64 dmg）和 Windows（NSIS、en-US/zh-CN MSI、便携 zip）发布打包。
- 新增 GitHub Actions CI 和 tag 驱动的 Release 工作流，自动生成 GitHub Release。
- 配置签名 secrets 后，macOS 发布产物会进行签名和公证。

### 变更

- MLight 亮度修改立即生效，并重放当前灯效命令而不插入 `OFF` 闪烁。
- MLight 默认亮度调整为 50%。
- 设置页改为 500ms 防抖自动保存，移除显式保存/重新载入按钮。
- 移除设置页的 mock 设备开关；仍可通过 `mock.enabled` 配置启用。
- 仪表盘聚合状态改为状态色 Hero 卡片：状态图标、低饱和背景和大号状态文字。
- 活动会话状态颜色统一按配置灯效映射显示；运行时长按秒/分钟/小时/天人性化展示。
- 服务信息移到设置页；移除 PID 和导航栏版本号。
- 导航名称改为「状态」，副标题改为「Agent 状态灯」。
- 服务日志、CLI help 和 CLI 输出统一为英文。
- 项目许可证从 MIT 切换为 Apache License 2.0。
- 文档拆分为产品 README 和开发文档，并提供中英文版本。

### 修复

- 修复 SSE 长连接导致 Ctrl+C/服务关停卡住的问题。
- 修复 Windows CI 的 LF 检出、MSYS 路径和桌面可执行文件检查顺序问题。
- Windows 打包现在同时生成 `en-US` 和 `zh-CN` MSI。
- 启用告警蜂鸣前增加响度与不适警告确认对话框。
- 统一聚合状态与活动会话的状态颜色，并实现服务运行时长实时显示。

[Unreleased]: https://github.com/zeaphoo/AgentBeacon/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.1.0-rc.1
