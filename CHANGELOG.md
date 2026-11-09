# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

English | [中文](CHANGELOG.zh-CN.md)

## [Unreleased]

### Added

- Hook diagnostics journal with `agent-beacon hooks`, `GET/DELETE /api/v1/hooks`, and `hook journal` service log entries.

### Fixed

- Pi integration no longer reports `idle` during an active run when extensions reload, and reports now carry the Pi session id and are sent in order (latest state wins).

## [0.1.0-rc.1] - 2026-11-04

### Added

- `agent-beacon` service with session registry, state aggregation, semantic light effects, HTTP API and SSE streaming.
- MLight single/3-LED and YH50-RGY-J USB status light support, including serial/libusb transports, device discovery and per-device actors.
- Mock device for hardware-free end-to-end validation, plus integration tests.
- Pi, OpenCode and DeepSeek Harness integrations with managed installation, migration and automatic upgrades.
- React web console for status, manual light control and service configuration.
- Tauri desktop app with splash screen, tray menu, launch-at-login and service supervision.
- Device capability metadata (brightness and buzzer) and feature badges in the UI.
- Chinese/English UI with system-language initialization and navbar language switching.
- Light/dark themes with system-following mode, persisted preferences and theme-aware status colors.
- Linux (deb/AppImage), macOS (arm64/x86_64 dmg) and Windows (NSIS, en-US/zh-CN MSI, portable zip) release packaging.
- GitHub Actions CI and tag-driven Release workflow with generated GitHub Release notes.
- Signed and notarized macOS release artifacts when signing secrets are configured.

### Changed

- MLight brightness changes now apply immediately and replay the current light state without an `OFF` flash.
- MLight default brightness is now 50%.
- Settings are auto-saved with a 500 ms debounce; explicit save/reload buttons were removed.
- The mock device switch was removed from Settings; mock remains available through `mock.enabled`.
- Dashboard aggregate state now uses a state-colored hero card with an icon and subtle tinted surface.
- Session status colors follow the configured effect mapping; runtime display uses humanized seconds/minutes/hours/days.
- Service information moved to Settings; PID and navbar version display were removed.
- Navigation label changed to “状态” / “Status” and the tagline to “Agent 状态灯” / “Agent status light”.
- Service and CLI logs, CLI help and command output are now English.
- The project license changed from MIT to Apache-2.0.
- Documentation was split into a product-focused README and a development guide, with English and Chinese versions.

### Fixed

- SSE long-lived connections no longer block Ctrl+C or graceful service shutdown.
- Windows CI checkout, MSYS path handling and desktop executable check order.
- Windows packaging now produces both `en-US` and `zh-CN` MSIs.
- Buzzer enablement now requires a warning confirmation dialog about loud sound and discomfort.
- Aggregate/session state color consistency and live service uptime rendering.

[Unreleased]: https://github.com/zeaphoo/AgentBeacon/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.1.0-rc.1
