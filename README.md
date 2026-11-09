<p align="center">
  <img src="web/public/logo.svg" width="88" alt="AgentBeacon logo" />
</p>

<h1 align="center">AgentBeacon</h1>

<p align="center">
  Sync coding-agent status to a USB status light.
</p>

<p align="center">
  <a href="https://github.com/zeaphoo/AgentBeacon/actions/workflows/ci.yml"><img src="https://github.com/zeaphoo/AgentBeacon/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/zeaphoo/AgentBeacon/actions/workflows/release.yml"><img src="https://github.com/zeaphoo/AgentBeacon/actions/workflows/release.yml/badge.svg" alt="Release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0" /></a>
  <a href="README.zh-CN.md">中文</a>
</p>

![AgentBeacon dashboard](docs/assets/dashboard.png)

AgentBeacon runs locally, watches the status reported by coding agents, and turns it into a simple light signal. It includes a Rust service, a React web console, and a Tauri desktop app.

## Features

- **Real-time agent status** — aggregate `idle`, `working`, `waiting`, and `error` sessions into one semantic state.
- **USB status lights** — MLight single/3-LED and YH50-RGY-J support, with hot-plug discovery and per-device profiles.
- **Device capabilities** — brightness and buzzer features are shown per device and per supported model.
- **Web console** — status overview, manual light control, automatic settings save, device bindings, and supported-device reference.
- **Desktop app** — splash screen, tray controls, launch at login, and supervised local service for Linux, macOS, and Windows.
- **Agent integrations** — installer-managed hooks and plugins for Pi, OpenCode, DeepSeek Harness, Claude Code, Codex CLI, Qoder CLI, and the Gemini CLI family (Gemini / Qwen Code / iFlow); any agent via CLI or HTTP.
- **Hardware-free testing** — optional mock device validates the full report → aggregate → light pipeline.
- **Hook diagnostics** — a bounded journal of hook events and state transitions, readable through `agent-beacon hooks` or `GET /api/v1/hooks`.
- **Bilingual and themeable UI** — Chinese/English, light/dark themes, system-language initialization, and system-following theme mode.
- **Local by default** — the service listens on `127.0.0.1:6980`; no cloud account or telemetry is required.

## Supported devices

| Profile | Connection | Brightness | Buzzer | Notes |
| --- | --- | :---: | :---: | --- |
| `mlight-single` | USB CDC | ✅ | — | Single-LED RGB status light |
| `mlight-3led` | USB CDC | ✅ | — | Three-LED RGB status light |
| `yh50-rgy-j` | USB serial / libusb | — | ✅ | Tri-color LED with buzzer |
| `mock` | Virtual | — | — | Development and hardware-free validation |

The web console displays the supported capabilities as feature badges on each connected device.

## UI

The main window has three sections:

- **Status** — current aggregate state, active sessions, connected devices, and live service uptime.
- **Manual control** — temporarily override the automatic light effect with a duration and target device.
- **Settings** — state-to-effect mapping, light behavior, device bindings, supported devices, and service information.

Language and theme selectors are available directly in the navbar. The status card uses the current state color as a subtle surface, with a larger icon and label for quick recognition.

## Quick start

1. Download the latest package from [Releases](https://github.com/zeaphoo/AgentBeacon/releases):
   - Linux: `.deb` or `.AppImage`
   - macOS: arm64 or x86_64 `.dmg`
   - Windows: NSIS installer, MSI, or portable `.zip`
2. Launch **AgentBeacon** (or run `agent-beacon` and open `http://127.0.0.1:6980`).
3. Plug in an MLight or YH50-RGY-J status light. The device is discovered automatically.
4. Install an agent integration:

   ```bash
   agent-beacon integrate install pi           # Pi
   agent-beacon integrate install opencode     # OpenCode
   agent-beacon integrate install dsh          # DeepSeek Harness
   agent-beacon integrate install claude-code  # Claude Code
   agent-beacon integrate install codex        # Codex CLI
   agent-beacon integrate install gemini       # Gemini CLI (also qwen, iflow)
   agent-beacon integrate install qoder        # Qoder CLI
   ```

   `agent-beacon integrate install` with no target installs every detected agent; `agent-beacon integrate list` shows installed, bundled, and available states.

   You can also do this from the console: the **Integrations** page lists every detected agent with its install state, one-click install/upgrade/uninstall, and an *install automatically* switch. On first launch the console offers to connect the agents it detected.

5. Open the console to inspect status, try manual lighting, or adjust settings.

For building from source, packaging, testing, and architecture details, see the [development guide](docs/development.md).

## Agent integrations

| Agent | Integration | Guide |
| --- | --- | --- |
| Pi | Extension | [Pi](docs/integrations/pi.md) |
| OpenCode | Plugin | [OpenCode](docs/integrations/opencode.md) |
| DeepSeek Harness | Cordis plugin | [DeepSeek Harness](docs/integrations/dsh.md) |
| Claude Code | `settings.json` hooks | [Claude Code](docs/integrations/claude-code.md) |
| Qoder CLI | `settings.json` hooks | [Qoder CLI](docs/integrations/qoder.md) |
| Gemini CLI · Qwen Code · iFlow | `settings.json` hooks | [Gemini family](docs/integrations/gemini.md) |
| Codex CLI | `config.toml` `notify` | [Codex CLI](docs/integrations/codex.md) |
| Other agents | `agent-beacon report` or `POST /api/v1/events` | [HTTP API in development guide](docs/development.md#http-api) |

The built-in integrations are shipped with the service and can be installed or upgraded from the CLI.

## Documentation

- [Development guide](docs/development.md) — architecture, prerequisites, build, test, release, contributing
- [中文文档](README.zh-CN.md)
- [Changelog](CHANGELOG.md) · [中文更新日志](CHANGELOG.zh-CN.md)
- [MLight protocol](docs/MLight-serial-protocol.md)
- [YH50-RGY-J protocol](docs/YH50-RGY-J-serial-protocol.md)
- [Hardware checklist](docs/hardware-checklist.md)

## Project status

This is a pre-release (`0.1.0-rc.1`). APIs, configuration fields, and packaging details may change before a stable release.

## License

Licensed under the [Apache License 2.0](LICENSE).
