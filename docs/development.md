# Development Guide

[Product README](../README.md) · [中文](development.zh-CN.md)

This guide covers the architecture, local development, testing, packaging, and contribution workflow for AgentBeacon.

## Architecture

```text
agent hooks / CLI
        │ HTTP
        ▼
  agent-beacon service
  ├─ session registry / aggregation
  ├─ semantic effect mapping
  ├─ device discovery + per-device actors
  ├─ HTTP API + SSE
  └─ embedded web console (127.0.0.1:6980)
        │
        ├─ USB status lights (MLight / YH50)
        └─ SSE ──▶ React web console / Tauri desktop app
```

### Repository layout

```text
crates/
  agent-beacon-core/     # domain models, config, sessions, effects
  agent-beacon-lights/   # discovery, protocols, transports, device actors
  agent-beacon/          # axum API, SSE, embedded web assets, CLI, worker
web/                     # React + Vite + Tailwind + Zustand console
desktop/src-tauri/       # Tauri desktop shell (splash, tray, service supervision)
docs/                    # protocols, integration guides, hardware checklist
scripts/                 # platform packaging and version helpers
.github/workflows/       # CI and tag-driven release
```

### Data flow

1. An agent hook or CLI sends an event to `POST /api/v1/events`.
2. The service updates the session registry and computes the aggregate state.
3. The configured mapping turns the aggregate state into a semantic light effect.
4. Each device actor expands the effect into protocol commands and writes to the device.
5. The web console receives snapshots over SSE and renders the current state.

## Prerequisites

- Rust 1.85+ and Cargo
- Node.js 22+ and pnpm 10+
- Platform dependencies:
  - **Linux**: `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libgtk-3-dev`, `libxdo-dev`, `patchelf`, `libudev-dev`, `libusb-1.0-0-dev`, `file`
  - **macOS**: Xcode Command Line Tools; no CH340 driver is required on macOS 10.14+
  - **Windows**: WebView2 and the WCH CH340 driver for YH50 serial fallback

## Local development

```bash
pnpm --dir web install

# terminal 1: service on 127.0.0.1:6980
pnpm dev:serve

# terminal 2: Vite dev server on 127.0.0.1:6973
pnpm dev:web
```

The Vite server proxies `/api` to the service. Open `http://127.0.0.1:6973`.

To run the desktop shell in development:

```bash
pnpm tauri:dev
```

## Build

```bash
pnpm build:web                 # build web assets
cargo build -p agent-beacon    # embed web/dist into the service
cargo run -p agent-beacon      # run the service
```

The service serves the embedded console at `http://127.0.0.1:6980`.

For the desktop app:

```bash
pnpm tauri:build
```

Or use the platform packaging scripts:

```bash
pnpm package:linux   # deb + AppImage
pnpm package:mac     # macOS dmg (macOS only)
pnpm package:win     # NSIS/MSI + portable zip (Windows Git Bash only)
```

## Tests

```bash
cargo test --workspace
pnpm --dir web test
```

Useful manual checks:

- `cargo run -p agent-beacon` starts the service.
- `agent-beacon status` and `agent-beacon devices` print English diagnostics.
- `agent-beacon integrate list` shows managed agent integrations.

## Configuration and API

Everything lives in one application data directory named after the bundle identifier `net.quadhub.agent-beacon`:

- Linux: `~/.local/share/net.quadhub.agent-beacon/`
- macOS: `~/Library/Application Support/net.quadhub.agent-beacon/`
- Windows: `%APPDATA%\net.quadhub.agent-beacon\`

`config.json` (owned by the service), the desktop shell's `desktop-prefs.json`, and `agent-beacon.log` all sit in that directory, so the CLI, the service and the app share one data root. Configs written by older versions under `~/.config/agent-beacon/` are copied over on first start.

The default listen address is `127.0.0.1:6980`. Configuration can be updated through the console or `PUT /api/v1/config`.

### HTTP API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/v1/health` | Service health |
| `GET` | `/api/v1/state` | Full state snapshot |
| `GET` | `/api/v1/stream` | SSE snapshots and updates |
| `GET` | `/api/v1/agents` | Known agents |
| `GET` | `/api/v1/devices` | Devices |
| `POST` | `/api/v1/devices/scan` | Trigger discovery |
| `POST` | `/api/v1/events` | Report an agent event |
| `GET/DELETE` | `/api/v1/hooks` | Read or clear recent hook/state-transition diagnostics |
| `DELETE` | `/api/v1/sessions` | Clear sessions |
| `GET` | `/api/v1/sessions?limit=N` | Recent sessions kept in memory (includes expired; never persisted) |
| `POST` | `/api/v1/control` | Apply a manual light effect |
| `POST` | `/api/v1/control/auto` | Restore automatic control |
| `GET/PUT` | `/api/v1/config` | Read or update configuration |
| `GET` | `/api/v1/integrations` | Agent integrations: detected, installed, and pending |
| `POST` | `/api/v1/integrations/settings` | Update `auto_install` / `onboarded` only |
| `POST` | `/api/v1/integrations/install-all` | Install every detected but unconnected agent |
| `POST` | `/api/v1/integrations/{target}/{install\|upgrade\|uninstall}` | Act on one integration |

### CLI

```bash
agent-beacon serve
agent-beacon report --agent pi --state working --session default
agent-beacon status
agent-beacon devices
agent-beacon hooks --limit 100
agent-beacon integrate list
agent-beacon integrate install pi
agent-beacon integrate install claude-code
agent-beacon integrate upgrade
agent-beacon integrate uninstall pi
```

The CLI help and diagnostics are English.

### Integration install mechanisms

`agent-beacon integrate install` supports the following strategies, dispatched from `Target::kind()` and `Target`-specific hook shapes:

| Kind | Targets | What is written |
| --- | --- | --- |
| File plugin | `pi`, `opencode` | A managed `.js` plugin with `@agent-beacon-integration` / `@agent-beacon-version` markers |
| Cordis patch | `dsh` | A JS hook plus a `BEGIN/END` block in `~/.dsh/cordis.patch.yml` |
| Plugin directory | `hermes` | A managed `plugin.yaml` + python module in `~/.hermes/plugins/agent-beacon/`, marked with `# @agent-beacon-integration` / `# @agent-beacon-version` |
| JSON hooks (nested) | `claude-code`, `qoder`, `codebuddy`, `workbuddy`, `github-copilot`, `gemini`, `qwen`, `iflow` | Commands appended to `settings.json` `hooks`, tagged with a trailing `# agent-beacon-hook:<target>:<version>` comment. `github-copilot` uses the same shape with camelCase event names |
| JSON hooks (named) | `antigravity` | A managed `agent-beacon` block in `~/.gemini/config/hooks.json`; tool events use `{ matcher, hooks }`, other events place the handler directly under the event |
| JSON hooks (dedicated file) | `grok` | A managed `~/.grok/hooks/agent-beacon.json` with a Claude-style `hooks` object |
| JSON hooks (process) | `zcode` | `{ type: "process", command, enabled, timeoutMs }` entries in `~/.zcode/cli/config.json`; the tag rides in `--marker agent-beacon-hook:<target>:<version>` because argv is not shell-parsed |
| Codex notify | `codex` | A `notify` array wrapped in `# >>> agent-beacon-hook:codex` comments in `config.toml` |

Installation is also available from the web console (`POST /api/v1/integrations/...`). `config.integrations.auto_install` makes the service install newly detected agents at startup; `config.integrations.onboarded` records that the first-launch prompt has been handled.

The JSON-hooks and Codex installers merge instead of overwriting: user-defined hooks and unrelated keys are preserved, and uninstall only removes tagged entries. `integrate install` without a target installs every target whose config directory exists.

All hook commands run `agent-beacon hook`, which reads the agent payload (stdin for JSON hooks, trailing argument for Codex notify), extracts the session ID / event name / tool name, derives the state when `--state auto` is used, and posts to `/api/v1/events`. It understands both Claude-style `session_id` / `tool_name` and Antigravity-style `conversationId` / `toolCall.name`, and always exits 0 so a missing service never blocks the agent.

### Diagnosing light changes

The service keeps an in-memory ring buffer of the latest 1000 hook events and state transitions. Every record includes the received event, whether it was accepted, and the aggregate/effect before and after the change. TTL expiry, manual overrides, config changes, and rejected payloads are recorded as well.

```bash
agent-beacon hooks --limit 100
agent-beacon hooks --limit 100 --json
curl -s 'http://127.0.0.1:6980/api/v1/hooks?limit=100'
curl -X DELETE http://127.0.0.1:6980/api/v1/hooks   # clear without restarting
```

Look for a short-lived `idle` record between two `working` records when the light flashes green during a run. The record's `agent/session`, source `message`, and exact millisecond timestamps tell you which hook sent it. Records are also written to the service log at `info` level with a `hook journal` message.

### State model

- Agent states: `idle`, `working`, `waiting`, `error`
- Session key: `(agent, session)`; when `session` is omitted it defaults to `{agent}:default`
- Default TTL: 900 seconds; an event can override it with `ttl_secs` (30–86400)
- Aggregate priority: `error > waiting > working > idle`; no sessions = `none`
- Default mapping: `none → off`, `idle → calm`, `working → busy`, `waiting → attention`, `error → alert`

## Hardware protocols

- [MLight serial protocol](MLight-serial-protocol.md)
- [YH50-RGY-J serial protocol](YH50-RGY-J-serial-protocol.md)
- [AgentCore-Light protocol (USB / Bluetooth SPP / BLE)](AgentCore-Light-serial-protocol.md)
- [PromLight protocol (BLE HID)](PromLight-ble-hid-protocol.md)
- [AgentPulse protocol (USB CDC / BLE)](AgentPulse-serial-protocol.md)
- [Hardware acceptance checklist](hardware-checklist.md)

Device discovery prefers USB serial numbers over port names, so bindings survive reconnects. Per-device profile overrides are available in Settings.

## Agent integrations

Integration assets live in `crates/agent-beacon/assets/integrations/`. The service can install, upgrade, and migrate managed plugin files for Pi, OpenCode, and DeepSeek Harness. See the [integration guides](integrations/) for agent-specific setup.

## CI and release

CI runs on pushes to `main` and pull requests:

- Rust workspace tests
- Web tests and production build
- Desktop compile check on Ubuntu, macOS, and Windows

The Release workflow is triggered by `v*` tags. It builds:

- Linux: `deb` and `AppImage`
- macOS: arm64 and x86_64 `dmg`
- Windows: NSIS, en-US/zh-CN MSI, and portable zip

A manual `workflow_dispatch` run builds and uploads artifacts without creating a GitHub Release.

### Releasing

1. Update versions in `Cargo.toml`, `package.json`, `web/package.json`, and `desktop/src-tauri/tauri.conf.json`.
2. Add the release entry to both `CHANGELOG.md` and `CHANGELOG.zh-CN.md`.
3. Commit the release changes.
4. Create and push an annotated tag:

   ```bash
   git tag -a v0.2.0-rc.1 -m "AgentBeacon v0.2.0-rc.1"
   git push origin v0.2.0-rc.1
   ```

The Release workflow verifies that the tag matches the manifests, extracts the bilingual changelog entry, and publishes a GitHub Release. macOS signing/notarization uses repository secrets (`MAC_CERT_P12_BASE64`, `MAC_CERT_PASSWORD`, `MAC_SIGN_IDENTITY`, `MAC_NOTARY_API_KEY_ID`, `MAC_NOTARY_API_ISSUER_ID`, `MAC_NOTARY_API_KEY_P8_BASE64`). Windows MSI uses a numeric ProductVersion even for prerelease versions.

## Contributing

1. Fork the repository and create a focused branch.
2. Keep changes scoped; add or update tests.
3. Run `cargo test --workspace` and `pnpm --dir web test`.
4. Use clear Conventional Commit-style messages where practical.
5. Open a pull request describing the behavior change and verification.

Before submitting, check that logs and CLI output remain English and that the UI still supports both Chinese and English and light/dark themes.

## License

Apache License 2.0. See [LICENSE](../LICENSE).
