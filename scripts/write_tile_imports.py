from pathlib import Path

ROOT = Path(r"C:\Users\rsfit\DungeonBarrage\client\src\DungeonBarrage.Client\assets\tiles")

TEMPLATE = """[remap]

importer="texture"
type="CompressedTexture2D"
uid="uid://{uid}"
path="res://.godot/imported/{imported}"
metadata={{
"vram_texture": false
}}

[deps]

source_file="res://assets/tiles/{name}"
dest_files=["res://.godot/imported/{imported}"]

[params]

compress/mode=0
compress/high_quality=false
compress/lossy_quality=0.7
compress/uastc_level=0
compress/rdo_quality_loss=0.0
compress/hdr_compression=1
compress/normal_map=0
compress/channel_pack=0
mipmaps/generate=false
mipmaps/limit=-1
roughness/mode=0
roughness/src_normal=""
process/channel_remap/red=0
process/channel_remap/green=1
process/channel_remap/blue=2
process/channel_remap/alpha=3
process/fix_alpha_border=true
process/premult_alpha=false
process/normal_map_invert_y=false
process/hdr_as_srgb=false
process/hdr_clamp_exposure=false
process/size_limit=0
detect_3d/compress_to=1
"""

UIDS = {
    "soil_fill.png": "b8k2soilfill16bit",
    "grass_cap.png": "c9m3grasscap16bit",
    "stone_fill.png": "d1n4stonefill16bit",
    "wood_fill.png": "e2p5woodfill16bit",
    "sky_dusk.png": "f3q6skydusk16bitx",
    "hills_far.png": "g4r7hillsfar16bit",
    "forest_mid.png": "h5s8forestmid16bt",
}


def main() -> None:
    for name, uid in UIDS.items():
        imported = f"{name}-pixelterrain.ctex"
        (ROOT / f"{name}.import").write_text(
            TEMPLATE.format(uid=uid, imported=imported, name=name),
            encoding="utf-8",
        )
        print("wrote", name)


if __name__ == "__main__":
    main()
