"""Portable system and core catalog; no emulator or Cocoa code in the UI."""
from dataclasses import dataclass
import json
from pathlib import Path

DATA = Path(__file__).resolve().parent / 'data'
CATALOG = json.loads((DATA / 'cores.json').read_text())


def core_launch_options(core_id, system):
    """Return core defaults with options scoped to the emulated system."""
    record = CATALOG.get(core_id, {})
    options = dict(record.get("options", {}))
    options.update(record.get("system_options", {}).get(system, {}))
    return options


def core_render_options(core_id, gl_capabilities, *, vulkan_available=False,
                        resuming=False, saved_renderer=None):
    """Choose a core's GPU or software options without changing its display API."""
    record = CATALOG.get(core_id, {})
    if not record.get("hardware_options") and not record.get("vulkan_options"):
        return {}, False, False
    compatibility_gl, _, core_gl, _ = gl_capabilities
    profile = record.get("hardware_profile", "either")
    gl_available = bool(record.get("hardware_options")) and (bool(core_gl) if profile == "core" else
                    bool(compatibility_gl) if profile == "compat" else
                    bool(compatibility_gl or core_gl))
    vk_available = bool(record.get("vulkan_options") and vulkan_available)
    if resuming:
        key = ("vulkan_options" if saved_renderer == "vulkan" and vk_available else
               "hardware_options" if saved_renderer == "opengl" and gl_available else
               "software_options")
    else:
        key = "vulkan_options" if vk_available else "hardware_options" if gl_available else "software_options"
    return dict(record.get(key, {})), gl_available or vk_available, key != "software_options"


@dataclass(frozen=True)
class System:
    key: str
    name: str
    openvgdb_id: str
    extensions: tuple
    color: str
    icon: Path
    default_core: str
    thumbnail: str
    media: str
    input_profile: str = "default"
    cover_size: tuple = (256, 256)


SYSTEMS = {key: System(key=key, **{**record, 'extensions': tuple(record['extensions']),
           'icon': DATA / 'icons' / record['icon'],
           'cover_size': tuple(record.get('cover_size', (256, 256)))})
           for key, record in sorted(json.loads((DATA / 'systems.json').read_text()).items(),
                                     key=lambda pair: pair[1]['name'].casefold())}
EXTENSION_SYSTEMS = {}
for key, system in SYSTEMS.items():
    for ext in system.extensions:
        EXTENSION_SYSTEMS.setdefault('.' + ext, []).append(key)
# Ambiguous extensions are recognized, but never silently assigned to a console.
EXTENSIONS = {ext: keys[0] if len(keys) == 1 else None for ext, keys in EXTENSION_SYSTEMS.items()}


def infer_core_systems(extensions, allowed=None):
    extensions = set(extensions)
    if allowed is not None:
        return [key for key in SYSTEMS if key in allowed and extensions.intersection(SYSTEMS[key].extensions)]
    # A local unknown core advertising only generic BIN/ROM/disc formats requires
    # explicit association, not every console that happens to use that extension.
    return [key for key in SYSTEMS if any(ext in extensions and len(EXTENSION_SYSTEMS['.' + ext]) == 1
                                        for ext in SYSTEMS[key].extensions)]
