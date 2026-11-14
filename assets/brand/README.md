# Bytwina brand assets

Bytwina's own application mark: a monochrome comet trail with a four-point starburst, a trailing
star and sparks. The artwork is flat ink layers on white — ink `#141417`, then `#4B4B53`,
`#9A9AA2` and a `#E1E1E1` comet trail at reduced opacity — and it is not Apple, Discord or any
other vendor's branding.

The mark is authored as raster art, so the 1024 px PNG is the master and the SVG is traced from it.

- `bytwina-mark-source.png` — the designer's 1024×1024 mark on a white background. Every other
  file here is generated from it; replace this file to rebrand.
- `bytwina-mark.svg` — vector trace of the mark (view box `100 63.5 828 828`), four paths per
  tone using `fill="#fff"` with `fill-opacity`. `crates/platform` rewrites that placeholder fill
  into the desktop's foreground colour for the Linux symbolic tray icon, so the string
  `fill="#fff"` must survive edits.
- `bytwina-mark.png` — the mark on transparency, resampled from the source.
- `bytwina-mark-mono.png` — white silhouette with per-tone alpha. `tools/generate-icons.py` pastes
  this into the shared icon atlas as `bytwina-mark`, which the interface tints at draw time.
- `bytwina-1024.png` — 1024×1024 sRGB RGBA application icon: the mark at 62 % of a white rounded
  plate (radius 230 of 1024). The rest of the `bytwina-{16,24,32,48,64,128,256,512}.png` ladder is
  resampled from it.
- `bytwina-tray.png` — 72×72 black-on-transparent render of the mark, embedded by `crates/platform`
  as the macOS menu bar template image; macOS tints it per appearance, so only its alpha is used.

Regenerate every raster, the Windows `.ico` and the packaged copies from the source mark:

```sh
python3 tools/generate-app-icon.py --mark assets/brand/bytwina-mark-source.png
```

The packaged platform icons built from the same artwork live in `packaging/` (`macos/Bytwina.icns`
and `Bytwina.icon`, `windows/Bytwina.ico`, `linux/hicolor/*`). The Windows icon and the
`packaging/linux/hicolor` ladder are written by the command above; the macOS `.icns` is not, and
needs the Apple icon toolchain.

The interface accent (`design::DEFAULT_PRIMARY_RGB`, `#1a72e8`) is separate from the mark, which is
always drawn in the fixed brand black/white pair, and it is tuned so white text on it keeps a
4.5:1 contrast ratio.
