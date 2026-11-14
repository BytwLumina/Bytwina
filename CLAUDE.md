# CLAUDE.md — working guide for Claude in Bytwina

This file is for Claude Code. It is deliberately more operational than `AGENTS.md`: the same
boundaries, plus the current numbers, the file map for the surfaces that break most often
(profile, cosmetics, banner), and the commands that actually catch a mistake before CI does.

**Read order:** §2 boundaries → §9 verification → §5.3 or §8 for the area you touch → the
matching `docs/*.md` file → code.

Language of the repository: English (code, comments, commits, docs). Answer the owner in the
language they used.

---

## 1. What Bytwina is

A native desktop client for **existing normal Discord accounts**, written in Rust and rendered
with `egui`/`wgpu`. Not a bot, not a web wrapper, not a backend. Windows is the only shipped
target; macOS/Linux code exists but is paused.

| Item | Value |
|---|---|
| Repo | `BytwLumina/Bytwina` — <https://github.com/BytwLumina/Bytwina> |
| Releases | nightly on a 03:17 UTC schedule, manual `nightly`/`production` dispatch, and pushes touching `Cargo.*`, `apps/**`, `crates/**`, `.github/release/**`, `packaging/windows/**` |
| Toolchain | pinned `1.98.1` (`rust-toolchain.toml`), edition 2024, resolver 3, `Cargo.lock` is committed |
| Size | 13 crates + `apps/desktop` + 3 tools (`xtask`, `replay-bench`, `tray-debug`) |
| Licence | MIT OR Apache-2.0, `publish = false`, author "Lumina contributors" |

A build that compiles is **not** evidence that a Discord route works. See §10.

---

## 2. Non-negotiable boundaries

1. **One state owner.** `client_core::State` is mutated only on the UI thread. Network adapters
   emit generation-tagged `Event`s; they never touch state.
2. **The UI has no network and no credentials.** `ui` renders and returns `Command`s.
3. **Everything is bounded** by bytes *and* item count. A new collection needs both limits and a
   test for the limit.
4. **Wire DTOs never become state.** `discord-protocol` parses into a bounded DTO → validate →
   `model`. Never hand a raw `serde_json::Value` to state.
5. **Fail closed.** Unknown kind, anchor, palette, size or state is dropped, not guessed.
6. **No telemetry, no backend, no credential extraction, no challenge bypass.** Plaintext login
   only when the owner enables it explicitly. Never log tokens.
7. **Protocol claims carry a label**: documented · restricted · unofficial · unverified. All
   normal-account profile, collectible, quest, discovery and saved-message routes are
   **unofficial** and mostly **unverified** against a live session.

Dependency facts worth remembering (they are what the policy gate protects):

- `eframe`/`egui` are pinned to a fork rev; the `persistence` feature must stay off
  (`persist_window: false`, `persist_egui_memory() -> false`).
- `reqwest` has no cookies, no redirects in the client; uploads use a separate credential-free
  client.
- Login/invite webviews are ephemeral (`.with_incognito(true)` in `crates/platform`).
- `ClientBuilder`/`unsafe_code = "forbid"` at workspace level; a few explicit `#[allow]` sites in
  `crates/platform`.

---

## 3. Layout and dependency rules

```
apps/desktop/          59 modules — the only place network, core, UI and OS meet
crates/model/          UI-neutral entities, no I/O
crates/discord-protocol/  wire JSON DTOs, bounded parsers
crates/discord-gateway/   gateway WebSocket (zlib-stream, v10)
crates/discord-api/       REST adapter (4 permits, 2 reserved for foreground)
crates/discord-voice/     media, DAVE/E2EE
crates/client-core/       State, commands, events, domain logic
crates/session-cache/     one bounded RAM timeline window per channel
crates/local-store/       SQLite, account-isolated
crates/platform/          keyring, tray, notifications, hotkeys, webviews, MF capture
crates/ui/             115 modules — egui views, tokens, i18n, artwork
crates/extensions/       Wasm plugin runtime
crates/diagnostics/      opt-in bounded log
crates/test-support/     synthetic fixtures only
```

Allowed edges:

```
ui ──────────► model (+ Command only)
client-core ─► model, session-cache, local-store, diagnostics
discord-api ─► discord-protocol, model
gateway ─────► discord-protocol, model, client-core (voice events only)
voice ───────► model, client-core (events), platform (capture)
apps/desktop ► everything
```

Never: `ui` importing `discord-api`, `discord-gateway`, `reqwest` or `tokio`; `model` touching a
file or a socket; `client-core` performing HTTP/WS itself.

---

## 4. Current numbers (verified against `main`, re-check before quoting)

| Value | Where |
|---|---|
| `Command` **70** variants, `Event` **90** variants | `crates/client-core/src/lib.rs` |
| Locale catalogs: **15** languages (14 files + System) | `crates/ui/src/i18n.rs` `Language::ALL` |
| `en-US` keys **3077**; `vi` **3077**; most others **2649**; `cs` 2727, `zh-*` 2721, `de` 2657 | `crates/ui/locales/*/main.ftl` |
| SQLite schema **29** (`NATIVE_SCHEMA` = `READABLE_SCHEMA`) | `crates/local-store/src/lib.rs` |
| Client build advertised **634_304** | `crates/client-core/src/fingerprint.rs` |
| Timeline window **500 msgs / 4 MiB / 1024 mutations** | `crates/session-cache/src/lib.rs` |
| Event bytes 4 MiB, 8 event slots, 16 command slots, bulk ack 1024 | `client-core` |
| Account navigation 131 072 entries, 128 MiB; permissions 64 MiB | `model/src/account.rs` |
| Gateway frame 64 MiB; member list 256 KiB, 1024 ops, 8 retired lists | `discord-gateway` |
| Diagnostics framing 12 bytes, 8 KiB / 64 lines | `discord-gateway` |
| REST 4 permits, 2 foreground, 2 read retries, 64 KiB thread preview | `discord-api` |
| Extension limits 16/4/2/0.25/1 MiB, 8 plugins, 64 capabilities, 64 panel elements, tick ≥ 250 ms | `crates/extensions` |
| `THEME_FIELDS` **18** colour slots; accent `0x1a72e8` | `crates/ui/src/design.rs` |
| UI tokens `RADIUS` 16, `PAD` 20, `BUTTON_HEIGHT` 38, `TITLE_BAR_HEIGHT` 36, idle timeout 600 s, custom font 32 MiB | `crates/ui/src/{design,lib,fonts}.rs` |
| `WINDOWS_FILES` allowlist (11 top-level entries) | `apps/desktop/src/updater_install.rs` |

Changing any of these means updating the constant **and** the test that pins it **and** the doc
sentence that states it, in one commit.

---

## 5. The event/command pipeline

### 5.1 The shape

```
Gateway WS ─► discord-gateway ─► Event {generation} ─┐
REST HTTP  ─► discord-api     ─► Outcome ────────────┴─► client_core::State::apply(Envelope)
                                                              │  (UI thread only)
ui input ─► Command ─► apps/desktop ─► discord-api / gateway / local-store / platform
```

`Envelope { generation, event }` is the only door into state. `State::apply_with_permissions`
drops any envelope whose `generation` differs from the current one — that is the whole race
defence. Any new async work must capture the current `generation`, echo it in the event, and let
the reducer reject stale results.

### 5.2 Adding one networked action (the sequence that works)

1. `model/` — the entity, if a new one is needed, with a `valid()`/`bytes()` bound.
2. `discord-protocol/` — the DTO + bounded parser, plus a malformed-input test.
3. `discord-api/` — the request builder (exact route, body bound, permit class). Writes are
   single-attempt; ambiguous outcomes are **not** retried.
4. `client-core/` — a `Command` variant, an `Event` variant, the reducer arm, the limits, and the
   fallback arm in the "was not queued" path (see `crates/client-core/src/lib.rs` around line
   2379 — commands rejected before dispatch must produce a user-visible status, never silence).
5. `apps/desktop/` — route the command to the API (usually a `connection.rs` arm) and push the
   outcome back as an `Event` with the same `request` id.
6. `apps/desktop/src/main.rs` demo block — answer the command with synthetic data so
   `--demo` never hangs on a spinner.
7. `ui/` — the control that emits the command and the state that shows the result; every user
   string goes through `crate::i18n::translate("<key>")` in **all** catalogs.

A `Command`/`Event` pair without a reducer arm compiles and silently does nothing. That is the
most common integration bug in this repository.

### 5.3 Status strings: `&'static str` keys or `Status`

`client_core::status::Status` is `Box<str>` with `From<&'static str>`, `From<String>`,
`From<Box<str>>`, `From<Failure>`, `Deref<Target=str>`, `Display`, `PartialEq<&str>`,
`new/as_str/is_empty/set/clear/EMPTY`.

Rules that avoid the compile-error cascade this repo has hit twice:

- A status **field** is either `Status` (when translated or owned text is possible) or
  `Option<&'static str>` (device/status hints that are never translated). Do not invent a third
  shape.
- Assigning a literal to a `Status` field needs `.into()`; assigning a translated `String` needs
  `.into()` too; assigning `(*error).into()` is required when the binding is `&&str` (a match on a
  `&Result`).
- `Option<&'static str>` fields are assigned `Some(...)` / `None`, never a bare `&str`.
- Read a `Status` as `.as_str()` when a `&str` is required (`RichText::new`, `design::hint`), or
  pass it directly where `impl Display`/`Deref` is accepted.
- Literal status text is fine when it is genuinely literal (`"Disconnected"`, demo fixtures). Keys
  are what the catalogs translate: `translate_if_key` leaves non-keys untouched, so both kinds
  share one type.

---

## 6. Repository conventions

- Tabs for indentation (`hard_tabs = true`), rustfmt default 100 columns. `cargo fmt --all` before
  every commit.
- No `unwrap()`/`expect()` on network or user data; tests may.
- Errors: `thiserror`; the single type crossing the boundary is `client_core::auth::Failure`.
  `ProtocolAt`/`CapacityAt` take **compile-time labels only** — never remote text, ids or decode
  detail. Prefer an existing variant over extending the enum.
- Comments explain **why**. A comment that restates the identifier is deleted.
- Few, high-signal tests. Write: wire parsing incl. malformed input, bounds, security boundaries,
  persistence/migrations, state reconciliation, a regression test for each real bug. Do not write:
  type/derive/default restatements, layout-value pins, "renders without panic" smoke tests,
  wall-clock asserts.
- Test placement: `#[cfg(test)] mod tests` at the bottom of the owning file; crate integration
  tests in `crates/<crate>/tests/`; fixtures come from `test-support`.
- i18n: adding a key means every `main.ftl`. A missing key falls back to English silently, so a
  full translation is a requirement, not a nicety. Use `translate`, `translate_args`,
  `translate_count`; `translate_if_key` only for values that may be service text.
- Commits: Conventional Commit, scoped to the crate, message states **why**.

---

## 7. Commands

```sh
cargo fmt --all                       # must leave no diff
cargo xtask check                     # fmt --check → clippy -D warnings → workspace tests
                                      # → check -p bytwina --no-default-features → policy
cargo xtask policy                    # persistence/webview/cookie policy only
cargo xtask package                   # release build + NSIS into dist/ and dist-installer/
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --locked -p bytwina --no-default-features
bun tests/login-handoff.cjs           # after auth changes
python extensions/test_catalog.py     # catalog only
```

Run modes:

```sh
cargo run --locked --features demo -- --demo            # synthetic app, no account
cargo run --locked --features demo -- --demo --demo-profile
cargo run --locked --features demo -- --demo --demo-theme=<preset> --demo-light
```

Native offline screenshot harness (this is how UI evidence is produced — no live account):

```sh
cargo run --locked --features demo --example profile_preview -- --demo \
  --page=profile|profile-card|collectibles|account|appearance|… \
  --output=shot.png [--width=1120] [--height=760] [--light] [--full] [--own] \
  [--page=collectibles --badge-directory|--badge-customize]
```

The example answers only synthetic commands; if you add a command that the profile editor emits
(`OwnedCollectibles`, `Profile`, `EditProfile`, …) and forget its arm there, the editor spins
forever in the harness.

Known local limitation: without MSVC build tools `link.exe` fails, so a local
`cargo build`/`cargo test` cannot link. `cargo fmt`, `cargo xtask policy`, `cargo check` for
crates whose build scripts need no linker, and Python/Bun scripts still run. CI is the compile
authority — say so instead of claiming a local build.

---

## 8. Profile, cosmetics and banner (the fragile area)

This is where the owner reports the most visual defects. Read this section before touching
`profiles.rs`, `profile_edit.rs`, `avatars*` or `collectibles`.

### 8.1 File map

| Concern | File |
|---|---|
| Popout / full profile / docked card, banner, avatar ring, badges, mutuals, board | `crates/ui/src/profiles.rs` (`card_view` ~3095, `paint_collectibles` ~4511, `Theme` ~1363) |
| Global profile editor draft, preview, native picture picker | `crates/ui/src/profile_edit.rs` |
| Avatar/decor/nameplate/effect/frame painting, banner painting | `crates/ui/src/avatars.rs`, `crates/ui/src/avatars/cosmetics.rs` |
| Name styles (Nitro fonts/effects/colours) | `crates/ui/src/name_styles.rs`, `crates/ui/src/fonts.rs` (`NAME_FONTS`) |
| `UserProfile`, `ProfileEdit`, cosmetic model + limits | `crates/model/src/profile.rs`, `crates/model/src/cosmetics.rs` |
| Profile wire decode, `encode_edit`, `encode_profile_metadata` | `crates/discord-protocol/src/profile.rs` |
| Collectible product decode, owned purchases | `crates/discord-protocol/src/collectibles.rs` |
| Save endpoints (two PATCH groups) | `crates/discord-api/src/profile_edit.rs` |
| Own-profile state machine (`load_own_profile`, `save_own_profile`, `can_save_own_profile`) | `crates/client-core/src/profile.rs` |
| Owned-collectible picker state | `crates/client-core/src/collectibles.rs` |
| Settings page hosting the editor | `crates/ui/src/settings.rs` (`Page::Profile` → `editor.show`) |
| Demo/offline profile fixture | `profiles::synthetic`, `synthetic_effect`, `synthetic_frame`, `test_support::synthetic_cosmetics`, `demo_collectibles` |

### 8.2 Geometry contracts

- Popout card `WIDTH = 340`, `PAD = 12`, avatar `80`, ring `6`, `RADIUS = 12`.
- Full profile `FULL_WIDTH = 1000`, banner `160`, avatar `120`, `FULL_PAD = 28`; split layout when
  `full_width >= 700`.
- Avatar decoration is drawn at `DECORATION_TO_AVATAR = 1.2` × the avatar, centred on it, and only
  at `>= DECORATION_MIN` (30 pt) — smaller avatars (replies, reactions, typing) stay undecorated.
- Effect canvas is `EFFECT_CANVAS = [450, 880]`, scaled by `card.width() / 450`, with a
  `EFFECT_INTRO_MS = 500` delay; layers are clipped to the card.
- Frame overflow is `[top, bottom, side] = overflow(width)` scaled from `inner_width` (default
  1200). Staples pin outside the top/bottom edge, rails align inside the card height and are
  skipped when `responsive` and the card is at least `FRAME_RAIL_MIN_ASPECT = 0.625` as wide as
  tall; borders tile down the card. Back layers sit **behind** the card, front layers **over** it.
- Nameplates are 16:3, right-aligned, over a palette gradient from 0 % to 30 % opacity; offline
  and muted rows show 10 % until hovered.

When a frame is "not showing completely", the suspects are, in order: the clip rect used for the
shapes (`paint_collectibles` computes a different clip for docked / full / popout), the card rect
passed to `frame_shapes` (it must be the final painted card, not an inner content rect), a
`Staple`/`Rail`/`Border` kind decoded as the wrong variant, and a layer with a missing anchor now
defaulting to `Top`. Fix the geometry or the decode — never widen the clip to hide a wrong rect.

### 8.3 Editing flow

- Two endpoints: account fields (`global_name`, `bio`, `pronouns`, `accent_color`, `avatar`,
  `avatar_decoration_sku_id`, `nameplate_sku_id`, `display_name_*`) go to `PATCH /users/@me`;
  metadata (`banner`, `theme_colors`, `profile_effect_id`) goes to `PATCH /users/@me/profile`.
  Server-scoped metadata goes to `PATCH /guilds/{id}/profile/@me` and is a separate command.
- `Draft::changes` sends only explicitly changed fields; nested `None` clears.
- `accept_avatar` receives the native picker result; a new picture is a PNG data URI capped by
  `MAX_PROFILE_AVATAR_URI`, and the preview texture is stored in `pending_picture`.
- The owned-collectible list arrives through `Command::OwnedCollectibles` / `Event::
  OwnedCollectibles`; `Picker::begin`/`finish` match on `request`.
- The editor preview draws the **saved** profile plus pending picture/draft text. It never paints
  the equipped frame or effect today. If the owner says the preview does not match the real card,
  the fix is to run the same `paint_collectibles` path there, not to add a second drawing.
- **Profile frames cannot be equipped from this editor**: `ProfileEdit` has no frame field and the
  UI has no frame picker. Frames are display-only (decoded from the profile). Adding the picker
  means: a `profile_frame_sku` field in `ProfileEdit` (model + `valid()`), the wire field in
  `encode_profile_metadata`, the `CollectibleKind::Frame` decor row, and a demo arm.

### 8.4 Banner

`avatars::paint_banner` paints the theme-derived colour, then cover-crops the image
(`scale = max(rect/source)`, centred UV). The editor preview calls it with `profile`, not with the
draft, so a freshly chosen banner only shows after save. When a banner looks wrong the order is:
`banner_key`/`banner_url` (guild banner beats user banner), then the crop, then the corner radius
the caller passes.

---

## 9. Verification before handing work over

1. `cargo fmt --all` — no diff.
2. `cargo clippy --workspace --all-targets --locked -- -D warnings` — remember clippy stops at
   the first crate, so after fixing one group re-run; new `.into()` / borrow changes commonly
   surface `useless_conversion`, `needless_borrow`, `needless_borrows_for_generic_args`,
   `private_interfaces`, `type_complexity` in the same commit.
3. `cargo check --locked -p bytwina --no-default-features` — release builds and the `nodepfault`
   job compile without the `demo` feature; a `#[cfg(demo)]` constant used in non-demo code breaks
   the release and not the lint.
4. `cargo test --workspace --locked` plus the demo targets
   (`cargo check --locked -p bytwina -p ui --all-targets --features bytwina/demo`).
5. UI changes: a `profile_preview` screenshot in both themes, or say "no visual capture".
6. Auth changes: `bun tests/login-handoff.cjs`.
7. Native string changes: every `.ftl` updated; keys exist in `en-US` and every shipped locale.

Then report: what changed, which checks actually ran, what could not run locally, and any
remaining unverified claim.

### Reading CI

The logs API needs a token with admin on the repo. Without it: the `ci-diagnostics` branch file
(can be stale — compare with the commit) or the public job page's **Annotations** panel. With an
authenticated `gh`:

```sh
gh run list --limit 5
gh run view <run> --json jobs --jq '.jobs[] | .name + ": " + .status + " " + .conclusion'
# while the run is still going this fails; stream the job log directly instead:
gh api repos/BytwLumina/Bytwina/actions/jobs/<job_id>/logs --allow-escape-sequences \
  | sed -E 's/\x1b\[[0-9;]*m//g; s/\^\[\[[0-9;]*m//g'
```

`native-client` gates: `lint`, `test`, `demo`, `nodepfault`, `offline` (Bun + xtask workspace +
login handoff), `package` (thin LTO build + NSIS artifact), aggregated by `native`; `security`
runs `cargo audit --deny warnings` (an `unsound` advisory fails it — patch the crate and unpin it
in the manifest that pins it, e.g. `crates/diagnostics/Cargo.toml`).

`release.yml`: `prepare` (semver + notes) → `build` (windows: `cargo fmt --check`,
`cargo check -p bytwina --bin bytwina --no-default-features`, login handoff, `cargo xtask package`)
→ `publish` (assets + `SHA256SUMS.txt`). Concurrency group `bytwina-release`,
`cancel-in-progress: false`, so a stuck build blocks the next night.

---

## 10. Claims, docs and honesty

- `docs/discord-compatibility.md` is the register of what is documented vs unverified. Every new
  route gets an entry with the date, the source and the label. Keep the schema number and limits
  stated there accurate.
- Fixtures and demo screenshots prove rendering, not interoperability.
- Do not claim macOS/Linux artefacts, notarized builds, or working quest heartbeats
  (the heartbeat route rejects non-Electron user agents; the client does not send them).
- Known paused work: macOS/Linux builds, `fuzz`, `extension-sdk`, Nix, package repositories,
  scheduled-event creation, heartbeat-driven quests, frame equipping from the editor.

---

## 11. Pitfalls that have already cost a CI cycle

| Symptom | Cause |
|---|---|
| Release build fails while lint passes | code behind `--no-default-features`, e.g. a `#[cfg(any(test, all(debug_assertions, feature = "demo")))]` constant used by runtime code |
| `String: From<&Box<str>>` / `From<&mut &'static str>` | `&reason`/`&name` where the value already is a reference; drop the `&` or use `(*x)` |
| `Status: From<&&str>` | matching on `&Result<T, &'static str>` — bind `(*error).into()` |
| `RichText::new(Status)` | pass `.as_str()` |
| `Option<&str> = "..."` | device status fields are `Option<&'static str>` |
| `private_interfaces` | a `pub fn` returning a `pub(crate)` type with no outside callers — delete it |
| `type_complexity` | a long tuple in a local `Vec` — `#[allow(clippy::type_complexity)]` on the item |
| Catalog load failure | duplicate message id in an `.ftl`; `no_catalog_declares_a_message_twice` catches it |
| Self-update rejected with "unexpected package content" | a new top-level file in `dist/` missing from `WINDOWS_FILES` |
| Workspace refuses to load | a stray `__pycache__`/non-crate dir under `tools/*` (`members = [..., "tools/*"]`) |

Do not edit generated outputs by hand: `assets/icons/atlas.png` (`tools/generate-icons.py`),
`assets/brand/bytwina-*.png` (`tools/generate-app-icon.py`), `extensions/catalog.json`
(`extensions/catalog.py --pin <sha>`).

---

## 12. Delivery

Default for this repository, unless the owner says otherwise in the conversation:

1. Work on a task branch; never switch or rebase another task branch; never stash or discard
   unrelated dirty files — use a separate worktree when the tree is dirty.
2. Stage explicit paths, review the staged diff, Conventional Commit scoped to the crate.
3. Push to `origin` when the owner has asked for delivery; open/update the PR with
   `.github/pull_request_template.md` (Summary · Verification · Before/after · Performance ·
   Risks). Completed work is ready for review; draft only for unfinished work.
4. Never merge, force-push, deploy, release or change repository permissions without an explicit
   instruction in the conversation.

---

_Licence: MIT OR Apache-2.0. Copyright 2026 Lumina contributors._
