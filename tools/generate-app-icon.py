"""Export the Bytwina app icon set from the 1024 px source mark.

Development tool: reads the monochrome starburst mark, writes the multi-size PNGs
and the Windows .ico. Run from the repository root:

    python tools/generate-app-icon.py path/to/Mark.png
"""

import argparse
from pathlib import Path
from PIL import Image

SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
BRAND = Path("assets/brand")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", help="1024 px PNG with an alpha channel")
    args = parser.parse_args()

    source = Image.open(args.source).convert("RGBA")
    if source.size != (1024, 1024):
        raise SystemExit(f"source must be 1024x1024, got {source.size}")

    BRAND.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        resized = source.resize((size, size), Image.LANCZOS)
        resized.save(BRAND / f"bytwina-{size}.png", optimize=True)
        print(f"wrote assets/brand/bytwina-{size}.png")

    # The window icon ships at 256; keep the historical file name for packaging.
    source.resize((512, 512), Image.LANCZOS).save(BRAND / "bytwina.png", optimize=True)
    print("wrote assets/brand/bytwina.png (512)")

    source.save(
        Path("packaging/windows/Bytwina.ico"),
        sizes=[(size, size) for size in ICO_SIZES],
    )
    print("wrote packaging/windows/Bytwina.ico")


if __name__ == "__main__":
    main()
