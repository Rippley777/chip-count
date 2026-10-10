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

## Scheduled API prices

The native `chip-pricing` worker downloads `https://models.dev/api.json` independently of session monitoring. It checks persisted refresh timestamps every minute: refresh after 24 hours since the last success, catch up at startup, and back off one hour after a failed attempt. A 30-second HTTP timeout and 16 MB response limit bound requests; redirects are rejected. HTTP runs outside the accounting mutex. Catalog validation completes before a single atomic database transaction updates current prices and version history. The core remains free of networking.

`pricing_refresh` invokes the same updater immediately; demo mode rejects live requests. `settings_save` accepts the boolean `pricing_auto_refresh` (default true, also for existing workspaces). Snapshots expose `pricing_refresh` with `last_attempt`, `last_success`, `error`, and `updated_models`. Local overrides and previously priced or unpriced events are retained. Long-context prices come from catalog tiers; bundled snapshots keep their existing rules. No changes to frontend CSP or sandbox entitlements are needed; HTTP stays in Rust and App Store builds already have network-client permission.

`chip-server --refresh-prices [--db PATH]` refreshes once without binding a server, then exits. Use it from system cron with the same database path as the desktop app. The normal development server runs the daily worker too.

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

## Mac App Store build

`npm run appstore:local` builds and ad-hoc signs a sandboxed test app with the separate identifier `labs.rippley.chip-count.sandbox-test`. It uses the same Rust `app-store` feature, embedded frontend, native picker, bookmark logic, export, and Finder APIs as distribution. This is a local acceptance build, not an App Store submission. The command checks the **signed** sandbox, user-selected file, and app-scope bookmark, and WebView network-client entitlements. Artifacts go into `artifacts/sandbox-local/`.

For distribution, install the application and installer certificates/private keys in Keychain and set:

- `APP_STORE_TEAM_ID`: your ten-character Apple Developer team ID.
- `APP_STORE_SIGNING_IDENTITY`: full `Apple Distribution: …` or `3rd Party Mac Developer Application: …` identity.
- `APP_STORE_PROFILE`: absolute path to a Mac App Store Connect `.provisionprofile` for `labs.rippley.chip-count`.
- `APP_STORE_INSTALLER_IDENTITY`: full `3rd Party Mac Developer Installer: …` or `Mac Installer Distribution: …` identity (packaging only).

Run `npm run appstore:build`, then `npm run appstore:package`. The build validates profile identity, team, expiry, and distribution type; generates team-specific entitlements; embeds the profile using Tauri's `bundle.macOS.files`; and verifies the resulting signature/entitlements. `npm run appstore:verify` rechecks the existing distribution bundle. The signed `.pkg` is in `artifacts/appstore/`. Upload via Apple's supported App Store Connect tools after completing acceptance. This path is separate from the Developer ID ZIP/notarization scripts. Profiles and generated signing files remain in ignored artifacts; never commit them.

The `tauri.appstore.conf.json` overlay enables the Rust feature and builds the frontend with `VITE_APP_STORE=1`. The script passes Cargo `--no-default-features` to exclude the LaunchAgent dependency. Even if someone enables both Cargo features, App Store code never registers the plugin and rejects launch-at-login mutations through either command boundary. Launch at login is disabled in the UI. No development HTTP server or external executable is shipped. The App Store feature always enables Tauri custom-protocol asset embedding, including local debug acceptance builds. WKWebView requires the network-client entitlement in the tested sandbox configuration; the production CSP still limits page connections to IPC and loads bundled assets. No network-server entitlement is granted.

Sandbox source access:

- `Engine::sandboxed` skips discovery and ignores `HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, and `CHIP_COUNT_DATA_DIR` as sources/permission. Data goes in the app's container.
- Native NSOpenPanel returns a read-only, app-scoped bookmark; IPC receives a one-use selection identifier and a display path. A typed path, stored path, imported configuration, or project path does not grant access.
- Bookmarks are stored privately in SQLite configuration, atomically with the source; they never appear in snapshots or exports. Startup resolves bookmarks without UI, starts access, renews stale bookmarks, and updates moved-root display paths. Failed/revoked/missing grants display an unavailable root with reconnect instructions.
- Arc-owned guards balance successful `startAccessingSecurityScopedResource` calls with `stopAccessingSecurityScopedResource`. Native scans and watchers retain guards; watchers unwatch before releasing old grants. Polling retries unavailable roots and reconciles missed changes after wake. Disabled/removed sources release grants after watcher teardown. Every JSONL read uses `File::open`; source logs are never opened writable.
- Export uses NSSavePanel's transient selected URL; it writes only while that URL is retained and rejects JSONL destinations and paths inside configured sources. The user-selected read/write entitlement is required for export; durable **source** bookmarks use `NSURLBookmarkCreationSecurityScopeAllowOnlyReadAccess`.
- Finder reveal uses NSWorkspace in-process on the main thread and requires an active root grant in addition to canonical validation. Project paths learned from log contents do not grant Finder access; select a containing source root if needed.

See [sandbox acceptance and reviewer instructions](../docs/APP_STORE_SANDBOX.md). Unit/browser checks alone do not prove macOS sandbox behavior.

References: [Apple sandbox file access](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox), [read-only bookmark creation](<https://developer.apple.com/documentation/foundation/nsurl/bookmarkdata(options:includingresourcevaluesforkeys:relativeto:)?language=objc>), [Tauri App Store packaging](https://v2.tauri.app/distribute/app-store/).
