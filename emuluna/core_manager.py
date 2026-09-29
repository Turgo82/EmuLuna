"""Install official Linux Libretro builds without modifying RetroArch's files."""
import argparse
import ctypes as C
from contextlib import contextmanager
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time
import zipfile

from PySide6.QtCore import QThread, Signal
from .artwork import Downloads, Cancelled, atomic_bytes, MIB
from .core import CoreError, ROOT
from .systems import SYSTEMS, CATALOG, infer_core_systems

BUILDBOT = "https://buildbot.libretro.com/nightly/linux/x86_64/latest/"


def probe(path):
    """Load metadata in a disposable child so a bad .so cannot crash Settings."""
    command = ([sys.executable, "--core-probe", str(path)] if getattr(sys, "frozen", False)
               else [sys.executable, "-m", "emuluna.core_manager", "--probe", str(path)])
    process = subprocess.run(command,
                             cwd=ROOT, capture_output=True, timeout=20)
    for line in process.stdout.decode(errors="replace").splitlines():
        if process.returncode == 0 and line.startswith("EMULUNA_CORE_INFO "):
            return json.loads(line.removeprefix("EMULUNA_CORE_INFO "))
    raise CoreError("This core could not be loaded on this Linux system. " + process.stderr.decode(errors="replace")[-600:])


def validate_elf(data):
    machine = {"x86_64": 62, "amd64": 62, "aarch64": 183, "arm64": 183}.get(platform.machine().lower())
    if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01" or int.from_bytes(data[18:20], "little") != machine:
        raise CoreError("Choose a 64-bit Linux core built for this computer’s processor.")


class CoreManager:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.directory = self.root / "cores"
        self.directory.mkdir(parents=True, exist_ok=True)
        self.manifest = self.directory / "installed.json"
        self.history_manifest = self.directory / "history.json"

    def installed(self):
        if not self.manifest.exists():
            return {}
        records = json.loads(self.manifest.read_text())
        if not isinstance(records, dict):
            raise CoreError("The installed core list is damaged.")
        return records

    def history(self, core_id=None):
        """Return retained builds, including binaries kept by older releases."""
        try:
            records = json.loads(self.history_manifest.read_text())
        except (OSError, ValueError):
            records = {}
        if not isinstance(records, dict):
            records = {}
        installed = self.installed()
        keys = [core_id] if core_id else list(set(records) | set(installed))
        result = {}
        for key in keys:
            versions = records.get(key, [])
            if not isinstance(versions, list):
                versions = []
            current = installed.get(key)
            known = {item.get("sha256") for item in versions if isinstance(item, dict)}
            if current:
                known.add(current.get("sha256"))
                directory = self.directory / key
                if directory.is_dir() and not directory.is_symlink():
                    for path in directory.glob("*.so"):
                        match = re.fullmatch(r"([0-9a-f]{64})\.so", path.name)
                        if not match or match.group(1) in known:
                            continue
                        digest = match.group(1)
                        versions.append({**current, "version": "Retained build", "build": None,
                                         "path": str(path.relative_to(self.root)), "sha256": digest,
                                         "source": current.get("source", "retained"),
                                         "installed": path.stat().st_mtime})
                        known.add(digest)
            usable = [item for item in versions if isinstance(item, dict)
                      and item.get("sha256") not in (None, current.get("sha256") if current else None)
                      and (self.root / item.get("path", "")).is_file()]
            if usable:
                result[key] = usable
        return result.get(core_id, []) if core_id else result

    def _write_history(self, records):
        atomic_bytes(self.history_manifest, json.dumps(records, indent=2).encode())

    def path(self, record):
        path = (self.root / record["path"]).resolve()
        if not path.is_relative_to(self.directory) or not path.is_file():
            raise CoreError("The selected core is missing. Download it again in Settings.")
        if hashlib.sha256(path.read_bytes()).hexdigest() != record["sha256"]:
            raise CoreError("The selected core file is damaged. Download it again in Settings.")
        return path

    @contextmanager
    def locked(self):
        with (self.directory / ".install.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            yield

    def install_bytes(self, data, core_id, *, source="local", build=None, crc=None, systems=None, catalog_id=None, started=None):
        if len(data) > 128 * MIB:
            raise CoreError("The core file is too large.")
        if not re.fullmatch(r"[a-z0-9_-]+", core_id):
            raise CoreError("Invalid core identifier")
        validate_elf(data)
        digest = hashlib.sha256(data).hexdigest()
        with tempfile.TemporaryDirectory(prefix=".install-", dir=self.directory) as tmp:
            candidate = Path(tmp) / "core.so"
            candidate.write_bytes(data)
            info = probe(candidate)
            extensions = info["extensions"].lower().split("|")
            catalog_id = catalog_id or (core_id if core_id in CATALOG else None)
            if catalog_id is None:
                matches = [key for key, item in CATALOG.items() if item['name'].casefold() == info['name'].casefold()]
                if len(matches) == 1:
                    catalog_id = matches[0]
            allowed = systems if systems is not None else CATALOG.get(catalog_id, {}).get("systems")
            supported = infer_core_systems(extensions, allowed)
            if not supported:
                raise CoreError("This core could not be associated with a system. Choose its system when importing it.")
            target = self.directory / core_id / (digest + ".so")
            record = {"id": core_id, "catalog_id": catalog_id, "name": info["name"], "version": info["version"], "systems": supported,
                      "path": str(target.relative_to(self.root)), "sha256": digest, "source": source,
                      "extensions": extensions, "build": build, "crc": crc, "installed": time.time()}
            with self.locked():
                target.parent.mkdir(parents=True, exist_ok=True)
                removed = target.parent / '.removed'
                if started is not None and removed.exists() and float(removed.read_text()) >= started:
                    raise Cancelled('The core was removed while its download was running.')
                removed.unlink(missing_ok=True)
                # Immutable builds let running games finish on their current version.
                if not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != digest:
                    os.replace(candidate, target)
                records = self.installed()
                history = self.history()
                previous = records.get(core_id)
                if previous and previous.get("sha256") != digest:
                    versions = [previous] + [item for item in history.get(core_id, [])
                                                     if item.get("sha256") != digest]
                    history[core_id] = list({item["sha256"]: item for item in reversed(versions)}.values())[::-1]
                    self._write_history(history)
                records[core_id] = record
                atomic_bytes(self.manifest, json.dumps(records, indent=2).encode())
        return record

    def restore(self, core_id, digest):
        """Make a retained core build current while keeping the newer build."""
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise CoreError("The selected core backup is invalid.")
        with self.locked():
            records = self.installed()
            current = records.get(core_id)
            if not current:
                raise CoreError("Install this core before restoring an older build.")
            versions = self.history(core_id)
            selected = next((item for item in versions if item.get("sha256") == digest), None)
            if not selected:
                raise CoreError("That core backup is no longer available.")
            path = (self.root / selected["path"]).resolve()
            if (not path.is_relative_to(self.directory) or not path.is_file()
                    or hashlib.sha256(path.read_bytes()).hexdigest() != digest):
                raise CoreError("That core backup is damaged.")
            info = probe(path)
            selected = {**selected, "name": info["name"], "version": info["version"],
                        "extensions": info["extensions"].lower().split("|"), "restored": time.time()}
            history = self.history()
            history[core_id] = [current] + [item for item in versions if item.get("sha256") != digest]
            self._write_history(history)
            records[core_id] = selected
            atomic_bytes(self.manifest, json.dumps(records, indent=2).encode())
        return selected

    def import_file(self, path, system=None):
        path = Path(path)
        with path.open("rb") as file:
            data = file.read(128 * MIB + 1)
        digest = hashlib.sha256(data).hexdigest()
        name = path.name.removesuffix(".so").removesuffix("_libretro")
        allowed = [system] if system else CATALOG.get(name, {}).get("systems")
        return self.install_bytes(data, "local-" + digest[:16], systems=allowed, catalog_id=name if name in CATALOG else None)

    def remote_index(self, downloads):
        if platform.machine().lower() not in ("x86_64", "amd64"):
            raise CoreError("Automatic downloads currently support Linux x86_64. You can import a native .so core on this processor.")
        text = downloads.get(BUILDBOT + ".index-extended", 4 * MIB).decode()
        entries = {}
        for line in text.splitlines():
            parts = line.split()
            if len(parts) == 3:
                build, crc, filename = parts
                if re.fullmatch(r"[0-9a-fA-F]{8}", crc):
                    entries[filename] = {"build": build, "crc": crc.lower()}
        return entries

    def download(self, core_id, downloads, progress=lambda _: None, index=None):
        started = time.time()
        if core_id not in CATALOG:
            raise CoreError("This core is not in the download catalog.")
        index = self.remote_index(downloads) if index is None else index
        filename = core_id + "_libretro.so"
        remote = index.get(filename + ".zip")
        if not remote:
            raise CoreError("This core is temporarily unavailable from the official build server.")
        current = self.installed().get(core_id)
        if current and current.get("crc") == remote["crc"]:
            try:
                if hashlib.sha256(self.path(current).read_bytes()).hexdigest() == current["sha256"]:
                    return current, False
            except CoreError:
                pass
        progress(f"Downloading {CATALOG[core_id]['name']}…")
        data = downloads.get(BUILDBOT + filename + ".zip", 64 * MIB)
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            entry = archive.getinfo(filename)
            if entry.file_size > 128 * MIB or f"{entry.CRC:08x}" != remote["crc"]:
                raise CoreError("The downloaded core does not match the server’s index. Try updating again.")
            with archive.open(entry) as file:
                binary = file.read(128 * MIB + 1)
        downloads.check()
        progress(f"Checking and installing {CATALOG[core_id]['name']}…")
        record = self.install_bytes(binary, core_id, source="buildbot", systems=CATALOG[core_id]["systems"], started=started, **remote)
        return record, True

    def remove(self, core_id, library):
        """Remove managed binaries only. Running Linux mappings and saves survive."""
        with self.locked():
            records = self.installed()
            record = records.get(core_id)
            if not record:
                return False
            directory = self.directory / core_id
            if not re.fullmatch(r"[a-z0-9_-]+", core_id) or directory.is_symlink():
                raise CoreError("The core location is invalid; nothing was removed.")
            directory.mkdir(parents=True, exist_ok=True)
            atomic_bytes(directory / '.removed', str(time.time()).encode())
            library.set_setting("core.removed." + core_id, "1")
            del records[core_id]
            atomic_bytes(self.manifest, json.dumps(records, indent=2).encode())
            history = self.history()
            history.pop(core_id, None)
            self._write_history(history)
            if directory.is_dir():
                for path in directory.iterdir():
                    if re.fullmatch(r"[0-9a-f]{64}\.so", path.name):
                        path.unlink(missing_ok=True)
            for system in record['systems']:
                if library.setting("core." + system) == core_id:
                    library.set_setting("core." + system, "auto")
        return True

    def missing_defaults(self, library, systems):
        if library.setting('core_auto_install', '1') != '1':
            return []
        records = self.installed()
        needed = []
        for system in systems:
            if system not in SYSTEMS:
                continue
            choice = library.setting('core.' + system, 'auto')
            if choice not in ('auto', 'builtin', ''):
                continue
            compatible = [record for record in records.values() if system in record['systems']]
            if compatible:
                continue
            core_id = SYSTEMS[system].default_core
            if library.setting('core.removed.' + core_id, '0') == '1':
                continue
            if core_id not in needed:
                needed.append(core_id)
        return needed

    def choice(self, library, system):
        choice = library.setting("core." + system, "auto")
        records = self.installed()
        if choice in ("builtin", "auto", ""):
            preferred = SYSTEMS[system].default_core
            choices = [preferred] + [key for key in records if key != preferred]
            record = next((records[key] for key in choices if key in records and system in records[key]["systems"]), None)
            if record is None:
                raise CoreError(f"No core is installed for {SYSTEMS[system].name}. Open Settings → Cores and download a compatible core.")
        else:
            record = records.get(choice)
            if not record or system not in record["systems"]:
                raise CoreError("The selected core is unavailable. Right-click the console in the sidebar and choose another core.")
        return record

    def selection(self, library, system):
        record = self.choice(library, system)
        self.path(record)
        return record

    def build(self, core_id, digest, system=None):
        """Resolve one exact installed or retained build without changing defaults."""
        current = self.installed().get(core_id)
        versions = ([current] if current else []) + self.history(core_id)
        record = next((item for item in versions if item.get("sha256") == digest), None)
        if not record:
            raise CoreError("The core build required by this save state is no longer installed.")
        if system and system not in record.get("systems", []):
            raise CoreError("The core build required by this save state does not support this system.")
        self.path(record)
        return record

    def state_build(self, state_path, game_id, system):
        """Resolve the exact retained core encoded by a versioned state path."""
        path = Path(state_path).resolve()
        root = (self.root / "states" / game_id).resolve()
        try:
            parts = path.relative_to(root).parts
        except ValueError:
            raise CoreError("This save state belongs to a different game.")
        if (len(parts) != 4 or parts[0] != "libretro" or
                not re.fullmatch(r"[0-9a-f]{64}", parts[2]) or
                path.suffix != ".oesavestate" or not path.is_file()):
            raise CoreError("This save state does not identify a compatible core build.")
        return self.build(parts[1], parts[2], system)

    def validate_launch(self, record, system, rom, bios_directory):
        from .bios import validate_bios
        catalog_id = record.get("catalog_id") or record["id"]
        blocked = CATALOG.get(catalog_id, {}).get("launch_block")
        if blocked:
            raise CoreError(blocked)
        validate_bios(catalog_id, system, bios_directory)



class CoreWorker(QThread):
    progress = Signal(str)
    result = Signal(str, bool)

    def __init__(self, root, *, core_id=None, local_file=None, system=None, update_all=False, install_all=False):
        super().__init__()
        self.root, self.core_id, self.local_file, self.update_all = root, core_id, local_file, update_all
        self.system = system
        self.install_all = install_all

    def run(self):
        try:
            manager = CoreManager(self.root)
            downloads = Downloads(self.isInterruptionRequested)
            if self.local_file:
                self.progress.emit("Checking the selected core…")
                manager.import_file(self.local_file, self.system)
                message = "Core imported. Right-click a console in the sidebar to select it."
            else:
                index = manager.remote_index(downloads)
                ids = list(CATALOG) if self.install_all else [key for key, value in manager.installed().items() if value["source"] == "buildbot"] if self.update_all else [self.core_id]
                changed, failures = 0, []
                for number, core_id in enumerate(ids, 1):
                    downloads.check()
                    try:
                        _, updated = manager.download(core_id, downloads, lambda message: self.progress.emit(f"{number}/{len(ids)} · {message}"), index)
                        changed += int(updated)
                    except Cancelled:
                        raise
                    except Exception as error:
                        failures.append(f"{core_id}: {error}")
                message = f"{changed} core{'s' if changed != 1 else ''} installed or updated." if changed else "Installed cores are up to date."
                if failures:
                    self.result.emit(message + "\n" + "\n".join(failures), False)
                    return
            self.result.emit(message, True)
        except Cancelled:
            self.result.emit("Core download cancelled.", True)
        except Exception as error:
            self.result.emit(str(error), False)


def probe_child(path):
    class Info(C.Structure):
        _fields_ = [("name", C.c_char_p), ("version", C.c_char_p), ("extensions", C.c_char_p),
                    ("need_fullpath", C.c_bool), ("block_extract", C.c_bool)]
    core = C.CDLL(str(Path(path).resolve()))
    required = ["init", "deinit", "api_version", "get_system_info", "get_system_av_info", "set_environment",
                "set_video_refresh", "set_audio_sample", "set_audio_sample_batch", "set_input_poll", "set_input_state",
                "set_controller_port_device", "load_game", "unload_game", "run", "reset", "serialize_size",
                "serialize", "unserialize", "get_memory_data", "get_memory_size"]
    for name in required:
        getattr(core, "retro_" + name)
    core.retro_api_version.restype = C.c_uint
    if core.retro_api_version() != 1:
        raise CoreError("Unsupported Libretro API version")
    core.retro_get_system_info.argtypes = [C.POINTER(Info)]
    core.retro_get_system_info.restype = None
    info = Info()
    core.retro_get_system_info(C.byref(info))
    decode = lambda value: (value or b"").decode(errors="replace")
    print("EMULUNA_CORE_INFO " + json.dumps({"name": decode(info.name), "version": decode(info.version),
                                            "extensions": decode(info.extensions)}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", required=True)
    args = parser.parse_args()
    probe_child(args.probe)


class DefaultCoreWorker(QThread):
    progress = Signal(str)
    result = Signal(str, bool)

    def __init__(self, root, systems):
        super().__init__()
        self.root, self.systems = root, systems

    def run(self):
        from .library import Library
        library = Library(self.root)
        try:
            manager = CoreManager(self.root)
            ids = manager.missing_defaults(library, self.systems)
            if not ids:
                self.result.emit('', True)
                return
            downloads = Downloads(self.isInterruptionRequested)
            index = manager.remote_index(downloads)
            failures, installed = [], 0
            for number, core_id in enumerate(ids, 1):
                downloads.check()
                # Recheck preferences after each download; a user may remove a
                # core or disable automatic installation while this job runs.
                if core_id not in manager.missing_defaults(library, self.systems):
                    continue
                try:
                    manager.download(
                        core_id, downloads,
                        lambda message, number=number: self.progress.emit(
                            f"{number}/{len(ids)} · {message}"),
                        index)
                    installed += 1
                except Cancelled:
                    raise
                except Exception as error:
                    failures.append(f'{CATALOG[core_id]["name"]}: {error}')
            self.result.emit(f'{installed} default core(s) installed.' +
                (' Retry failed downloads in Settings. ' + '; '.join(failures) if failures else ''), not failures)
        except Cancelled:
            self.result.emit('Core downloads cancelled. Retry in Settings → Cores.', False)
        except Exception as error:
            self.result.emit('Default core download failed. Retry in Settings → Cores. ' + str(error), False)
        finally:
            library.close()
