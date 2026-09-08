"""Convert generated Kingdom-style paintings into engine-ready tile PNGs."""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

SESSION = Path(
    r"C:\Users\rsfit\.grok\sessions\C%3A%5CUsers%5Crsfit\01a0566d-7f18-75c3-8a4a-88bd6e00cd6b\images"
)
OUT = Path(r"C:\Users\rsfit\DungeonBarrage\client\src\DungeonBarrage.Client\assets\tiles")


def wrap_blend(arr: np.ndarray, seam: int) -> np.ndarray:
    out = arr.copy()
    h, w = out.shape[:2]
    seam = min(seam, w // 4, h // 4)
    if seam <= 0:
        return out
    for i in range(seam):
        t = (seam - i) / (2.0 * seam)
        out[:, i] = (out[:, i] * (1.0 - t) + arr[:, w - seam + i] * t).astype(np.uint8)
        out[:, w - 1 - i] = (out[:, w - 1 - i] * (1.0 - t) + arr[:, seam - 1 - i] * t).astype(
            np.uint8
        )
        out[i, :] = (out[i, :] * (1.0 - t) + arr[h - seam + i, :] * t).astype(np.uint8)
        out[h - 1 - i, :] = (out[h - 1 - i, :] * (1.0 - t) + arr[seam - 1 - i, :] * t).astype(
            np.uint8
        )
    return out


def wrap_blend_x(arr: np.ndarray, seam: int) -> np.ndarray:
    out = arr.copy()
    h, w = out.shape[:2]
    seam = min(seam, w // 4)
    if seam <= 0:
        return out
    for i in range(seam):
        t = (seam - i) / (2.0 * seam)
        out[:, i] = (out[:, i] * (1.0 - t) + arr[:, w - seam + i] * t).astype(np.uint8)
        out[:, w - 1 - i] = (out[:, w - 1 - i] * (1.0 - t) + arr[:, seam - 1 - i] * t).astype(
            np.uint8
        )
    return out


def pixelate(im: Image.Image, size: tuple[int, int]) -> Image.Image:
    small = im.convert("RGB").resize(size, Image.Resampling.BOX)
    return small


def save_png(arr: np.ndarray, name: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr).save(OUT / name, format="PNG", optimize=True)


def process_fill(src: Path, dest_name: str, size: int = 256) -> None:
    im = pixelate(Image.open(src), (size, size))
    arr = wrap_blend(np.array(im), seam=14)
    save_png(arr, dest_name)


def process_grass(src: Path) -> None:
    im = Image.open(src).convert("RGB")
    w, h = im.size
    cap = im.crop((0, 0, w, int(h * 0.46)))
    cap = pixelate(cap, (256, 96))
    arr = wrap_blend_x(np.array(cap), seam=18)
    save_png(arr, "grass_cap.png")


def process_wide(
    src: Path,
    dest_name: str,
    crop_top: float = 0.0,
    crop_bottom: float = 0.0,
    size: tuple[int, int] = (640, 360),
) -> None:
    im = Image.open(src).convert("RGB")
    w, h = im.size
    top = int(h * crop_top)
    bottom = h - int(h * crop_bottom)
    im = im.crop((0, top, w, max(top + 1, bottom)))
    im = pixelate(im, size)
    save_png(np.array(im), dest_name)


def process_stone_cap(src: Path) -> None:
    im = Image.open(src).convert("RGB")
    w, h = im.size
    cap = im.crop((0, int(h * 0.06), w, int(h * 0.42)))
    cap = pixelate(cap, (256, 96))
    arr = wrap_blend_x(np.array(cap), seam=18)
    save_png(arr, "stone_cap.png")


def main() -> None:
    process_fill(SESSION / "1.jpg", "wood_fill.png")
    process_fill(SESSION / "2.jpg", "soil_fill.png")
    process_wide(SESSION / "3.jpg", "sky_dusk.png", crop_bottom=0.18)
    process_wide(SESSION / "4.jpg", "hills_far.png")
    process_fill(SESSION / "5.jpg", "stone_fill.png")
    process_grass(SESSION / "6.jpg")
    process_wide(
        SESSION / "7.jpg",
        "forest_mid.png",
        crop_top=0.18,
        crop_bottom=0.16,
        size=(640, 280),
    )
    process_stone_cap(SESSION / "8.jpg")


if __name__ == "__main__":
    main()
