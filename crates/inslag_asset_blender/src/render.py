import hashlib
import json
import sys
from pathlib import Path

import bpy
import gpu


def dependencies():
    result = set()
    for library in bpy.data.libraries:
        result.add(Path(bpy.path.abspath(library.filepath)).resolve())
    for collection in [bpy.data.images, bpy.data.fonts, bpy.data.sounds, bpy.data.movieclips]:
        for data in collection:
            path = getattr(data, "filepath", "")
            if not path or path.startswith("<") or getattr(data, "packed_file", None):
                continue
            if isinstance(data, bpy.types.Image) and data.source != "FILE":
                if data.source in {"SEQUENCE", "TILED", "MOVIE"}:
                    raise ValueError(f"pack or bake external animated/tiled image {data.name}")
                continue
            result.add(Path(bpy.path.abspath(path, library=data.library)).resolve())
    return result


def render(job):
    if tuple(job["version"]) != bpy.app.version:
        raise ValueError(f"expected Blender {job['version']}, found {bpy.app.version_string}")
    bpy.ops.wm.open_mainfile(filepath=job["source"])
    scene = bpy.data.scenes[job["scene"]]
    bpy.context.window.scene = scene
    if scene.render.engine != "BLENDER_EEVEE":
        raise ValueError("source scene must select EEVEE; no fallback renderer is used")
    layer = scene.view_layers[job["layer"]]
    for other in scene.view_layers:
        other.use = other == layer
    bpy.context.window.view_layer = layer
    layer.use_pass_z = True
    if job["resolution"]:
        scene.render.resolution_x, scene.render.resolution_y = job["resolution"]
    scene.render.resolution_percentage = 100
    pixels = scene.render.resolution_x * scene.render.resolution_y * len(job["frames"])
    if pixels == 0 or pixels > 256_000_000:
        raise ValueError("render sequence exceeds 256M pixels")
    paths = dependencies()
    hashes = {path: hashlib.sha256(path.read_bytes()).digest() for path in paths}
    root = Path(job["output"])
    for index, frame in enumerate(job["frames"]):
        scene.frame_set(frame)
        bpy.ops.render.render(layer=layer.name, scene=scene.name)
        result = bpy.data.images["Render Result"]
        scene.render.image_settings.media_type = "IMAGE"
        scene.render.image_settings.file_format = "PNG"
        scene.render.image_settings.color_mode = "RGBA"
        scene.render.image_settings.color_depth = "8"
        result.save_render(str(root / f"{index}.png"), scene=scene)
        scene.render.image_settings.media_type = "MULTI_LAYER_IMAGE"
        scene.render.image_settings.file_format = "OPEN_EXR_MULTILAYER"
        scene.render.image_settings.color_depth = "32"
        result.save_render(str(root / f"{index}.exr"), scene=scene)
        bpy.data.images.remove(result)
    for path, before in hashes.items():
        if hashlib.sha256(path.read_bytes()).digest() != before:
            raise ValueError(f"source dependency changed during rendering: {path}")
    report = {
        "version": bpy.app.version_string,
        "build": bpy.app.build_hash.decode(),
        "gpu": gpu.platform.renderer_get(),
        "dependencies": sorted(str(path) for path in paths),
    }
    (root / "report.json").write_text(json.dumps(report))


if __name__ == "__main__":
    render(json.loads(Path(sys.argv[sys.argv.index("--") + 1]).read_text()))
