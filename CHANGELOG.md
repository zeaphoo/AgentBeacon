# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

English | [中文](CHANGELOG.zh-CN.md)

## [0.2.0-rc.2] - 2026-11-18

### Added

- AgentCore-Halo profile (`agentcore-halo`) sharing the AgentCore-Light text protocol and distinguished by product name; the console shows the device-reported name, firmware, and battery.
- Device renders on the dashboard cards, device list, and *supported models* dialog; non-square assets keep their aspect ratio and unknown profiles fall back to a generic icon.
- A *show device images* switch in Settings (on by default); turning it off uses the generic icon on cards, lists, and the supported models dialog.
- The active-sessions card now shows the latest 7 rows and adds an in-memory session history with a detail dialog.
- A hook-log dialog on the Settings service card, filterable by agent and keyword; entries show the agent event name and raw payload.
- Google Antigravity and Grok Build integrations. Antigravity gets a managed `agent-beacon` block in `~/.gemini/config/hooks.json` (tool events use matcher groups, other events use direct handlers); Grok Build gets a dedicated `~/.grok/hooks/agent-beacon.json` with Claude-style events.
- Brand logos for CodeBuddy, WorkBuddy, GitHub Copilot, Hermes, ZCode, and the other integrations.

### Changed

- Device discovery now runs in a dedicated scan worker; offline devices are removed after a 3-minute grace period, and Bluetooth devices appear only after connecting.
- Battery and charging changes are broadcast to the console immediately instead of waiting for the next poll.
- App data moved under the shared application data root; the bundle ID is now `net.quadhub.agent-beacon`.
- Manual override duration is remembered locally by the console instead of coming from global config (see *Removed*).

### Fixed

- Charging is derived from a sustained voltage rise instead of the `CHG` pin; battery percentage is computed from voltage, and `CHG` is only shown as a charging-input flag, never as "USB connected" or "charging".
- Event-log and recent-session dialogs keep a minimum height when content is short.
- *Manual light control* is a single card now: the success banner and standalone *restore* button are gone. The status area is a panel tinted with the override effect: it reads *no manual control* plus an automatic hint when idle, or the effect, countdown, and an *end manual control* button when one is active.
- The dashboard override notice now says *all/some devices manually controlled* depending on scope, and its colour follows the overridden effect.

### Removed

- The `manual_override_default_secs` config field: the console now remembers the last used duration locally, and the backend uses a fixed 60-second default when `duration_secs` is omitted; stale fields in old configs are ignored.
- The mock device concept: the `mock` profile, `mock.enabled` config, mock command log, dashboard/settings references, and hardware-free mock actor are gone. Devices now come only from real discovery.

## [0.2.0-rc.1] - 2026-11-10

### Added

- Integrations page in the web console: every detected agent with its install state, one-click install/upgrade/uninstall, and an *install automatically* switch.
- First-launch prompt that offers to connect detected-but-unconnected agents; dismissing or connecting records `integrations.onboarded` so it never shows twice.
- `GET /api/v1/integrations`, `POST /api/v1/integrations/settings`, `POST /api/v1/integrations/install-all`, and `POST /api/v1/integrations/{target}/{install|upgrade|uninstall}`.
- New config field `integrations.auto_install` to install detected agents automatically at startup.
- Claude Code, Codex CLI, Qoder CLI, and Gemini CLI family (Gemini / Qwen Code / iFlow) integrations, installable with `agent-beacon integrate install <target>`.
- CodeBuddy, WorkBuddy, GitHub Copilot, ZCode, and Hermes Agent integrations. CodeBuddy/WorkBuddy/GitHub Copilot write Claude-style `settings.json` hooks (GitHub Copilot uses camelCase event names), ZCode writes `{ "type": "process" }` entries into `~/.zcode/cli/config.json`, and Hermes Agent gets a managed plugin directory (`plugin.yaml` + python module) under `~/.hermes/plugins/agent-beacon/`.
- `agent-beacon hook` command that reads agent hook payloads from stdin or an argument, derives the state from the event name, and reports it. It always exits 0 so a stopped service never blocks the agent.
- Integration installers now merge into existing configuration: `settings.json` hooks and `config.toml` `notify` are tagged, preserved on upgrade, and removed on uninstall without touching user-defined entries.
- Hook diagnostics journal with `agent-beacon hooks`, `GET/DELETE /api/v1/hooks`, and `hook journal` service log entries.
- Device protocol reference for AgentCore-Light (USB CDC / Bluetooth SPP / BLE NUS), plus a standalone probe script `scripts/agentcore_probe.py` for exercising it without the service.
- Hardware control protocol reference for PromLight (BLE HID: `5E 5E` binary frames, READ/WRITE/LED commands, ACK and error codes, persistent parameters, battery via BLE Battery Service).

### Fixed

- AgentCore-Light device support (`agentcore-light` profile): text-line protocol with `HELLO` handshake, `IDLE`/`BUSY`/`SUCCESS`/`ERROR`/`WAIT_CONFIRM` states, `BRIGHTNESS_LEVEL` (percent ×3) and buzzer mapping, plus a background reader that drains the device's `STATUS`/`CHG` reports.
- AgentCore-Light discovery now reads the devices **BlueZ already knows** before falling back to advertising scans (every 10 s, no BLE discovery triggered): a light that is already connected stops advertising, so scanning alone can never find it — and skipping discovery also avoids tearing down existing LE links.
- New **root-free SPP (RFCOMM) transport** for AgentCore-Light: `spp:AA:BB:CC:DD:EE:FF[:channel]` opens an `AF_BLUETOOTH/BTPROTO_RFCOMM` socket directly, with no `/dev/rfcomm*` node and no root, as the fallback when BLE is unavailable.
- `devices[{ port: "ble:…" / "spp:…" }]` is now honoured as a link binding; these ports are never enumerated by the system, so such bindings were previously ignored. BLE is preferred over SPP.
- New diagnostics: `--example bt_links` (known devices, no scan) and `--example spp_probe` (SPP handshake).
- AgentCore-Light BLE (Nordic UART Service) transport: discovered by advertisement name, driven with the same text protocol, MTU-chunked writes with a write-with-response fallback, duplicate-notification filtering, and a background notification reader.
- `ble_scan` / `ble_status` diagnostics under `crates/agent-beacon-lights/examples`.
- Automatic AgentCore-Light detection over USB CDC (`4C4A:4155`; Espressif `303A` also needs a product-name keyword) and Bluetooth SPP (bluetooth serial ports are verified with a one-off `HELLO` handshake before an actor is attached).
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

[0.2.0-rc.2]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.2.0-rc.2
[0.2.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.2.0-rc.1
[0.1.0-rc.1]: https://github.com/zeaphoo/AgentBeacon/releases/tag/v0.1.0-rc.1
