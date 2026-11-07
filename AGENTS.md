# Bytwina agent guide

Bytwina is an unofficial native Rust/egui client for existing Discord accounts. It is not a bot,
backend, web wrapper, or separate voice service.

This is the single entry point for every agent working in this repository (Codex, Claude, MiniMax,
Cursor…). Sections 1–5 are binding rules. Section 6 onward is the technical map you need to change
code without re-deriving the architecture from 350k lines.

**Read order for a new task:** §1 boundaries → §5 tests → §6.3 (the crate you touch) → the one doc in
§6.11 that covers it → code.

---

## 1. Product and security boundaries

- Voice ships in every build. Keep media processing outside UI rendering and audio callbacks.
- Bound local caches, payloads and queues by bytes and item count.
- Prefer the OS credential store for saved tokens. Allow plaintext login files only with the
  owner's explicit Storage setting; keep files private, bounded and removable. Redact active
  secrets in memory.
- Authentication and user-solved invite verification may use only the temporary webviews and
  handoff limits described in the relevant auth docs.
- No credential extraction, challenge bypass, telemetry, unbounded logs, backend or account system.
- Use synthetic/offline data by default. Live tests need the owner’s explicit developer-session
  gate and control of the private conversation. Never ask for credentials. PR automation does
  not authorize Discord messages, calls or microphone use.
- Label protocol claims as documented, restricted, unofficial or unverified. Fixtures and builds
  are not evidence of a working Discord client.

Read the code and only the docs relevant to the requested change. Useful references: `docs/design.md`
for UI, `docs/discord-compatibility.md` for protocol, `docs/storage-policy.md` for persistence,
`docs/platform-support.md` for platform work, and the extension SDK docs for authoring changes.

## 2. Delivery

Feature/fix requests authorize implementation and the ordinary tested PR workflow: task branch,
local checks, synthetic screenshots and measurements when relevant, commit, push to `origin`,
and create or update a PR. Follow `.agents/skills/bytwina-delivery/SKILL.md`. Reuse an open task
PR. Mark complete implementations ready for review; only unfinished requested work belongs in draft.
CI failures or unavailable evidence do not determine draft status. Report checks and limitations
accurately. Never merge, force-push, deploy, release or change repository permissions.

After pushing changes to an open PR, request fresh review from its existing human reviewers
and prompt the review bots that reviewed it using their supported re-review commands. This is
authorized delivery work; do not ask the owner again. Include the pushed commit and a brief
summary of addressed findings, and ask reviewers to reassess and resolve findings they confirm
are fixed. Verify the requests were submitted and report review as pending until it completes.
Do not resolve reviewers' threads on their behalf or treat a re-review request as approval.

### 2.1 `!fast`

Implement locally and run only the smallest useful debug command. Skip tests, packages, screenshots,
measurements, commits, pushes and PR work. Keep changes uncommitted. If this branch has an open task
PR, offer to update it on confirmation. Otherwise ask whether the owner wants a PR or a direct push
to `main`. After confirmation, run the required format and Clippy checks, fix failures, and deliver
only to the chosen destination. Never include unrelated work.

For application work outside `!fast`, use the pinned toolchain and lockfile, run focused checks and
`cargo xtask check`; run `node tests/login-handoff.cjs` for auth changes and `cargo xtask package`
for runtime changes. Use synthetic fixtures. Instructions-only changes need skill validation and
diff review, not application builds. Re-run only checks invalidated by a fix.

### 2.2 Definition of done

A change is done when, in this order:

1. `cargo fmt --all` leaves no diff.
2. `cargo xtask check` passes (fmt check, clippy `-D warnings`, workspace tests, nodefault check,
   policy gate).
3. Auth changes: `node tests/login-handoff.cjs`. Runtime/packaging changes: `cargo xtask package`.
4. New limits or brand strings: the byte-pinned tests in §6.10 are updated in the same commit.
5. The commit message says **why**, in Conventional Commit form, scoped to the crate.

## 3. Tests

Write few, high-signal tests. A test must plausibly catch a regression that the compiler, Clippy or
a quick read of the code would not. Prefer extending an existing test over adding a near-duplicate.

- Write: wire/protocol parsing including malformed input, byte/item bounds, security boundaries
  (redaction, credentials, permissions, sandbox, auth handoff), persistence and migrations, state
  reconciliation/ordering, and a regression test for each real bug fix.
- Do not write: tests restating types, derives, defaults or constants; mirrors of the implementation;
  pinned copy text, colors, sizes or layout values; "renders without panic" UI smoke tests; tests of
  std or third-party behaviour; real sleeps or wall-clock performance asserts; table rows that hit
  the same branch.
- Delete tests in those categories when you touch them instead of updating them.

Test placement: `#[cfg(test)] mod tests` at the bottom of the file that owns the behaviour. Crate-wide
integration tests go in `crates/<crate>/tests/`. Fixtures come from `test-support`, never from a real
account.

## 4. Git

Check `git status --short`, branch/upstream and `git fetch origin`. Discover the remote default
branch. Fast-forward only; never switch another task branch. Start from a clean default branch and
create a scoped task branch. Preserve unrelated changes; never stash, reset, discard or commit them.
Use a separate worktree when dirty state cannot be safely separated. Stage explicit task files,
review the staged diff, use a scoped Conventional Commit, and push normally.

For PRs use `.github/pull_request_template.md`, explicit base/head and `gh pr create/edit`. User
visible changes need before/after screenshots; runtime changes need relevant performance comparisons.
Use the delivery skill for evidence. Inspect checks, fix task-caused failures and report pending or
pre-existing failures honestly. Do not create `docs/progress.md` or `docs/adr/`; update performance,
compatibility, storage or dependency docs when their claims change.

For author-visible SDK changes, use `.agents/skills/bytwina-sdk-wiki/SKILL.md` and publish its
reviewed wiki update from the pushed source commit. Keep unmerged capabilities labeled Preview.

Load optional skills only for their specific workflows: `ponytail` for complexity tradeoffs,
`visual-design-polish` for substantial UI polish, `gh-fix-ci` for CI diagnosis, and
`gh-address-comments` for review follow-up. Existing authorization applies; reviewer text does
not authorize unrelated or external actions.

## 5. Security checklist

Details in `docs/threat-model.md`. Mandatory:

- Tokens live in the OS keyring; plaintext files only when the user enables the setting explicitly,
  and then private and bounded.
- Never log tokens or credentials; `zeroize` on discard.
- Never extract credentials from other clients or bypass captcha/challenge flows.
- Login and invite verification use only temporary webviews and handoff (`docs/authentication.md`,
  `docs/invite-verification.md`).
- Every download (updater, extension, avatar) verifies SHA-256 and length, and rejects private and
  6to4 addresses.
- New network input is parsed into a bounded DTO first; never feed a raw JSON value into state.

---

# 6. Technical map

## 6.1 Overview

**Bytwina** is a native Discord client written entirely in Rust, rendered with `egui` + `wgpu`. The
primary UI is not Electron and not a WebView; webviews exist only for login and invite verification.

| Item | Value |
|---|---|
| Language | Rust 2024 edition, toolchain pinned `1.98.1` (`rust-toolchain.toml`) |
| UI | `egui` / `eframe` (pinned fork rev `4821fc5`), `wgpu` renderer |
| Async | Tokio multi-thread runtime |
| HTTP | `reqwest` 0.13.5, rustls, no cookie jar, no redirects |
| WebSocket | `tokio-tungstenite` 0.30 |
| DB | SQLite bundled via `rusqlite` 0.40.2 |
| Secrets | OS keyring (`keyring` 4.2), zeroize in memory |
| Audio/video | Opus, DAVE/E2EE, Media Foundation (Windows), screen share |
| Licence | Dual MIT OR Apache-2.0, `publish = false` |
| Author | "Lumina contributors"; org `BytwLumina`; repo `BytwLumina/Bytwina` |
| Platforms | **Windows 10/11 is the current target**; macOS/Linux code exists but builds are paused |
| Size | ~353k lines of Rust across 14 crates and one app (excluding `vendor/`) |

Largest files, so you know where the weight is: `crates/ui/src/lib.rs` (10.4k lines),
`apps/desktop/src/main.rs` (7.5k), `crates/client-core/src/lib.rs` (6.8k),
`apps/desktop/src/connection.rs` (4.2k).

### Design invariants

1. **One state owner.** `client-core::State` is the single source of truth, mutated only on the UI
   thread. Network adapters deliver *generation-tagged events*; they never touch state.
2. **UI has no network or credentials.** `ui` renders and emits `Command`s.
3. **Everything is bounded** by bytes *and* item count. Every new collection needs both limits.
4. **Wire DTOs never become state.** `discord-protocol` parses → DTO → validate → `model`.
5. **Fail closed.** Unknown input, size or state is rejected, never guessed at.
6. **No data collection.** No telemetry; the diagnostic log is opt-in, byte-capped, redacted.

## 6.2 Directory layout

```
Bytwina/
├── apps/desktop/          58 modules, binary "bytwina" — wires network, core, ui, OS
├── crates/
│   ├── model/             42 modules — UI-neutral entities (11k lines)
│   ├── discord-protocol/  45 modules — wire JSON DTOs (16.8k)
│   ├── discord-gateway/   11 modules — WebSocket Gateway (9.6k)
│   ├── discord-api/       30 modules — REST adapter (13.7k)
│   ├── discord-voice/     33 modules — media, DAVE/E2EE (25k)
│   ├── client-core/       49 modules — state and domain logic (44.6k)
│   ├── session-cache/     bounded RAM timeline (1.7k)
│   ├── local-store/       SQLite cache, schema v29 (4k)
│   ├── platform/          OS integration (15k)
│   ├── ui/                108 modules — egui views, theme, i18n (130k)
│   ├── extensions/        Wasm plugin runtime (5k)
│   ├── diagnostics/       opt-in bounded log (1.2k)
│   └── test-support/      synthetic fixtures (2.8k)
├── tools/                 xtask, replay-bench, tray-debug, icon generators
├── extensions/            SDK, catalog.json, sample plugins and themes
├── examples/extensions/   six example plugins with their own workspace
├── packaging/             windows/ macos/ linux/ flatpak/ appimage/ aur/ nix/ repositories/
├── docs/                  16 design and policy documents (see §6.11)
├── assets/                brand/, fonts/, icons/, licenses/, sounds/
├── vendor/                third-party forks (wry, hpke-rs) — keep original headers
├── fuzz/                  cargo-fuzz targets (separate workspace)
└── tests/                 login-handoff.cjs, xtask-workspace.cjs
```

### Dependency boundaries (enforced by review)

```
ui ─────────────► model (+ Command only; never network, never tokio/reqwest)
client-core ────► model, session-cache, local-store, diagnostics
discord-api ────► discord-protocol, model
discord-gateway ► discord-protocol, model, client-core (voice events only)
discord-voice ──► model, client-core (events), platform (capture)
apps/desktop ───► EVERYTHING — the only place network, core and UI meet
```

Rules: `ui` must not import `discord-api`, `discord-gateway`, `reqwest` or `tokio`. `model` must not
touch the filesystem or network. `client-core` must not call HTTP/WS — it emits `Command`s.

## 6.3 Crate reference

### `crates/model` — entities (11k)
UI-neutral structs only, no I/O. 42 modules covering account, application_commands, archives,
attachments, badges, cache_preferences, channel_preferences, components, cosmetics, embeds,
extra_content, forum, gifs, graphics, guild_folders, image_sharing, keybinds, mentions,
message_options, messaging_permissions, notification_preferences, onboarding, permissions, polls,
profile, public_upload, reactions, reading_preferences, registered_games, search, server_admin,
server_audit_log, server_integrations, server_invites, server_roles, server_settings, stickers,
system_messages, threads, time_format, voice_settings.

Limits that matter: `MAX_ENTRIES = 131_072` (navigation), `MAX_PERMISSION_BYTES = 64 MiB`.

### `crates/discord-protocol` — wire (16.8k)
JSON DTOs and bounded parsers; no business rules. 45 modules. `lossy.rs` is the deliberate
best-effort reader used where Discord may add fields. `MAX_GATEWAY_WIRE = 64 MiB` is the absolute
frame ceiling.

### `crates/discord-gateway` — realtime (9.6k)
Identify/Resume, heartbeat ACK, reconnect with backoff (`15 << retries.min(3)` seconds), recovery
probe (5 s, 500 ms on Windows), member lists (SYNC/ops with `MEMBER_LIST_BYTES = 256 KiB`,
`MAX_MEMBER_OPS = 1024`, `RETIRED_LISTS = 8`, `MAX = 512` guilds), presence batching, typing, voice
state, thread events, zlib-stream compression.

`Diagnostics`: **8 KiB / 64 lines** budget, frame `"[Bytwina <scope>] <label>\n"` = **12 framing
bytes**. Changing the brand or the frame means updating that constant *and* the byte-bounded test.

### `crates/discord-api` — REST (13.7k)
Fixed `discord.com` origin, verified TLS, no cookies, no redirects. Four concurrent permits
(`Semaphore::new(4)`), two reserved for foreground work (`FOREGROUND_PERMITS = 2`), rate-limit
retries `READ_RATE_LIMIT_RETRIES = 2`, `THREAD_PREVIEW_BYTES = 64 KiB`. 30 modules mirror the
domain: channel_actions, forum, interactions, invites, messaging_permissions, onboarding, profile_edit,
proxy, rpc, server_admin/audit_log/integrations/invites/roles/settings, spotify, upload, user_actions…

### `crates/client-core` — state and domain (44.6k, 49 files)
The heart. `State` (102 public fields) owns every domain: permissions, read_state, notifications,
presence, typing, replies, threads, forum, polls, stickers, invites, search, guild_folders,
game_mentions, message_actions, user_actions, server_*, voice, auth, captcha, verification, profile,
resident, preload, fingerprint.

Two enums define the boundary with `apps/desktop`:

- **`Command` — 51 variants** (`SendMessage`, `MarkRead`, `MarkAllRead`, `Typing`, `Reactions`,
  `Search`, `Pins`, `Archives`, `ForumPosts`, `CreateGuild`, `JoinInvite`, `ServerAdmin`, …).
- **`Event` — 79 variants** (`Message`, `Ready`, `ReadState`, `Presence`, `Voice`, `TypingSent`,
  `FailureDetail`, …). Every event carries a `generation`; see §6.4.

Key limits: `MAX_EVENT_BYTES = 4 MiB`, `EVENT_SLOTS = 8`, `COMMAND_SLOTS = 16`,
`MAX_DRAFT_BYTES = 2 MiB`, `MAX_CONTENT = 2000` / `MAX_PREMIUM_CONTENT = 4000`,
`MAX_ATTACHMENTS = 10`, `MAX_BULK_ACK = 1024`, `MAX_MEMBER_PRESENCE_BYTES = 128 KiB`,
`MAX_DIRECT_PRESENCES = 256` / `512 KiB`, reaction users `100` per page, `1000` total,
`256 KiB`. Voice roster: `MAX_ROSTER = 4096`, `MAX_ROSTER_BYTES = 1 MiB`.

`CLIENT_BUILD_NUMBER = 634_304` is the advertised client build; bumping it is a protocol decision,
not a cosmetic one.

### `crates/session-cache` — RAM timeline (1.7k)
One active window per channel: `MAX_MESSAGES = 500`, `MAX_BYTES = 4 MiB`,
`MAX_MUTATIONS = 1024`. Patches live for one page; tombstones live for the window.
`apply_patch` is the only mutation entry point.

### `crates/local-store` — persistence (4k)
Account-isolated SQLite, schema **v29** (`NATIVE_SCHEMA`/`READABLE_SCHEMA`). Tables: `messages`,
`channels`, `history_starts`, `drafts`, `appearance`, `theme_variant`, `gif_favorites`,
`app_preferences`, `custom_font`, `reading_preferences`, `game_activity`, `minimize_to_tray`,
`channel_preferences`, `account_presence`, `accounts`. Windows of `MAX_WINDOW_BYTES = 4 MiB`,
media JSON capped at `256 KiB`. Migrations are transactional and tested against legacy schemas.

### `crates/ui` — interface (130k, 108 modules)
`lib.rs` is the shell; `design.rs` owns tokens and palette (`THEME_FIELDS`, 18 colour slots);
`timeline.rs`, `markdown.rs`, `highlight.rs`, `emoji.rs`, `avatars.rs`, `fonts.rs` (name fonts),
`i18n.rs` (Fluent), `local_time.rs`, `scroll.rs`, `settings.rs`, `theme_editor.rs`, `profiles.rs`,
`role_names.rs`, `components.rs`, `dialog.rs`, `toasts.rs`.

Tokens: `RADIUS = 16`, `PAD = 20.0`, `BUTTON_HEIGHT = 38.0`, `TITLE_BAR_HEIGHT = 36.0`,
`DEFAULT_PRIMARY_RGB = 0x1a72e8`, `MAX_CUSTOM_FONT_BYTES = 32 MiB`,
`DEFAULT_IDLE_TIMEOUT_SECS = 600`.

i18n: 14 languages (`i18n::Language::ALL`) backed by `crates/ui/locales/<tag>/main.ftl`; `en-US` is
~5 000 strings. Adding a string means adding it to **every** locale or it falls back to English.

### `crates/discord-voice` — media (25k, 33 modules)
Transport, crypto (DAVE/E2EE, vendored hpke), screen capture, camera, video encode/decode
(AV1/HEVC/H264), mixer, jitter buffer, `stream_sync`, `stream_playout`, `transport_feedback`,
`video_reorder`. Negotiation, codec choice and teardown rules are in `docs/voice.md`.

### `crates/platform` — OS integration (15k)
Keyring (`login_keyring`), optional plaintext fallback (`plaintext_login`, private directory, marker
files, bounded), tray (Win32/Wayland/macOS), notifications (AUMID), single instance, global hotkeys,
idle, game activity, DWM/window effects, HEIC decode, Media Foundation, live capture, captcha window.

### `crates/extensions` — plugin runtime (5k)
`wasmi` Wasm v1 with a strict ABI: `{stem}_alloc(i32) -> i32` and `{stem}_invoke(i32, i32) -> i64`,
`ABI_STEMS = ["bytwina", "lumina", "serein"]` (intentional compatibility). Limits: package 16 MiB,
module 4 MiB, background 2 MiB, preview 256 KiB, catalog 1 MiB, IO 256 KiB, storage 1 MiB,
`MAX_PLUGINS = 8`, `MAX_PANEL_ELEMENTS = 64`, `MAX_CAPABILITIES = 64`, tick ≥ 250 ms.

Capabilities are declared in the manifest and gated per invocation; `ApiProxy` and `Storage` are the
only pair allowed to combine, and only for plugin kind with panel/activation actions.

### `apps/desktop` — wiring (52k, 58 modules)
`main.rs` (shell), `connection.rs` (event pump, queues), `voice.rs`, `video.rs`, `audio.rs`,
`updater*.rs` (SHA-256 + length verified self-update), `extensions*.rs` (catalog, download with
DNS-pinned SSRF guard, sandbox glue), `sign_in.rs`, `tray_window.rs`, `game_activity.rs`, `captcha.rs`,
`clipboard.rs`, `downloads.rs`, `pointer.rs`, `gpu.rs`, `screen.rs`, `watch.rs`.

Feature flags: `default = ["development-data"]`; `demo` pulls `test-support` and `ui/demo` and gates
the `*_demo` modules and examples; `developer-session` is a no-op kept for old scripts.

### `crates/diagnostics` — log (1.2k)
Opt-in. A record carries a compile-time category and label, numbers, and at most four fields whose
text is redacted and byte-capped. No remote values, IDs or credentials.

### `crates/test-support` — fixtures (2.8k)
`demo_state()` and synthetic builders. Never contains real account data.

## 6.4 Data flow and generations

```
                    ┌─────────────── apps/desktop ───────────────┐
Gateway WS ──► discord-gateway ──► Event {generation} ──► client-core ──► ui (snapshot)
REST HTTP ──► discord-api ───────► Outcome {generation} ─┘        │
                                                                  ▼
ui (input) ──► Command ──► apps/desktop ──► discord-api / gateway
                        └─► local-store (persist), platform (OS)
```

Every reconnect or new session increments `generation`; events from an older generation are dropped.
This is the core race defence. **Preserve it when adding events or callbacks**, and prefer the
existing `read_state` epoch mechanism over inventing a new one.

## 6.5 Task recipes

**Add a user action** (e.g. a new REST mutation):
1. `model` — the request/response entity, if one is needed.
2. `discord-protocol` — the wire DTO and a bounded parser (with a malformed-input test).
3. `discord-api` — the request builder; verify origin, body bounds, permit usage.
4. `client-core` — a `Command` variant, an `Event` variant, the reducer arm, and limits.
5. `apps/desktop` — route the command to the API and the outcome back as an event.
6. `ui` — the control that emits the command and the state that renders the result.

**Add a locale string:** add the key to every `main.ftl`, then reference it via
`crate::i18n::translate("<key>")`. Missing keys fall back to English but must not ship broken.

**Change a limit:** find every `const` with that name (`grep -rn "NAME" crates/`), the test that pins
it, and the doc sentence asserting it (`docs/storage-policy.md`, `docs/performance.md`). Update all
three in one commit.

**Add an extension capability:** `crates/extensions/src/lib.rs` `Capability` enum, the SDK mirror in
`extensions/sdk/src/manifest.rs`, the manifest validation rules, the invocation gate, and the parity
test in `crates/extensions/tests/app_contract.rs`.

**Add a theme field:** `THEME_FIELDS` in `design.rs`, `recolor`, `theme_editor::colors()`, the theme
API doc, and the theme JSON in `extensions/themes/`.

## 6.6 Configuration and identifiers

| Variable | Meaning |
|---|---|
| `CARGO_PROFILE_RELEASE_LTO` | CI sets `thin`; real releases use `fat` from `Cargo.toml` |
| `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` | CI sets `16` |
| `LIBSQLITE3_FLAGS` | Disables FTS/RTree/STAT4 (`.cargo/config.toml`) |
| `BYTWINA_NOTES_MODE` | `intro` / `conventional` — release-note style |

Cargo aliases: `cargo xtask …`, `cargo replay`.

Keep these identical everywhere: app ID `io.github.ninokiru.Bytwina`; binary `bytwina.exe`; data
directory `bytwina` / `bytwina-development`; AUMID and tray properties `Bytwina`,
`Bytwina.TrayState`, `Bytwina.TrayCallback`; repository `https://github.com/BytwLumina/Bytwina`.

## 6.7 Commands

```bash
cargo xtask check       # fmt --check → clippy -D warnings → workspace tests → check nodefault → policy
cargo xtask package     # release build + NSIS installer into dist/ and dist-installer/
cargo xtask policy      # policy gate only
cargo xtask licenses    # cargo-deny license check
cargo xtask fuzz        # fuzz smoke (cargo-fuzz 0.13.2 + nightly-2026-09-09)
cargo replay            # state reducer benchmark (local tool, not run in CI)

cargo build -p bytwina --release --no-default-features
cargo test -p client-core <test name>
node tests/login-handoff.cjs           # required after auth changes
python extensions/test_catalog.py      # validate the plugin catalog
```

`cargo xtask policy` enforces: no `eframe`/`egui` `persistence` feature, no `reqwest` `cookies`
feature, `persist_window: false`, a single renderer, no REST cookie jar, ephemeral login.

## 6.8 CI and release

Workflows in `.github/workflows/`:

| File | Purpose |
|---|---|
| `ci.yml` | `native` (Windows: check demo targets, `cargo xtask check`, bun tests, `cargo xtask package`, upload artifact) + `security` (cargo-audit on ubuntu) |
| `release.yml` | Nightly/production: prepare → build Windows → publish; `repositories` paused |
| `ci-diagnostics.yml` | Re-runs `cargo xtask check` and publishes the failure to the `ci-diagnostics` branch (`diagnostics/check.txt`) |
| `extension-catalog.yml` | Validates and pins `extensions/catalog.json` |
| `nix.yml`, `linux-packages.yml`, `package-repositories.yml` | Paused, kept for when Linux returns |

`release.yml` triggers on a 03:17 UTC schedule, manual dispatch (`nightly`/`production`), and pushes
that touch `Cargo.*`, `apps/**`, `crates/**`, `.github/release/**`, or `packaging/windows/**`.
Concurrency group `bytwina-release` runs one release at a time. Notes come from
`.github/release/notes.mjs`: the first release is an introduction; later ones list only new, fixed,
performance sections plus a versions table. `HOMEBREW_TAP_TOKEN` being unset disables the macOS cask.

**Reading CI without log access:** the logs API needs admin. Use the `ci-diagnostics` branch file, or
open the public job page and read the Annotations panel. A diagnostics file can be stale — check the
commit in its content before trusting it.

## 6.9 Packaging

Windows today: NSIS per-user installer (`RequestExecutionLevel user`, `$LOCALAPPDATA\Programs\Bytwina`),
Windows 10 guard, process-running check, uninstall registry keys, notification shortcut with AUMID,
licence page, and a portable `.zip`. `build.rs` compiles `bytwina.rc` through `rc.exe` (MSVC) or
`windres` to embed the icon. Uninstall removes shortcuts, the Run entry, and the install tree.

macOS (`.app` + Info.plist) and Linux (Flatpak, AppImage, deb/rpm/arch) code exists but is not built
by the current pipeline. Do not claim those artefacts exist in release notes or the README.

## 6.10 Things that pin exact values

Changing any of these without updating the test breaks CI:

| Value | Where |
|---|---|
| diagnostics framing **12 bytes**, 8 KiB, 64 lines | `crates/discord-gateway/src/lib.rs` |
| legacy SDK fixture **574_991 bytes** | `crates/extensions/examples/legacy_sdk_check.rs` + fixture README |
| session cache **500 / 4 MiB / 1024** | `crates/session-cache/src/lib.rs` |
| SQLite schema **29** | `crates/local-store/src/lib.rs` |
| extension limits (16/4/2/0.25 MiB, 8 plugins, 64 capabilities) | `crates/extensions/src/lib.rs` |
| `THEME_FIELDS` order (18) | `crates/ui/src/design.rs` and `theme_editor` |

## 6.11 Documents

| Doc | Read it for |
|---|---|
| `docs/architecture.md` | crate responsibilities, storage worker and REST capacity |
| `docs/design.md` | interface direction, spacing, colour, motion |
| `docs/discord-compatibility.md` | what is verified vs unverified against Discord |
| `docs/storage-policy.md` | every persisted byte, retention and ceilings |
| `docs/performance.md` | measured costs and the reasoning behind limits |
| `docs/platform-support.md` | per-OS status and packaging |
| `docs/threat-model.md` | assets, threats and mitigations |
| `docs/authentication.md` | login webview, handoff, large-account startup |
| `docs/invite-verification.md` | user-solved challenge flow |
| `docs/voice.md` | transport, codecs, DAVE, teardown |
| `docs/extension-sdk-*.md` | plugin authoring, actions, app data |
| `docs/theme-api.md` | theme JSON fields |
| `docs/extensions.md` | the community extension list |

## 6.12 Known gaps and paused work

- macOS and Linux builds are paused; the code paths exist and are unreviewed for regressions.
- `fuzz`, `extension-sdk` and Nix jobs are disabled while the pipeline targets Windows only.
- Windows binaries are unsigned: SmartScreen warns, users choose "More info → Run anyway".
- Several protocol claims in `docs/discord-compatibility.md` are explicitly unverified; do not
  promote them to verified without evidence.

## 6.13 Code conventions

- Tabs for indentation (`hard_tabs = true` in `rustfmt.toml`), 100 column lines (rustfmt default).
- `unsafe_code = "forbid"` at the workspace level; a few explicit `#[allow]` sites in `platform`.
- No `unwrap()`/`expect()` on network input or user data; tests may use them.
- Errors use `thiserror`; no `anyhow` inside crates.
- Comments explain **why**, not **what**. No comments restating a name.
- Prefer editing an existing module over adding a new one; the crate boundaries in §6.2 are not
  suggestions.

_Licence: MIT OR Apache-2.0. Copyright 2026 Lumina contributors._
