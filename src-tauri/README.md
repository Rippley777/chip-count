# Desktop integration

The packaged app embeds `chip-core` directly. No HTTP server, Node.js runtime, API key, or transcript upload is needed after installation. `chip-server` is only a loopback development bridge for Vite/browser inspection.

## Running and packaging

From the repository root:

```sh
npm run desktop
npm run desktop:build
```

The macOS build is an unsigned local `.app`; distribution signing and notarization require the distributor's Apple identity. Tauri uses native WebView dependencies on each target platform.

`CHIP_COUNT_DATA_DIR` overrides the local application-data directory. The default identifier is `labs.rippley.chip-count`; the SQLite filename is `chip-count.sqlite`. Desktop preferences live in SQLite, window geometry uses the window-state plugin, and `notified-alerts.json` retains notification identities across restarts. Demo data uses a separate in-memory SQLite engine and is never imported into production.

The main window can close to the tray. The tray exposes current production usage, open, compact monitor, monitoring pause/resume, and quit. On macOS, clicking the Dock icon also restores a hidden main window. The compact window loads the same application with `?compact=1` and stays above other windows.

## Native boundary

- `dispatch` runs the shared command envelope on a blocking worker.
- `monitoring` changes the watcher/poll worker's pause state; omit `paused` to read its current value without changing it.
- `desktop_settings` persists close-to-tray and notification preferences and changes launch-at-login registration through the native plugin.
- `save_export` receives content, opens a native save dialog, and writes only the chosen destination. A cancelled dialog returns `saved: false`.
- `reveal_path` canonicalizes and checks the requested path against known source and project roots, then opens the native file manager without a shell.
- `compact` creates or focuses the single compact window.

The background worker uses filesystem events for JSONL paths and a three-second reconciliation fallback. Events are coalesced; expensive work runs outside the UI thread. `index-updated` communicates `{ at, paused }`; `index-error` communicates an actionable error message. Core checkpoints, rather than watcher events, are authoritative.

Budget notifications are optional. The persistent core alert history determines which notifications can appear; a separate identity set prevents repeated native notifications. Existing alerts are not replayed at startup or when notifications are enabled later.

Production CSP permits bundled resources and Tauri IPC only. The development CSP additionally allows Vite's local HMR connection and inline development refresh bootstrap. Frontend permissions are restricted to index-event subscriptions, focus inspection, and file selection. The frontend has no general filesystem, shell, URL-opening, or notification permission.

## Development API

`cargo run -p chip-server -- --db /absolute/path/index.sqlite` serves `127.0.0.1:4319`. Routes are `GET /api/health`, `POST /api/dispatch`, and `POST /api/monitoring`. Mutations require JSON and all requests validate the loopback Host/Origin and cross-site browser metadata. The API exposes no save-file, reveal, shell, notification, or autostart operation. HTTP request bodies are bounded to 4 MB.

## Upstream references

Implementation checked against Tauri 2 documentation and the versions pinned by Cargo.lock:

- [Native menus](https://v2.tauri.app/learn/window-menu/)
- [System tray](https://v2.tauri.app/learn/system-tray/)
- [Dialog plugin](https://v2.tauri.app/plugin/dialog/)
- [Autostart plugin](https://v2.tauri.app/plugin/autostart/)
- [Capabilities](https://v2.tauri.app/security/capabilities/)

`icons/generate.py` regenerates the original chip-and-bars icon without third-party art or font dependencies.
