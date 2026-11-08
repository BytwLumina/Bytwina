# Bytwina brand assets

Bytwina's own application mark: a monochrome comet trail with a four-point starburst, a trailing
star and sparks. It is drawn from flat ink colours only — no gradients and no embedded raster
image — and it is not Apple, Discord or any other vendor's branding.

- `bytwina.svg` — light rounded plate carrying the mark; the master vector source and the shape
  used for the raster previews.
- `bytwina-flat.svg` — dark rounded plate variant for dark surfaces and low-colour contexts.
- `bytwina-mark.svg` — solid silhouette trimmed to its bounding box, view box `148 141 760 760`.
  `tools/generate-icons.py` rasterizes this one into the shared icon atlas as `bytwina-mark`,
  which the interface tints at draw time.
- `bytwina-1024.png` — 1024×1024 sRGB RGBA preview render of the light plate icon.
- `bytwina-tray.png` — 72×72 black-on-transparent render of `bytwina-mark.svg`, embedded by
  `crates/platform` as the macOS menu bar template image; macOS tints it per appearance, so only
  its alpha is used. Regenerate with:

  ```sh
  sed -e 's/fill="#fff"/fill="#000"/g' -e 's/stroke="#fff"/stroke="#000"/g' \
    assets/brand/bytwina-mark.svg |
    rsvg-convert -w 72 -h 72 -o assets/brand/bytwina-tray.png
  ```

The packaged platform icons built from the same artwork live in `packaging/` (`macos/Bytwina.icns`
and `Bytwina.icon`, `windows/Bytwina.ico`, `linux/hicolor/*`). Regenerate the PNG sizes and the
Windows icon from the 1024 px render:

```sh
python tools/generate-app-icon.py assets/brand/bytwina-1024.png
```

Brand palette: ink `#111111`, trailing grays `#3C3C3C`, `#6A6A6A`, comet trail `#D9D9D9`, plate
white `#FFFFFF` and plate ink `#111111`. The interface accent
(`design::DEFAULT_PRIMARY_RGB`, `#1a72e8`) is unchanged and is tuned so white text on it keeps a
4.5:1 contrast ratio.
