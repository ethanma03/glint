"""Generate tests/fixtures: one file per format plus nasty edge cases.

Requires ImageMagick 7 (`magick`) on PATH. Run: python scripts/make_fixtures.py
"""

import shutil
import struct
import subprocess
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "tests" / "fixtures"


def magick(*args: str) -> None:
    subprocess.run(["magick", *args], check=True, cwd=OUT)


def sprite_cmd(fill: str) -> list[str]:
    # 32x32 pixel-art blob with a transparent background and a 1px outline.
    return [
        "-size", "32x32", "xc:none",
        "-fill", fill, "-stroke", "#1a1020", "-strokewidth", "1",
        "-draw", "roundrectangle 6,6 25,25 4,4",
        "-fill", "#ffffff", "-stroke", "none", "-draw", "point 12,12", "-draw", "point 19,12",
    ]


def aseprite(path: Path) -> None:
    """Minimal valid .aseprite: 16x16 RGBA, 1 layer, 3 frames, one tag."""
    w = h = 16

    def string(s: str) -> bytes:
        b = s.encode()
        return struct.pack("<H", len(b)) + b

    def chunk(kind: int, body: bytes) -> bytes:
        return struct.pack("<IH", len(body) + 6, kind) + body

    def frame(chunks: list[bytes], duration: int) -> bytes:
        body = b"".join(chunks)
        return struct.pack("<IHHH2sI", 16 + len(body), 0xF1FA, len(chunks), duration, b"\0\0", len(chunks)) + body

    layer = chunk(0x2004, struct.pack("<HHHHHHB3s", 1, 0, 0, 0, 0, 0, 255, b"\0" * 3) + string("Layer 1"))
    tags = chunk(0x2018, struct.pack("<H8s", 1, b"\0" * 8)
                 + struct.pack("<HHBH6s3sB", 0, 1, 0, 0, b"\0" * 6, b"\xff\x00\x00", 0) + string("walk"))
    frames = []
    for i, color in enumerate([(230, 60, 60, 255), (60, 200, 90, 255), (70, 110, 240, 255)]):
        px = bytearray(w * h * 4)
        for y in range(4, 12):
            for x in range(4 + i, 12 + i):
                o = (y * w + x) * 4
                px[o:o + 4] = bytes(color)
        cel = chunk(0x2005, struct.pack("<HhhBHh5s", 0, 0, 0, 255, 2, 0, b"\0" * 5)
                    + struct.pack("<HH", w, h) + zlib.compress(bytes(px)))
        chunks = ([layer, tags] if i == 0 else []) + [cel]
        frames.append(frame(chunks, 120))
    body = b"".join(frames)
    header = struct.pack("<IHHHHHIHII B3s H BB hhHH 84s",
                         128 + len(body), 0xA5E0, len(frames), w, h, 32, 1, 100, 0, 0,
                         0, b"\0" * 3, 0, 1, 1, 0, 0, 16, 16, b"\0" * 84)
    assert len(header) == 128, len(header)
    path.write_bytes(header + body)


def add_exif_orientation(path: Path, value: int) -> None:
    """Insert an APP1 Exif segment holding only an Orientation tag (magick won't write one)."""
    ifd = struct.pack("<H", 1) + struct.pack("<HHIHH", 0x0112, 3, 1, value, 0) + struct.pack("<I", 0)
    payload = b"Exif\x00\x00" + b"II*\x00" + struct.pack("<I", 8) + ifd
    app1 = b"\xff\xe1" + struct.pack(">H", len(payload) + 2) + payload
    data = path.read_bytes()
    assert data[:2] == b"\xff\xd8"
    path.write_bytes(data[:2] + app1 + data[2:])


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for f in OUT.iterdir():
        f.unlink()

    magick(*sprite_cmd("#e8553e"), "sprite.png")
    magick("sprite.png", "-fill", "#3ee86b", "-draw", "color 15,20 point", "sprite_b.png")
    # 4x2 sheet of 32x32 cells; the last cell is empty.
    magick("-size", "128x64", "xc:none",
           *[a for i in range(7) for a in ("-fill", f"hsl({i * 50},80%,55%)", "-draw",
             f"circle {i % 4 * 32 + 16},{i // 4 * 32 + 16} {i % 4 * 32 + 16},{i // 4 * 32 + 4 + i}")],
           "sheet.png")
    frames = [a for i in range(4) for a in ("(", "-size", "24x24", "xc:none", "-fill", f"hsl({i * 90},80%,55%)",
                                             "-draw", f"rectangle {i * 4},4 {i * 4 + 8},20", ")")]
    magick("-dispose", "previous", "-delay", "12", *frames, "-loop", "0", "anim.gif")
    magick("anim.gif", "-coalesce", "anim.webp")
    magick("anim.gif", "-coalesce", "APNG:anim.apng")

    magick("-size", "640x400", "plasma:fractal", "-seed", "7", "photo.jpg")
    magick("photo.jpg", "-colorspace", "CMYK", "cmyk.jpg")
    magick("-size", "300x200", "gradient:#f60-#06f", "-strip", "exif_rotated.jpg")
    add_exif_orientation(OUT / "exif_rotated.jpg", 6)
    magick("-size", "256x64", "gradient:", "-depth", "16", "gray16.png")
    magick("-size", "200x100", "gradient:red-blue", "-alpha", "set", "-channel", "A", "-evaluate", "set", "60%",
           "+channel", "rgba16.png")
    magick("photo.jpg", "-resize", "200x", "photo.webp")
    magick("sprite.png", "photo.qoi")
    magick("sprite.png", "sprite.tga")
    magick("sprite.png", "sprite.bmp")
    magick("sprite.png", "-define", "dds:compression=none", "sprite.dds")
    magick("sprite.png", "-define", "icon:auto-resize=64,32,16", "multi.ico")
    magick("photo.jpg", "-resize", "200x", "photo.avif")
    magick("photo.jpg", "-resize", "200x", "photo.jxl")
    magick("sprite.png", "sheet.png", "-adjoin", "pages.tiff")
    magick("-size", "64x64", "gradient:", "-colorspace", "RGB", "-evaluate", "multiply", "4", "-depth", "16",
           "-define", "exr:color-type=RGBA", "bright.exr")
    magick("sprite.png", "layers.psd")
    # 17000px wide: forces texture tiling on GPUs with 16384 max texture size.
    magick("-size", "17000x300", "gradient:#222-#ddd", "-depth", "8", "wide.png")

    (OUT / "icon.svg").write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">'
        '<circle cx="12" cy="12" r="10" fill="#f5b84a"/><path d="M7 12l3 3 7-7" stroke="#141417" '
        'stroke-width="2.5" fill="none"/></svg>')
    aseprite(OUT / "walk.aseprite")

    # Edge cases.
    shutil.copy(OUT / "photo.jpg", OUT / "actually_jpeg.png")       # wrong extension
    shutil.copy(OUT / "sprite.png", OUT / "no_extension")           # no extension
    shutil.copy(OUT / "sprite.png", OUT / "brackets [1].png")       # magick frame-syntax lookalike
    shutil.copy(OUT / "sprite.png", OUT / "ünïcödé スプライト.png")   # unicode path
    (OUT / "empty.png").write_bytes(b"")
    data = (OUT / "photo.jpg").read_bytes()
    (OUT / "truncated.jpg").write_bytes(data[: len(data) // 3])
    png = (OUT / "wide.png").read_bytes()
    (OUT / "truncated.png").write_bytes(png[: len(png) // 2])
    (OUT / "garbage.png").write_bytes(b"this is not an image at all\n" * 20)
    print(f"wrote {len(list(OUT.iterdir()))} fixtures to {OUT}")


if __name__ == "__main__":
    main()
