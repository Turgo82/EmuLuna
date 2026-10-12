"""One place for managed console storage and compatibility with older libraries."""
from pathlib import Path
import json

from .systems import SYSTEMS


def console_root(root):
    root = Path(root).resolve()
    folder = root / "consoles"
    location = root / "console-location.json"
    approved = root / "consoles"
    if location.is_file():
        value = json.loads(location.read_text(encoding="utf-8"))
        if not isinstance(value, dict) or value.get("format") != 1 or not Path(value.get("path", "")).is_absolute():
            raise ValueError("The console storage location is invalid.")
        approved = Path(value["path"])
    # Only a location explicitly selected by the user may leave the app folder.
    if folder.is_symlink() and folder.resolve() != approved.resolve():
        raise ValueError("The console folder does not match the selected library location.")
    if not folder.is_symlink() and approved != root / "consoles":
        raise ValueError("The selected console library is disconnected.")
    return folder


def storage_contains(root, path):
    root = Path(root).resolve()
    return Path(path).resolve().is_relative_to(root) or Path(path).resolve().is_relative_to(console_root(root).resolve())


def console_folder(root, system):
    if system not in SYSTEMS:
        raise ValueError("Unknown console")
    selected = console_root(root)
    if selected.is_symlink() and not selected.is_dir():
        raise ValueError("The console library is disconnected. Reconnect its drive.")
    folder = selected / system
    if not folder.resolve().is_relative_to(console_root(root).resolve()):
        raise ValueError("The console folder points outside the selected library")
    return folder


def game_folders(root, kind, game_id, system):
    if kind not in ("saves", "states", "screenshots"):
        raise ValueError("Unknown game folder")
    if not game_id or Path(game_id).name != game_id or game_id in (".", ".."):
        raise ValueError("Invalid game identifier")
    return (console_folder(root, system) / kind / game_id, Path(root) / kind / game_id)


def state_roots(root, game_id, system):
    return game_folders(root, "states", game_id, system)


def unique_rom_path(folder, name, game_id):
    """Keep readable names; reserve another name instead of overwriting a ROM."""
    if not name or Path(name).name != name or name in (".", ".."):
        raise ValueError("Invalid ROM filename")
    folder = Path(folder)
    candidate = folder / name
    number = 0
    while candidate.exists() or candidate.is_symlink():
        number += 1
        suffix = game_id[:12] + (f"-{number}" if number > 1 else "")
        candidate = folder / f"{Path(name).stem} [{suffix}]{Path(name).suffix}"
    return candidate
