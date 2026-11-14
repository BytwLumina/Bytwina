"""Export the Bytwina app icon set from the designer's mark.

Development tool. Two modes, both run from the repository root:

    # Recompose the plate icon from the mark artwork and rebuild every size.
    python tools/generate-app-icon.py --mark assets/brand/bytwina-mark-source.png

    # Rebuild the sizes from an already plated 1024 px icon.
    python tools/generate-app-icon.py path/to/plated-1024.png

`--mark` also rewrites `bytwina-mark.png` (the mark on transparency),
`bytwina-mark-mono.png` (the white silhouette the interface tints) and
`bytwina-tray.png` (the black 72 px menu bar template).
"""

import argparse
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
# Linux needs two sizes the ladder does not otherwise carry.
LINUX_SIZES = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512, 1024]
BRAND = Path("assets/brand")
HICOLOR = Path("packaging/linux/hicolor")
CANVAS = 1024
# The plate design: a rounded square with the mark at 62 % of its width.
PLATE_RADIUS = 230
MARK_SHARE = 0.62
# Luminance is compressed so the pale comet trail still reads as a silhouette.
MONO_GAMMA = 0.55
MONO_FLOOR = 0.05


def mono_alpha(mark):
    """Alpha that keeps the artwork's tone: the ink is opaque, the trail is faint."""
    luminance = np.asarray(mark.convert("L")).astype(np.float32) / 255.0
    dark = mark.convert("L").getextrema()[0] / 255.0
    alpha = np.clip((1.0 - luminance) / max(1.0 - dark, 1e-6), 0.0, 1.0) ** MONO_GAMMA
    alpha[alpha < MONO_FLOOR] = 0.0
    return Image.fromarray((alpha * 255).round().astype(np.uint8), "L")


def plate(mark):
    """White rounded plate carrying the trimmed mark, centred."""
    alpha = mono_alpha(mark)
    left, top, right, bottom = alpha.getbbox()
    trimmed = mark.convert("RGB").crop((left, top, right, bottom))
    box = round(CANVAS * MARK_SHARE)
    scale = box / max(trimmed.size)
    resized = trimmed.resize(
        (round(trimmed.width * scale), round(trimmed.height * scale)), Image.LANCZOS
    )
    icon = Image.new("RGBA", (CANVAS, CANVAS), (255, 255, 255, 255))
    icon.paste(
        resized,
        ((CANVAS - resized.width) // 2, (CANVAS - resized.height) // 2),
    )
    # Draw the rounded mask larger, then downsample, so the corners stay smooth.
    scale = 4
    mask = Image.new("L", (CANVAS * scale, CANVAS * scale), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, CANVAS * scale - 1, CANVAS * scale - 1),
        radius=PLATE_RADIUS * scale,
        fill=255,
    )
    icon.putalpha(mask.resize((CANVAS, CANVAS), Image.LANCZOS))
    return icon


def write_ladder(icon):
    BRAND.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        resized = icon.resize((size, size), Image.LANCZOS)
        resized.save(BRAND / f"bytwina-{size}.png", optimize=True)
        print(f"wrote assets/brand/bytwina-{size}.png")

    # The window icon ships at 512/256; keep the historical file names for packaging.
    icon.resize((512, 512), Image.LANCZOS).save(BRAND / "bytwina.png", optimize=True)
    print("wrote assets/brand/bytwina.png (512)")

    icon.save(
        Path("packaging/windows/Bytwina.ico"),
        sizes=[(size, size) for size in ICO_SIZES],
    )
    print("wrote packaging/windows/Bytwina.ico")

    # `apps/desktop` loads the window icon from the packaged copies.
    icon.resize((256, 256), Image.LANCZOS).save(
        Path("packaging/windows/bytwina.png"), optimize=True
    )
    print("wrote packaging/windows/bytwina.png (256)")
    for size in LINUX_SIZES:
        directory = HICOLOR / f"{size}x{size}" / "apps"
        directory.mkdir(parents=True, exist_ok=True)
        icon.resize((size, size), Image.LANCZOS).save(directory / "bytwina.png", optimize=True)
    print(f"wrote {len(LINUX_SIZES)} packaging/linux/hicolor sizes")


def write_mark(mark):
    alpha = mono_alpha(mark)
    transparent = mark.convert("RGB").copy()
    transparent.putalpha(alpha)
    transparent.save(BRAND / "bytwina-mark.png", optimize=True)
    print("wrote assets/brand/bytwina-mark.png")

    silhouette = Image.new("RGBA", mark.size, (255, 255, 255, 0))
    silhouette.putalpha(alpha)
    silhouette.save(BRAND / "bytwina-mark-mono.png", optimize=True)
    print("wrote assets/brand/bytwina-mark-mono.png")

    # macOS menu bar template: only the alpha matters, so it is drawn black.
    tray = Image.new("RGBA", (72, 72), (0, 0, 0, 0))
    tray.putalpha(alpha.resize((72, 72), Image.LANCZOS))
    tray.save(BRAND / "bytwina-tray.png", optimize=True)
    print("wrote assets/brand/bytwina-tray.png (72)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", help="1024 px PNG with an alpha channel")
    parser.add_argument("--mark", help="mark artwork on a flat white background")
    args = parser.parse_args()

    if args.mark:
        mark = Image.open(args.mark).convert("RGB")
        if mark.width != mark.height or mark.width < 256:
            raise SystemExit(f"mark must be square and at least 256 px, got {mark.size}")
        if mark.size != (CANVAS, CANVAS):
            mark = mark.resize((CANVAS, CANVAS), Image.LANCZOS)
        write_mark(mark)
        write_ladder(plate(mark))
        return

    if not args.source:
        parser.error("a plated source or --mark is required")
    source = Image.open(args.source).convert("RGBA")
    if source.size != (CANVAS, CANVAS):
        raise SystemExit(f"source must be 1024x1024, got {source.size}")
    write_ladder(source)


if __name__ == "__main__":
    main()
