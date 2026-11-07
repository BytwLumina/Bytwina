# Bytwina — Agent onboarding

Bản đồ kỹ thuật của dự án, viết cho các AI agent (Codex, Claude, MiniMax, Cursor…) đọc trước khi
sửa code. Kèm với `AGENTS.md` (quy tắc delivery, test, bảo mật) — file này nói **cấu trúc và ý đồ**,
`AGENTS.md` nói **được phép làm gì**.

---

## 1. Khái quát

**Bytwina** là client Discord native viết 100% bằng Rust, render bằng `egui` + `wgpu`. Không phải
bot, không phải backend, không phải web wrapper (không Electron/WebView cho UI chính).

| Hạng mục | Giá trị |
|---|---|
| Ngôn ngữ | Rust 2024 edition, toolchain ghim `1.98.1` (`rust-toolchain.toml`) |
| UI | `egui` / `eframe` (fork ghim rev `4821fc5`), renderer `wgpu` |
| Async | Tokio (multi-thread runtime) |
| HTTP | `reqwest` 0.13.5, rustls, không cookie jar, không redirect |
| WebSocket | `tokio-tungstenite` 0.30 |
| DB | SQLite bundled qua `rusqlite` 0.40.2 |
| Secrets | OS keyring (`keyring` 4.2), zeroize in-memory |
| Audio/video | Opus, DAVE/E2EE, Media Foundation (Windows), screen share |
| Licence | Dual MIT OR Apache-2.0, `publish = false` |
| Tác giả | "Lumina contributors", org `BytwLumina` |
| Nền tảng | **Windows 10/11 là mục tiêu hiện tại**; macOS/Linux có code nhưng đang tạm hoãn build |
| Tổng LOC | ~353k dòng Rust (không tính `vendor/`) |

### Nguyên tắc thiết kế cốt lõi

1. **Một chủ sở hữu state**: `client-core` giữ toàn bộ state trên UI thread. Network adapter chỉ
   gửi *event đã gắn generation tag*, không bao giờ sửa state trực tiếp.
2. **UI không có network/credential**: `ui` chỉ phát ra `Command`; `apps/desktop` nối dây.
3. **Mọi thứ có giới hạn**: cache, payload, queue đều bị chặn theo **byte** và **số item**.
4. **Wire DTO không bao giờ thành state**: `discord-protocol` parse JSON → DTO → validate → mới vào
   `model`/`client-core`.
5. **Fail closed**: input lạ, size lạ, trạng thái lạ → từ chối, không đoán.
6. **Không thu thập dữ liệu**: không telemetry, không log không giới hạn. Log chẩn đoán là opt-in,
   có hạn mức byte, redact secrets.

---

## 2. Cấu trúc thư mục

```
Bytwina/
├── apps/desktop/          # Binary "bytwina": nối dây mọi thứ (52k dòng)
├── crates/
│   ├── model/             # Entity UI-neutral: User, Message, Channel, Guild… (11k)
│   ├── discord-protocol/  # DTO wire JSON (16.8k) — không chứa logic nghiệp vụ
│   ├── discord-gateway/   # WebSocket Gateway: Hello/ACK/Heartbeat/Resume (9.5k)
│   ├── discord-api/       # REST client: origin cố định, TLS verify, bounded body (13.7k)
│   ├── discord-voice/     # Media: Opus, DAVE/E2EE, camera, screen share (25k)
│   ├── client-core/       # ⭐ Chủ sở hữu state duy nhất (44.6k)
│   ├── session-cache/     # RAM timeline có hạn: 500 msg / 4 MiB (1.7k)
│   ├── local-store/       # SQLite cache theo account, có trần dung lượng (4k)
│   ├── platform/          # OS: keyring, tray, notification, webview login (15k)
│   ├── ui/                # ⭐ egui views + widget + theme + i18n (130k)
│   ├── extensions/        # Runtime plugin Wasm (wasmi), capability-gated (5k)
│   ├── diagnostics/       # Log chẩn đoán opt-in, bounded, redacted (1.2k)
│   └── test-support/      # Fixture tổng hợp cho test/demo (2.8k)
├── tools/
│   ├── xtask/             # cargo xtask check|package|policy|licenses|fuzz
│   ├── replay-bench/      # Benchmark reducer state (dev only)
│   └── tray-debug/        # Debug tray icon
├── extensions/            # SDK + catalog + plugin/theme mẫu (Python tooling)
├── packaging/             # windows/ macos/ linux/ flatpak/ appimage/ aur/ nix/
├── docs/                  # architecture, design, threat-model, storage-policy…
├── assets/                # brand/, fonts/, icons/, licenses/
├── vendor/                # Fork thứ ba (wry, hpke-rs) — giữ nguyên header
└── fuzz/                  # cargo-fuzz target (workspace riêng)
```

### Ranh giới phụ thuộc (quan trọng)

```
ui ──────────► model (+ Command, không có network)
client-core ─► model, session-cache, local-store, diagnostics
discord-api ─► discord-protocol, model
discord-gateway ─► discord-protocol, model, client-core (voice event)
apps/desktop ► TẤT CẢ (nơi duy nhất nối network ↔ core ↔ ui)
```

`ui` **không được** import `discord-api`, `discord-gateway`, `reqwest`, `tokio`.
`model` **không được** import filesystem/network.
`client-core` **không được** gọi HTTP/WS trực tiếp — chỉ phát `Command`.

---

## 3. Luồng dữ liệu

```
                    ┌─────────────── apps/desktop ───────────────┐
Gateway WS ──► discord-gateway ──► Event(generation) ──► client-core ──► ui (snapshot)
REST HTTP ──► discord-api ───────► Outcome(generation) ─┘        │
                                                                  ▼
ui (input) ──► Command ──► apps/desktop ──► discord-api / gateway
                        └─► local-store (persist), platform (OS)
```

**Generation tag**: mỗi lần reconnect/session mới, `generation` tăng. Event của generation cũ bị
bỏ. Đây là cơ chế chống race kinh điển của repo — **giữ nguyên khi thêm event mới**.

---

## 4. Các chức năng (theo crate)

### `discord-gateway` — realtime
Identify/Resume, heartbeat, reconnect có backoff, member list (SYNC/ops), presence batch, typing,
voice state, thread events. Có `Diagnostics` với ngân sách byte **8 KiB / 64 dòng**, framing
`"[Bytwina <scope>] <label>\n"` = **12 byte** khung (nếu đổi brand/format phải cập nhật hằng số này
và test ở `crates/discord-gateway/src/lib.rs`).

### `discord-api` — REST
Origin cố định `discord.com`, TLS verified, không cookie, không redirect. Có parser cho forum edit,
slowmode, hide-after, permission overwrites (murmur3), reaction path, mention, timestamp, GIF allowlist.

### `client-core` — state + nghiệp vụ (49 file)
Mỗi file là một miền: `permissions`, `read_state`, `notifications`, `presence`, `typing`, `replies`,
`threads`, `forum`, `polls`, `stickers`, `invites`, `search`, `server_*` (admin/roles/settings/
integrations/audit_log), `voice`, `auth`, `captcha`, `verification`, `profile`, `resident`, `preload`,
`fingerprint`, `guild_folders`, `game_mentions`, `message_actions`, `user_actions`…

### `ui` — giao diện (130k)
`lib.rs` là shell chính, `design.rs` là token/palette/theme, `timeline.rs`, `markdown.rs`,
`highlight.rs`, `emoji.rs`, `avatars.rs`, `fonts.rs` (name fonts), `i18n.rs` (Fluent),
`local_time.rs`, `scroll.rs`, `settings.rs`, `theme_editor.rs`, `profiles.rs`, `role_names.rs`.

### `discord-voice` — media
Transport, crypto (DAVE/E2EE, hpke vendored), screen capture, camera, video encode/decode
(AV1/HEVC/H264), mixer, jitter buffer, sync/playout.

### `platform` — OS
Keyring, tray (Win32/Wayland/macOS), notifications (AUMID), single instance, global hotkeys,
idle, game activity, DWM, HEIC, Media Foundation, live capture.

### `apps/desktop` — nối dây (52k)
`main.rs` (shell), `connection.rs`, `voice.rs`, `video.rs`, `audio.rs`, `updater*.rs` (tự cập nhật,
verify SHA-256 + length), `extensions*.rs` (catalog, tải, sandbox), `sign_in.rs`, `tray_window.rs`,
`game_activity.rs`, `captcha.rs`, `clipboard.rs`, `downloads.rs`, `pointer.rs`, `gpu.rs`.

### `extensions` — plugin
Runtime `wasmi` (Wasm v1), ABI `{stem}_alloc(i32)->i32` + `{stem}_invoke(i32,i32)->i64`, giới hạn
16 MiB / 1 memory / 4096 element, capability-gated. `ABI_STEMS = ["bytwina", "lumina", "serein"]`
là cố ý (tương thích tên cũ).

---

## 5. Cấu hình & môi trường

| Biến | Ý nghĩa |
|---|---|
| `CARGO_PROFILE_RELEASE_LTO` | CI đặt `thin`; release thật `fat` (từ `Cargo.toml`) |
| `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` | CI đặt `16` |
| `LIBSQLITE3_FLAGS` | Tắt FTS/RTree/STAT4 (`.cargo/config.toml`) |
| `BYTWINA_NOTES_MODE` | `intro` / `conventional` — chọn kiểu release notes |

Alias: `cargo xtask …`, `cargo replay` (xem `.cargo/config.toml`).

**Product identifier** (nhất quán toàn repo):
- App ID: `io.github.ninokiru.Bytwina`
- Binary: `bytwina.exe`
- Data dir: `bytwina` / `bytwina-development`
- AUMID/tray prop: `Bytwina`, `Bytwina.TrayState`, `Bytwina.TrayCallback`
- Repo: `https://github.com/BytwLumina/Bytwina`

---

## 6. Lệnh thường dùng

```bash
cargo xtask check           # fmt --check → clippy -D warnings → test workspace → check no-default → policy
cargo xtask package         # build release + NSIS installer vào dist/ và dist-installer/
cargo xtask policy          # chỉ policy gate
cargo xtask licenses        # cargo-deny check licenses
cargo xtask fuzz            # smoke fuzz (cần cargo-fuzz 0.13.2 + nightly-2026-09-09)

cargo build -p bytwina --release --no-default-features
cargo test -p client-core <tên test>
node tests/login-handoff.cjs           # bắt buộc khi sửa auth
python extensions/test_catalog.py      # validate catalog plugin
```

CI: `.github/workflows/ci.yml` (native windows + security) và `release.yml` (nightly, Windows only).
`ci-diagnostics.yml` đẩy lỗi `cargo xtask check` lên nhánh `ci-diagnostics` (file
`diagnostics/check.txt`) để đọc khi không có quyền tải log.

---

## 7. Quy ước code

- **Tabs** để indent (`rustfmt.toml` có `hard_tabs = true`), dòng tối đa 100 cột (mặc định rustfmt)
  — `cargo fmt --all -- --check` là gate CI.
- `unsafe_code = "forbid"` ở workspace (trừ vài chỗ có `#[allow]` tường minh trong `platform`).
- Không `unwrap()`/`expect()` trên input mạng hoặc dữ liệu người dùng; test thì được.
- Lỗi dùng `thiserror`, không `anyhow` trong crate.
- Comment giải thích **vì sao**, không giải thích **cái gì**.
- Đổi hằng số/brand → quét cả test pin chiều dài byte (đã từng hai lần làm đỏ CI).

---

## 8. Bảo mật — đọc trước khi sửa

Chi tiết ở `docs/threat-model.md`. Tóm tắt bắt buộc:

- Token lưu ở OS keyring; file plaintext chỉ khi user bật Setting rõ ràng, phải private + bounded.
- Không bao giờ log token/credential; `zeroize` khi xoá.
- Không extract credential từ client khác, không bypass captcha/challenge.
- Login/invite verification chỉ dùng webview tạm thời + handoff (xem `docs/authentication.md`,
  `docs/invite-verification.md`).
- Mọi download (updater, extension, avatar) phải verify SHA-256 + độ dài, chặn IP private/6to4.

---

## 9. Test

- Test nằm trong `#[cfg(test)] mod …` cùng file; ưu tiên **ít mà trúng**.
- Cần test: parse wire lỗi, giới hạn byte/item, ranh giới bảo mật, state machine.
- `test-support` chứa fixture tổng hợp; không đưa dữ liệu thật vào test.
- `crates/extensions/examples/legacy_sdk_check.rs` có pin **574 991 byte** cho fixture — đổi fixture
  phải cập nhật pin này và `README` cùng chỗ.

---

## 10. Nếu bạn là agent đang sửa code

1. Đọc `AGENTS.md` trước — nó quy định delivery, `!fast`, test, quyền hạn.
2. Chỉ đọc doc liên quan: `docs/design.md` (UI), `docs/discord-compatibility.md` (protocol),
   `docs/storage-policy.md` (persistence), `docs/platform-support.md` (platform).
3. Sửa xong: `cargo fmt --all`, rồi `cargo xtask check`. Nếu đổi auth → thêm
   `node tests/login-handoff.cjs`. Nếu đổi runtime → `cargo xtask package`.
4. Không commit/push/merge release trừ khi được yêu cầu rõ ràng.
5. Không thêm dependency mới nếu không cần; mọi dependency vào `[workspace.dependencies]`.

---

_Licence: MIT OR Apache-2.0. Copyright 2026 Lumina contributors._
