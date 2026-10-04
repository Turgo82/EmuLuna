"""A separate EmuLuna process owns each running game and libretro core."""
import argparse
import json
from datetime import datetime
from pathlib import Path
import sys
import time

from PySide6.QtCore import Qt, QTimer, QLockFile, QSaveFile, QIODevice
from PySide6.QtGui import QAction, QActionGroup, QImage, QKeySequence
from PySide6.QtMultimedia import QAudio, QAudioFormat, QAudioSink, QMediaDevices
from PySide6.QtWidgets import QApplication, QMainWindow, QMessageBox, QComboBox, QStyle, QMenu

from .branding import configure_application
from .theme import follow_system_theme, theme_palette
from .core import Core, CoreError, saved_state_renderer
from .video_filters import ALL_FILTERS, valid_filter
from .shaders import PRESETS, parameter_values
from .shader_controls import ShaderControls
from .core_manager import CoreManager
from .desktop import prefer_native_desktop, restore_window_size, save_window_size
from .systems import CATALOG, core_launch_options, core_render_options
from .gamepad import Gamepad
from .library import Library
from .gameplay import GameplayHUD, GameplayNotice, GameplayDiagnostics, set_hud_icon
from .controls import LAYOUTS, layout_for, keyboard_buttons, keyboard_axes, combined_axes
from .controller_profiles import SPECS, load_profile, keyboard_layout
from .game_mode import GameMode
from .hardware_render import (HardwareCoreDisplay, HardwareRenderError, VulkanCoreDisplay,
                              probe_hardware_contexts, probe_vulkan_context)

KEYS = keyboard_buttons(LAYOUTS["default"])


from .video_screen import Screen


class Player(QMainWindow):
    def __init__(self, library, game_id, muted=False, frame_limit=0, screenshot=None,
                 state_file=None, restart_auto=False, core_id=None, core_sha256=None):
        super().__init__()
        follow_system_theme(library)
        self.setPalette(theme_palette())
        self.closed = False
        self.game_mode = GameMode()
        self.game_mode_enabled = None
        self.library = library
        self.game = library.get(game_id)
        if not self.game:
            raise CoreError("The game is no longer in the library.")
        self.game_id = game_id
        self.lock = QLockFile(str(library.root / "saves" / (game_id + ".lock")))
        self.lock.setStaleLockTime(0)
        if not self.lock.tryLock(0):
            raise CoreError("This game is already running. Close its other window first.")
        self.state_directory = library.root / "states" / game_id
        save_directory = library.root / "saves" / game_id
        try:
            rom_path = library.validate_game(game_id)
            manager = CoreManager(library.root)
            selected = manager.selection(library, self.game["system"])
            if core_id or core_sha256:
                if not (core_id and core_sha256):
                    raise CoreError("The requested save-state core is incomplete.")
                selected = manager.build(core_id, core_sha256, self.game["system"])
            bios_directory = library.setting("bios_directory", str(library.root / "system"))
            manager.validate_launch(selected, self.game["system"], rom_path, bios_directory)
            if selected:
                catalog_id = selected.get("catalog_id") or selected["id"]
                catalog = CATALOG.get(catalog_id, {})
                launch_options = core_launch_options(catalog_id, self.game["system"])
                self.accessory_options = []
                self.runtime_options = []
                for definition in catalog.get("runtime_options", {}).get(self.game["system"], []):
                    record = dict(definition)
                    record["setting"] = f"core_option.{self.game['system']}.{catalog_id}.{record['key']}"
                    valid = {record["enabled"], record["disabled"]}
                    record["value"] = library.setting(record["setting"], record["default"])
                    if record["value"] not in valid:
                        record["value"] = record["default"]
                    launch_options[record["key"]] = record["value"]
                    self.runtime_options.append(record)
                for accessory in catalog.get("accessories", []):
                    count = max(1, min(4, int(accessory.get("count", 1))))
                    defaults = accessory.get("defaults", [accessory.get("default", "")])
                    choices = dict(accessory.get("choices", []))
                    for index in range(count):
                        key = accessory["key"].format(player=index + 1)
                        default = defaults[min(index, len(defaults) - 1)]
                        setting = f"core_option.{game_id}.{selected['id']}.{key}"
                        value = library.setting(setting, default)
                        if value not in choices:
                            value = default
                        launch_options[key] = value
                        self.accessory_options.append({
                            "group": accessory["label"], "count": count, "player": index + 1,
                            "key": key, "setting": setting, "value": value,
                            "choices": tuple(accessory.get("choices", [])),
                            "restart": bool(accessory.get("restart", False)),
                        })
                self.state_directory /= "libretro/" + selected["id"]
                self.state_directory /= selected["sha256"]
                save_directory /= "libretro/" + selected["id"]
                options = {"libretro_path": manager.path(selected), "core_id": selected["id"],
                           "system_dir": bios_directory, "content_digest": game_id,
                           "options": launch_options}
            if state_file and restart_auto:
                raise CoreError("Choose either Resume or Restart.")
            # Resolve again inside the game lock: all ordinary launches resume,
            # including callers that did not explicitly pass --state-file.
            automatic = self.state_directory / "auto.oesavestate"
            if not state_file and not restart_auto and automatic.is_file():
                state_file = automatic
            self.pending_state = Path(state_file).resolve() if state_file else None
            if self.pending_state and (self.pending_state.parent != self.state_directory.resolve()
                                       or not self.pending_state.is_file()):
                raise CoreError("This save state is missing or needs a different core build.")
            self.state_restore_failed = False
            library.prepare_save_filenames(self.game, save_directory)
            compatible_hardware = (self.game["system"] == "n64" and
                                   (selected.get("catalog_id") or selected["id"]) == "parallel_n64")
            saved_renderer = saved_state_renderer(state_file) if state_file else None
            hardware_enabled = library.setting("experimental.hardware_rendering", "1") == "1"
            catalog_id = selected.get("catalog_id") or selected["id"]
            core_record = CATALOG.get(catalog_id, {})
            hardware_required = bool(core_record.get('hardware_required'))
            adaptive = bool(core_record.get("hardware_options") or hardware_required)
            capabilities = probe_hardware_contexts() if adaptive or compatible_hardware else (0, 0, 0, 0)
            vulkan_available = bool(adaptive and (core_record.get("vulkan_options") or
                                                  core_record.get('vulkan_context'))
                                    and probe_vulkan_context())
            n64_gpu_available = bool(capabilities[0] or capabilities[2])
            hardware_candidate = compatible_hardware and n64_gpu_available and (
                saved_renderer == "opengl" if state_file else hardware_enabled
            )
            render_options, hardware_available, using_hardware = core_render_options(
                catalog_id, capabilities, vulkan_available=vulkan_available,
                resuming=bool(state_file), saved_renderer=saved_renderer)
            if compatible_hardware and state_file and saved_renderer == "opengl" and not n64_gpu_available:
                raise CoreError("This save state needs an OpenGL core context that is unavailable on this computer.")
            if adaptive and state_file and saved_renderer == "opengl" and not using_hardware:
                raise CoreError("This save state needs an OpenGL core context that is unavailable on this computer.")
            if adaptive and state_file and saved_renderer == "vulkan" and not vulkan_available:
                raise CoreError("This save state needs a Vulkan core context that is unavailable on this computer.")
            if hardware_required and not using_hardware:
                raise CoreError("This core requires Vulkan or OpenGL hardware rendering. "
                                "A compatible GPU context is unavailable for this game or save state.")
            if hardware_required and state_file and saved_renderer == 'opengl':
                vulkan_available = False
            self.hardware_state_compatibility = bool(
                (compatible_hardware and hardware_enabled and n64_gpu_available and state_file
                 and saved_renderer != "opengl")
                or (adaptive and hardware_available and state_file and not using_hardware)
            )
            core_options = dict(options)
            # A core may request a hardware context regardless of which API
            # presents frames to the window. The libretro callback negotiates
            # only contexts this machine can actually create.
            core_options["allow_hardware"] = True
            core_options["allow_vulkan"] = vulkan_available
            if hardware_candidate or render_options:
                accelerated = dict(options.get("options", {}))
                accelerated.update(render_options)
            if hardware_candidate:
                accelerated.update({
                    "parallel-n64-gfxplugin": "gliden64",
                    # GLideN64's automatic RSP leaves F-Zero X's road black;
                    # the core's cxd4 RSP renders the same saved scene correctly.
                    "parallel-n64-rspplugin": (
                        "cxd4" if self.game["title"].casefold().startswith("f-zero x") else "auto"
                    ),
                    # Keep adjacent 2D texture rectangles aligned in scenes
                    # such as Mischief Makers' save-selection screen.
                    "parallel-n64-gliden64-EnableNativeResTexrects": "Optimized",
                })
            if hardware_candidate or render_options:
                core_options["options"] = accelerated
            print("Core: " + selected["id"], flush=True)
            attempts = [(render_options, vulkan_available)]
            if hardware_required and vulkan_available and capabilities[2] and not state_file:
                attempts.append((render_options, False))
            if catalog_id == "mednafen_psx_hw" and not state_file:
                if render_options == CATALOG[catalog_id].get("vulkan_options"):
                    gl_options, _, gl_usable = core_render_options(catalog_id, capabilities)
                    if gl_usable:
                        attempts.append((gl_options, False))
                if render_options != CATALOG[catalog_id].get("software_options"):
                    attempts.append((CATALOG[catalog_id]["software_options"], False))
            for index, (candidate, use_vulkan) in enumerate(attempts):
                attempt_options = dict(core_options)
                attempt_options['allow_vulkan'] = use_vulkan
                if candidate != render_options:
                    attempt_options["options"] = {**options.get("options", {}), **candidate}
                try:
                    self.core = Core(rom_path, self.game["system"], save_directory, **attempt_options)
                    if self.core.hardware_requested:
                        kind = ("Vulkan" if self.core.hardware_context_type == 6 else
                                "OpenGL Core" if self.core.hardware_context_type == 3 else "OpenGL")
                        print("Core requested hardware context: " + kind, flush=True)
                        (VulkanCoreDisplay if self.core.hardware_context_type == 6 else
                         HardwareCoreDisplay)(self.core)
                    elif hardware_required or (candidate != core_record.get("software_options") and len(attempts) > 1):
                        raise CoreError("The core declined the requested hardware renderer.")
                    break
                except (CoreError, HardwareRenderError) as error:
                    if hasattr(self, "core"):
                        self.core.close()
                        del self.core
                    if index + 1 == len(attempts):
                        raise
                    print(f"Core renderer failed: {error}; trying the next available renderer.", flush=True)
            if self.hardware_state_compatibility:
                self.core.renderer = "Software (save-state compatibility)"
            # The old Snes9x adapter wrote raw SRAM. Import only into the matching
            # standard Snes9x core, with the native layer checking exact size.
            # Other core formats and all old save states remain untouched.
            if self.game['system'] == 'snes' and selected['id'] == 'snes9x' and not (save_directory / 'battery.srm').exists():
                legacy = library.root / 'saves' / game_id
                library.prepare_save_filenames(self.game, legacy)
                battery = legacy / (rom_path.stem + '.sav')
                if battery.is_file():
                    self.core.import_battery(battery)
            if restart_auto:
                (self.state_directory / "auto.oesavestate").unlink(missing_ok=True)
                (self.state_directory / "auto.png").unlink(missing_ok=True)
        except Exception:
            if hasattr(self, "core"):
                self.core.close()
            self.lock.unlock()
            raise
        self.control_layout = layout_for(self.game['system'])
        count = SPECS[self.game['system']]['players']
        self.control_profiles = [load_profile(library, self.game['system'], player) for player in range(count)]
        keyboard_layouts = [keyboard_layout(self.game['system'], profile) for profile in self.control_profiles]
        self.key_bindings_by_player = [keyboard_buttons(layout) for layout in keyboard_layouts]
        self.axis_bindings_by_player = [keyboard_axes(layout) for layout in keyboard_layouts]
        self.pressed_axes_by_player = [set() for _ in range(count)]
        self.pads = [Gamepad(self.control_layout, bindings=profile['gamepad'],
                             device=profile['device'], player=player)
                     for player, profile in enumerate(self.control_profiles)]
        # Player 1 aliases retain compatibility with the existing HUD/tests.
        self.key_bindings = self.key_bindings_by_player[0]
        self.axis_bindings = self.axis_bindings_by_player[0]
        self.pressed_axes = self.pressed_axes_by_player[0]
        self.pad = self.pads[0]
        self.rumble_enabled = library.setting("controller_rumble", "1") == "1"
        self.rumble_intensity = self.preference_int("controller_rumble_intensity", 100, 0, 100)
        self.rumble_unavailable_notified = False
        self.keys = 0
        self.extra_keys = [0] * (count - 1)
        self.paused = False
        self.focus_paused = False
        self.muted = muted or library.setting("muted", "0") == "1"
        self.volume = self.preference_int("volume", 80, 0, 100) / 100
        self.pause_unfocused = library.setting("pause_unfocused", "1") == "1"
        self.fast = False
        self.fast_speed = self.preference_int("fast_forward_speed", 3, 2, 10)
        self.frames = 0
        self.fps_sample_frames = 0
        self.fps_sample_started = time.perf_counter()
        self.frame_limit, self.capture_path = frame_limit, screenshot
        self.refresh_game_mode()
        self.audio = self.audio_device = None
        self.audio_latency_ms = self.preference_int("audio_latency", 80, 20, 250)
        self.audio_underruns = 0
        self.audio_pending = bytearray()
        chosen_filter = valid_filter(library.setting("video_filter." + self.game["system"],
                                                   library.setting("video_filter", "nearest")))
        self.screen = Screen(renderer_mode=library.setting("experimental.frontend_renderer", "auto"),
                             video_filter=chosen_filter)
        self.screen.filter_resolution = self.preference_int("filter_resolution." + self.game["system"], 0, 0, 2160)
        self.shader_dialog = None
        self.load_shader_parameters()
        self.screen.shader_failed.connect(self.shader_failed, Qt.QueuedConnection)
        self.filter_timer = QTimer(self)
        self.filter_timer.setSingleShot(True)
        self.filter_timer.timeout.connect(self.persist_shader_parameters)
        self.screen.integer_scale = library.setting("integer_scale", "0") == "1"
        self.use_system_aspect = library.setting("aspect_mode", "system") == "system"
        if self.use_system_aspect:
            self.screen.display_aspect = self.core.aspect
        self.setCentralWidget(self.screen)
        self.setWindowTitle(f"{self.game['title']} — {selected['name'] + ' (Libretro)' if selected else 'EmuLuna'}")
        self.resize(850, 630)
        self.window_size_key = f"window.game.{self.game['system']}.size"
        self.setStyleSheet("""
            QMainWindow,QToolBar,QStatusBar,QMenuBar,QMenu {background:palette(window);color:palette(window-text)}
            QToolBar {spacing:12px;padding:8px}
            QToolButton {color:palette(button-text);padding:6px;border:1px solid transparent;border-radius:4px}
            QToolButton:hover,QMenuBar::item:selected,QMenu::item:selected {background:palette(highlight);color:palette(highlighted-text)}
            QMenu {border:1px solid palette(mid);border-radius:8px;padding:6px}
            QMenu::item {padding:7px 38px 7px 12px}
            QMenu::right-arrow {right:10px}
            QComboBox {padding:5px;background:palette(button);color:palette(button-text);border:1px solid palette(mid)}
        """)
        self.setFocusPolicy(Qt.StrongFocus)
        bar = self.addToolBar("Game")
        bar.setMovable(False)
        self.game_toolbar = bar
        self.pause_action = self.action(bar, "Pause", "Ctrl+P", self.toggle_pause)
        self.save_action = self.action(bar, "Save state", "F5", self.save)
        self.load_action = self.action(bar, "Load state", "F8", self.load)
        self.slot = QComboBox()
        self.slot.addItems([f"Slot {i}" for i in range(1, 10)])
        self.slot.setFocusPolicy(Qt.NoFocus)
        bar.addWidget(self.slot)
        menu = QMenu("Gameplay options", self)
        self.menuBar().hide()
        menu.addActions([self.pause_action, self.save_action, self.load_action])
        slots = menu.addMenu("Save slot")
        slot_group = QActionGroup(self)
        self.slot_actions = []
        for index in range(9):
            action = slots.addAction(f"Slot {index + 1}")
            action.setCheckable(True)
            action.setChecked(index == 0)
            slot_group.addAction(action)
            action.triggered.connect(lambda checked=False, index=index: self.slot.setCurrentIndex(index))
            self.slot_actions.append(action)
        self.slot.currentIndexChanged.connect(lambda index: self.slot_actions[index].setChecked(True))
        self.reset_action = self.action(menu, "Reset", "Ctrl+R", self.reset)
        self.action(None, "Restore automatic save", "Ctrl+F8", self.load_auto)
        self.action(menu, "Screenshot", "F12", self.screenshot)
        self.fullscreen_action = self.action(None, "Full screen", "F11", self.fullscreen)
        self.mute_action = self.action(None, "Mute", "Ctrl+M", self.toggle_mute)
        self.mute_action.setCheckable(True)
        self.mute_action.setChecked(self.muted)
        self.action(None, "Volume up", "Ctrl+Up", lambda: self.set_volume(round(self.volume * 100) + 5))
        self.action(None, "Volume down", "Ctrl+Down", lambda: self.set_volume(round(self.volume * 100) - 5))
        self.add_accessory_menus(menu)
        self.add_runtime_option_menus(menu)
        filters = menu.addMenu("Video Filter")
        self.filters_menu = filters
        filters.setToolTipsVisible(True)
        self.shader_adjust_action = filters.addAction("Configure Shader…", self.adjust_shader)
        filters.addSeparator()
        filter_group = QActionGroup(self)
        self.filter_actions = {}
        for key, name in ALL_FILTERS.items():
            if key == next(k for k, spec in PRESETS.items() if spec.get('additional')):
                filters.addSeparator()
            action = filters.addAction(name)
            action.setCheckable(True)
            action.setChecked(key == self.screen.video_filter)
            action.setToolTip(f"{PRESETS[key]['passes']} pass(es) · {PRESETS[key]['cost']} GPU demand")
            action.triggered.connect(lambda checked=False, key=key: self.set_video_filter(key))
            filter_group.addAction(action)
            self.filter_actions[key] = action
        scaling = menu.addMenu("Scaling")
        scaling_group = QActionGroup(self)
        scaling_group.setExclusive(True)
        self.fit_action = self.action(scaling, "Fit to window", "", lambda _=False: self.integer_scale(False))
        self.integer_action = self.action(scaling, "Integer scaling", "Ctrl+I", self.integer_scale)
        for action in (self.fit_action, self.integer_action):
            action.setCheckable(True)
            scaling_group.addAction(action)
        self.integer_action.setChecked(self.screen.integer_scale)
        self.fit_action.setChecked(not self.screen.integer_scale)
        scaling.addSeparator()
        self.resize_integer_action = self.action(scaling, "Resize window to integer scale", "", self.resize_to_integer_scale)
        self.action(menu, "Controls", "F1", self.controls)
        for popup in (menu, *menu.findChildren(QMenu)):
            popup.setAttribute(Qt.WA_TranslucentBackground)
        self.stop_action = self.action(None, "Power off", "Ctrl+W", self.close)
        self.hud = GameplayHUD(self.screen, actions=[
            ("power", self.stop_action, "power"),
            ("pause", self.pause_action, QStyle.SP_MediaPause),
            ("reset", self.reset_action, QStyle.SP_BrowserReload),
            ("save", self.save_action, QStyle.SP_DialogSaveButton),
            ("load", self.load_action, QStyle.SP_DialogOpenButton),
            ("mute", self.mute_action, QStyle.SP_MediaVolume),
            ("fullscreen", self.fullscreen_action, "fullscreen")],
            options=menu, volume=round(self.volume * 100), set_volume=self.set_volume,
            paused=lambda: self.paused or self.focus_paused,
            hide_cursor=library.setting("hide_game_cursor", "1") == "1",
            order=("power", "pause", "reset", "save", "load", "options", "mute", "volume", "fullscreen"))
        set_hud_icon(self.mute_action, self.hud, QStyle.SP_MediaVolumeMuted if self.muted else QStyle.SP_MediaVolume)
        bar.hide()
        self.notice = GameplayNotice(self.screen)
        self.diagnostics = GameplayDiagnostics(
            self.screen,
            show_fps=library.setting("experimental.show_fps", "0") == "1",
            show_renderer=library.setting("experimental.show_renderer_debug", "0") == "1",
        )
        self.diagnostics.set_core_renderer(getattr(self.core, 'renderer', 'Software'))
        if self.hardware_state_compatibility:
            self.diagnostics.setToolTip(
                "This state was created by the software renderer. Restart the game to begin an OpenGL session."
            )
            self.notice.show_message("Resumed with the save state's software renderer")
        self.statusBar().showMessage(self.control_layout["hint"])
        self.setup_audio()
        self.audio_devices = QMediaDevices(self)
        self.audio_devices.audioOutputsChanged.connect(self.audio_output_changed)
        self.library.played(game_id)
        self.next_frame = time.perf_counter()
        self.timer = QTimer(self)
        self.timer.setTimerType(Qt.PreciseTimer)
        self.timer.timeout.connect(self.tick)
        self.timer.start(1)
        self.battery_timer = QTimer(self)
        self.battery_timer.timeout.connect(self.flush_battery)
        self.battery_timer.start(30000)
        self.control_settings = None
        self.controls_timer = QTimer(self)
        self.controls_timer.timeout.connect(self.refresh_controls)
        self.controls_timer.start(1000)
        QApplication.instance().applicationStateChanged.connect(self.application_focus_changed)
        restore_window_size(self, library, self.window_size_key)
        self.windowed_state = self.windowState()
        if library.setting("fullscreen_default", "0") == "1" and not frame_limit:
            QTimer.singleShot(0, self.initial_fullscreen)

    def preference_int(self, key, default, minimum, maximum):
        try:
            return max(minimum, min(maximum, int(self.library.setting(key, str(default)))))
        except ValueError:
            return default

    def add_accessory_menus(self, menu):
        """Build core-declared accessory choices without console logic in Qt."""
        records = getattr(self, "accessory_options", ())
        if not records:
            return
        root = menu.addMenu("Controller Accessories")
        groups = {}
        for record in records:
            if record["group"] not in groups:
                groups[record["group"]] = root.addMenu(record["group"])
            group_menu = groups[record["group"]]
            target = group_menu.addMenu(f"Player {record['player']}") if record["count"] > 1 else group_menu
            actions = QActionGroup(self)
            actions.setExclusive(True)
            for value, label in record["choices"]:
                action = target.addAction(label)
                action.setCheckable(True)
                action.setChecked(value == record["value"])
                actions.addAction(action)
                action.triggered.connect(lambda checked=False, record=record, value=value, label=label:
                                         self.set_accessory(record, value, label))

    def set_accessory(self, record, value, label):
        record["value"] = value
        self.library.set_setting(record["setting"], value)
        if record["restart"]:
            self.notice.show_message(f"{label} will be used next time the game opens")
        else:
            self.core.set_option(record["key"], value)
            self.notice.show_message(f"{record['group']} · {label}")

    def add_runtime_option_menus(self, menu):
        """Expose safe core-declared options that can change during gameplay."""
        groups = {}
        for record in getattr(self, "runtime_options", ()):
            if record["group"] not in groups:
                groups[record["group"]] = menu.addMenu(record["group"])
            group = groups[record["group"]]
            action = group.addAction(record["label"])
            action.setCheckable(True)
            action.setChecked(record["value"] == record["enabled"])
            action.triggered.connect(lambda checked=False, record=record:
                                     self.set_runtime_option(record, checked))
            record["action"] = action

    def set_runtime_option(self, record, checked):
        value = record["enabled"] if checked else record["disabled"]
        record["value"] = value
        self.library.set_setting(record["setting"], value)
        self.core.set_option(record["key"], value)
        state = "On" if checked else "Off"
        self.notice.show_message(f"{record['label']} · {state}")

    def refresh_controls(self):
        """Pick up settings from the library process without restarting a core."""
        if self.closed:
            return
        self.refresh_focus_pause()
        self.refresh_game_mode()
        system = self.game['system']
        self.rumble_enabled = self.library.setting("controller_rumble", "1") == "1"
        self.rumble_intensity = self.preference_int("controller_rumble_intensity", 100, 0, 100)
        saved = tuple(self.library.setting(f'controls.{system}.{player}', '{}')
                      for player in range(len(self.pads)))
        if saved == self.control_settings:
            return
        self.control_settings = saved
        profiles = [load_profile(self.library, system, player) for player in range(len(self.pads))]
        if profiles == self.control_profiles:
            return
        for player, (old, new) in enumerate(zip(self.control_profiles, profiles)):
            if old == new:
                continue
            layout = keyboard_layout(system, new)
            self.key_bindings_by_player[player] = keyboard_buttons(layout)
            self.axis_bindings_by_player[player] = keyboard_axes(layout)
            self.pressed_axes_by_player[player].clear()
            if player == 0:
                self.keys = 0
            else:
                self.extra_keys[player - 1] = 0
            if old['device'] != new['device']:
                self.pads[player].close()
                self.pads[player] = Gamepad(self.control_layout, bindings=new['gamepad'],
                                            device=new['device'], player=player)
            else:
                self.pads[player].bindings = new['gamepad']
        self.control_profiles = profiles
        self.key_bindings = self.key_bindings_by_player[0]
        self.axis_bindings = self.axis_bindings_by_player[0]
        self.pad = self.pads[0]

    def refresh_game_mode(self):
        enabled = (not self.frame_limit
                   and self.library.setting("game_mode.keep_awake", "1") == "1")
        if enabled == self.game_mode_enabled:
            return
        self.game_mode_enabled = enabled
        if enabled:
            available = self.game_mode.start()
            print("Game mode: display sleep " + ("inhibited" if available else "inhibition unavailable"),
                  flush=True)
        else:
            self.game_mode.stop()

    def initial_fullscreen(self):
        if not self.closed and self.isVisible() and not self.isFullScreen():
            self.fullscreen()

    def action(self, parent, title, key, fn):
        action = QAction(title, self)
        action.setShortcut(QKeySequence(key))
        action.triggered.connect(fn)
        if parent is not None:
            parent.addAction(action)
        # Keep shortcuts active when the fullscreen menu and toolbar are hidden.
        self.addAction(action)
        return action

    def setup_audio(self):
        device = QMediaDevices.defaultAudioOutput()
        wanted = self.library.setting("audio_device", "")
        if wanted:
            device = next((item for item in QMediaDevices.audioOutputs() if bytes(item.id()).hex() == wanted), device)
        fmt = QAudioFormat()
        fmt.setSampleRate(self.core.sample_rate)
        fmt.setChannelCount(2)
        fmt.setSampleFormat(QAudioFormat.Int16)
        if device.isNull() or not device.isFormatSupported(fmt):
            self.statusBar().showMessage("Audio output unavailable. The game can still run.")
            return
        self.audio = QAudioSink(device, fmt, self)
        self.audio_latency_ms = self.preference_int("audio_latency", 80, 20, 250)
        self.audio.setBufferSize(self.core.sample_rate * 4 * self.audio_latency_ms // 1000)
        self.audio.stateChanged.connect(self.audio_state_changed)
        self.audio_device = self.audio.start()
        self.audio.setVolume(0 if self.muted else self.volume)
        if self.audio_device is None:
            self.statusBar().showMessage("Audio output could not start. Choose another output in Settings.")
        else:
            self.prime_audio()
        self.sync_audio_pause()

    def prime_audio(self, milliseconds=None):
        """Keep a small cushion so a fullscreen repaint cannot starve audio."""
        if not self.audio_device:
            return 0
        duration = milliseconds if milliseconds is not None else max(20, min(50, self.audio_latency_ms // 2))
        count = self.core.sample_rate * 4 * duration // 1000 & ~3
        return self.audio_device.write(bytes(count)) if count else 0

    def audio_state_changed(self, state):
        """Recover cleanly if an expensive frame exhausts the device buffer."""
        if (state == QAudio.State.IdleState and self.audio
                and self.audio.error() == QAudio.Error.UnderrunError
                and not (self.paused or self.focus_paused or self.fast)):
            self.audio_underruns += 1
            self.prime_audio(max(20, min(50, self.audio_latency_ms // 2)))

    def sync_audio_pause(self):
        if self.audio:
            if self.paused or self.focus_paused or self.fast:
                self.audio.suspend()
            else:
                self.audio.resume()

    def audio_output_changed(self):
        if self.closed:
            return
        if self.audio:
            self.audio.stop()
            self.audio.deleteLater()
        self.audio = self.audio_device = None
        self.audio_pending.clear()
        self.setup_audio()

    def tick(self):
        if self.paused or self.focus_paused:
            self.next_frame = time.perf_counter()
            self.fps_sample_started = self.next_frame
            self.fps_sample_frames = 0
            return
        now = time.perf_counter()
        if now < self.next_frame:
            return
        period = 1 / (self.core.fps * (self.fast_speed if self.fast else 1))
        if now - self.next_frame > 0.15:
            self.next_frame = now
        self.next_frame += period
        try:
            if self.pending_state and self.frames >= 2:
                path, self.pending_state = self.pending_state, None
                try:
                    self.core.load_state(path)
                    self.flush_audio()
                    self.notice.show_message("State loaded")
                except (OSError, CoreError) as error:
                    self.state_restore_failed = True
                    if not self.paused:
                        self.toggle_pause()
                    self.report(error)
                    return
            previous_rate = self.core.sample_rate
            pad_buttons = [pad.poll() for pad in self.pads]
            axes = [combined_axes(bindings, pressed, pad.axes) for bindings, pressed, pad in
                    zip(self.axis_bindings_by_player, self.pressed_axes_by_player, self.pads)]
            players = [(self.extra_keys[index - 1] | pad_buttons[index], axes[index])
                       for index in range(1, len(self.pads))]
            pixels, audio = self.core.frame(self.keys | pad_buttons[0], axes[0], players)
            scale = self.rumble_intensity / 100 if self.rumble_enabled else 0
            for port, pad in enumerate(self.pads):
                strong, weak = self.core.rumble(port)
                accepted = pad.set_rumble(strong * scale, weak * scale)
                if scale and (strong or weak) and pad.pad and not accepted and not self.rumble_unavailable_notified:
                    self.rumble_unavailable_notified = True
                    self.notice.show_message("Controller vibration is unavailable")
            if self.core.sample_rate != previous_rate:
                if self.audio:
                    self.audio.stop()
                    self.audio.deleteLater()
                self.audio = self.audio_device = None
                self.audio_pending.clear()
                self.setup_audio()
            self.screen.frame = (pixels if isinstance(pixels, QImage) else
                QImage(pixels, self.core.width, self.core.height,
                       self.core.width * 4, QImage.Format_RGB32).copy())
            if self.use_system_aspect:
                self.screen.display_aspect = self.core.aspect
            self.screen.update()
            if self.audio_device and not self.fast:
                self.audio_pending.extend(audio)
                # Bound latency after underruns, device changes or a slow desktop.
                max_bytes = self.core.sample_rate * 4 // 8
                if len(self.audio_pending) > max_bytes:
                    del self.audio_pending[:-max_bytes]
                count = min(len(self.audio_pending), self.audio.bytesFree()) & ~3
                if count:
                    written = self.audio_device.write(bytes(self.audio_pending[:count]))
                    if written > 0:
                        del self.audio_pending[:written]
            self.frames += 1
            self.fps_sample_frames += 1
            sample_now = time.perf_counter()
            elapsed = sample_now - self.fps_sample_started
            if elapsed >= 0.5:
                self.diagnostics.set_fps(self.fps_sample_frames / elapsed)
                self.fps_sample_frames = 0
                self.fps_sample_started = sample_now
            if self.frame_limit and self.frames >= self.frame_limit:
                if self.capture_path:
                    self.grab().save(str(self.capture_path))
                self.close()
        except Exception as e:
            self.timer.stop()
            QMessageBox.critical(self, "Emulation stopped", str(e))
            self.close()

    def state_path(self, name=None):
        return self.state_directory / (name or f"slot-{self.slot.currentIndex()+1}.oesavestate")

    def save_state_with_preview(self, path):
        self.core.save_state(path)
        # A preview failure must never discard a successfully saved game.
        preview = path.with_suffix(".png")
        output = QSaveFile(str(preview))
        if (not self.screen.frame.isNull() and output.open(QIODevice.WriteOnly)
                and self.screen.frame.save(output, "PNG") and output.commit()):
            return
        output.cancelWriting()
        try:
            preview.unlink(missing_ok=True)
        except OSError:
            pass
        if not self.screen.frame.isNull():
            self.statusBar().showMessage("State saved, but its screenshot could not be written.", 5000)

    def save_auto(self):
        try:
            self.core.flush()
            if not self.state_restore_failed and not self.pending_state:
                self.save_state_with_preview(self.state_path("auto.oesavestate"))
        except (OSError, CoreError) as e:
            self.statusBar().showMessage(f"Automatic save failed: {e}")

    def flush_battery(self):
        try:
            self.core.flush()
        except (OSError, CoreError) as error:
            self.statusBar().showMessage(f"In-game save could not be written: {error}")

    def save(self):
        try:
            self.save_state_with_preview(self.state_path())
            self.statusBar().showMessage(f"Saved to slot {self.slot.currentIndex()+1}", 5000)
            self.notice.show_message(f"State saved · Slot {self.slot.currentIndex()+1}")
        except (OSError, CoreError) as e:
            self.notice.show_message("State could not be saved")
            self.report(e)

    def load(self):
        try:
            self.core.load_state(self.state_path())
            self.screen.frame = QImage(str(self.state_path().with_suffix(".png")))
            self.state_restore_failed = False
            self.flush_audio()
            self.statusBar().showMessage(f"Restored slot {self.slot.currentIndex()+1}", 5000)
            self.notice.show_message(f"State loaded · Slot {self.slot.currentIndex()+1}")
        except (OSError, CoreError) as e:
            self.notice.show_message("State could not be loaded")
            self.report(e)

    def report(self, error):
        was_paused = self.paused
        self.paused = True
        self.sync_audio_pause()
        QMessageBox.warning(self, "EmuLuna", str(error))
        self.paused = was_paused
        self.sync_audio_pause()
        self.next_frame = time.perf_counter()

    def load_auto(self):
        try:
            self.core.load_state(self.state_path("auto.oesavestate"))
            self.screen.frame = QImage(str(self.state_path("auto.oesavestate").with_suffix(".png")))
            self.state_restore_failed = False
            self.flush_audio()
            self.statusBar().showMessage("Restored the last automatic save", 5000)
            self.notice.show_message("Automatic state loaded")
        except (OSError, CoreError) as e:
            self.notice.show_message("Automatic state could not be loaded")
            self.report(e)

    def reset(self):
        self.core.reset()
        self.flush_audio()

    def screenshot(self):
        path = self.library.root / "screenshots" / f"{self.game_id[:12]}-{datetime.now():%Y%m%d-%H%M%S}.png"
        if self.screen.frame.save(str(path)):
            self.statusBar().showMessage(f"Screenshot saved: {path}", 7000)

    def toggle_pause(self):
        self.paused = not self.paused
        self.pause_action.setText("Resume" if self.paused else "Pause")
        set_hud_icon(self.pause_action, self.hud, QStyle.SP_MediaPlay if self.paused else QStyle.SP_MediaPause)
        self.clear_input()
        self.sync_audio_pause()
        self.hud.reveal()
        self.next_frame = time.perf_counter()

    def toggle_mute(self):
        self.muted = not self.muted
        self.library.set_setting("muted", int(self.muted))
        self.mute_action.setChecked(self.muted)
        set_hud_icon(self.mute_action, self.hud, QStyle.SP_MediaVolumeMuted if self.muted else QStyle.SP_MediaVolume)
        if self.audio:
            self.audio.setVolume(0 if self.muted else self.volume)

    def set_volume(self, value):
        value = max(0, min(100, int(value)))
        self.volume = value / 100
        self.library.set_setting("volume", value)
        if self.audio:
            self.audio.setVolume(0 if self.muted else self.volume)
        self.hud.volume.blockSignals(True)
        self.hud.volume.setValue(value)
        self.hud.volume.blockSignals(False)

    def set_fast(self, enabled):
        if self.fast == enabled:
            return
        self.fast = enabled
        self.flush_audio()
        self.next_frame = time.perf_counter()

    def flush_audio(self):
        self.audio_pending.clear()
        if self.audio:
            self.audio.reset()
            self.audio_device = self.audio.start()
            self.audio.setVolume(0 if self.muted else self.volume)
            self.prime_audio()
        self.sync_audio_pause()

    def set_video_filter(self, key):
        if self.filter_timer.isActive():
            self.persist_shader_parameters()
            self.filter_timer.stop()
        if self.shader_dialog:
            self.shader_dialog.close()
        self.screen.video_filter = valid_filter(key)
        self.load_shader_parameters()
        self.library.set_setting("video_filter." + self.game['system'], self.screen.video_filter)
        self.filter_actions[self.screen.video_filter].setChecked(True)
        self.screen.update()
        self.notice.show_message(ALL_FILTERS[self.screen.video_filter])

    def load_shader_parameters(self):
        key = self.screen.video_filter
        if key in PRESETS:
            try:
                saved = json.loads(self.library.setting("shader_parameters." + self.game['system'] + "." + key, "{}"))
            except (ValueError, TypeError):
                saved = {}
            self.screen.shader_parameters = parameter_values(key, saved)

    def adjust_shader(self):
        key = self.screen.video_filter
        if key not in PRESETS:
            return
        if self.shader_dialog:
            self.shader_dialog.close()
        self.shader_dialog = ShaderControls(key, self.screen.shader_parameters, self.set_shader_parameter, self,
            select=self.select_shader_in_dialog, backend=self.screen.backend, resolution=self.screen.filter_resolution,
            resolution_changed=self.set_filter_resolution)
        self.screen.backend_changed.connect(self.shader_dialog.set_backend)
        dialog = self.shader_dialog
        dialog.finished.connect(lambda: self.shader_dialog_closed(dialog))
        dialog.show()

    def shader_dialog_closed(self, dialog):
        if self.shader_dialog is dialog:
            self.shader_dialog = None

    def set_shader_parameter(self, name, value):
        self.screen.shader_parameters[name] = value
        self.screen.update()
        self.filter_timer.start(250)

    def select_shader_in_dialog(self, key):
        self.persist_shader_parameters()
        self.filter_timer.stop()
        self.screen.video_filter = valid_filter(key)
        self.load_shader_parameters()
        self.library.set_setting("video_filter." + self.game['system'], self.screen.video_filter)
        self.filter_actions[self.screen.video_filter].setChecked(True)
        self.screen.update()
        return self.screen.shader_parameters

    def set_filter_resolution(self, value):
        self.screen.filter_resolution = value
        self.library.set_setting("filter_resolution." + self.game['system'], value)
        self.screen.update()

    def shader_failed(self, message):
        if self.closed:
            return
        # Fail safely for this session, without replacing the saved preference
        # or writing the failed shader's parameter values under another name.
        self.filter_timer.stop()
        self.filter_actions['nearest'].setChecked(True)
        self.screen.video_filter = 'nearest'
        self.screen.shader_parameters = {}
        if self.shader_dialog:
            self.shader_dialog.close()
            self.shader_dialog = None
        self.notice.show_message("Filter unavailable · using Nearest Neighbor")
        self.statusBar().showMessage(message, 15000)
        print("Shader unavailable: " + message, file=sys.stderr)

    def persist_shader_parameters(self):
        if self.screen.video_filter in PRESETS:
            self.library.set_setting("shader_parameters." + self.game['system'] + "." + self.screen.video_filter,
                                     json.dumps(self.screen.shader_parameters))

    def integer_scale(self, value):
        self.screen.integer_scale = bool(value)
        self.integer_action.setChecked(self.screen.integer_scale)
        self.fit_action.setChecked(not self.screen.integer_scale)
        self.library.set_setting("integer_scale", int(self.screen.integer_scale))
        self.screen.update()
        if hasattr(self, 'notice'):
            self.notice.show_message("Integer scaling" if self.screen.integer_scale else "Fit to window")

    def resize_to_integer_scale(self):
        self.integer_scale(True)
        if self.isFullScreen():
            self.notice.show_message("Fullscreen size is fixed · integer borders may remain")
            return
        content = self.screen.integer_size()
        chrome = self.size() - self.screen.size()
        self.resize(content.width() + chrome.width(), content.height() + chrome.height())
        self.notice.show_message(f"Integer window · {content.width()} × {content.height()}")

    def fullscreen(self):
        leaving = self.isFullScreen()
        if leaving:
            size = self._emuluna_windowed_size
            self.showNormal()
            self.resize(size)
            if self.windowed_state & Qt.WindowMaximized:
                self.showMaximized()
        else:
            self.windowed_state = self.windowState()
            self.showFullScreen()
        self.menuBar().hide()
        self.statusBar().setVisible(leaving)
        self.fullscreen_action.setText("Full screen" if leaving else "Exit full screen")
        set_hud_icon(self.fullscreen_action, self.hud, "fullscreen" if leaving else "fullscreen-exit")
        self.hud.reveal()

    def controls(self):
        self.report(self.control_layout['description'] + "\n\nChange console and player mappings in Settings → Controls.\n\nPause: Ctrl+P\nSave: F5    Load: F8\nRestore automatic save: Ctrl+F8\nFull screen: F11    Screenshot: F12\nFast forward: hold Tab\nGames pause when their window loses focus.")

    def clear_input(self):
        self.keys = 0
        self.extra_keys = [0] * len(self.extra_keys)
        for pressed in self.pressed_axes_by_player:
            pressed.clear()
        self.set_fast(False)
        self.audio_pending.clear()
        for pad in self.pads:
            pad.stop_rumble()

    def refresh_focus_pause(self):
        if self.closed or not hasattr(self, 'controls_timer'):
            return
        self.pause_unfocused = self.library.setting("pause_unfocused", "1") == "1"
        active = (self.isActiveWindow() and not self.isMinimized()
                  and QApplication.instance().applicationState() == Qt.ApplicationActive)
        paused = self.pause_unfocused and not active and not self.frame_limit
        if paused != self.focus_paused:
            self.focus_paused = paused
            self.clear_input()
            self.sync_audio_pause()
            self.next_frame = time.perf_counter()

    def application_focus_changed(self, state):
        # Qt updates the native window's activation after application state.
        QTimer.singleShot(0, self.refresh_focus_pause)

    def changeEvent(self, event):
        from PySide6.QtCore import QEvent
        if (event.type() in (QEvent.ActivationChange, QEvent.WindowStateChange)
                and hasattr(self, 'controls_timer') and not self.closed):
            self.clear_input()
            self.refresh_focus_pause()
            QTimer.singleShot(0, self.refresh_focus_pause)
        super().changeEvent(event)

    def keyPressEvent(self, event):
        if event.isAutoRepeat(): return
        handled = False
        for player, (buttons, axes) in enumerate(zip(self.key_bindings_by_player, self.axis_bindings_by_player)):
            if event.key() in buttons:
                if player == 0:
                    self.keys |= buttons[event.key()]
                else:
                    self.extra_keys[player - 1] |= buttons[event.key()]
                handled = True
            if event.key() in axes:
                self.pressed_axes_by_player[player].add(event.key())
                handled = True
        if handled:
            event.accept()
        elif event.key() == Qt.Key_Tab:
            self.set_fast(True)
        elif event.key() == Qt.Key_Escape and self.isFullScreen():
            self.fullscreen()
        else:
            super().keyPressEvent(event)

    def keyReleaseEvent(self, event):
        if event.isAutoRepeat(): return
        handled = False
        for player, (buttons, axes) in enumerate(zip(self.key_bindings_by_player, self.axis_bindings_by_player)):
            if event.key() in buttons:
                if player == 0:
                    self.keys &= ~buttons[event.key()]
                else:
                    self.extra_keys[player - 1] &= ~buttons[event.key()]
                handled = True
            if event.key() in axes:
                self.pressed_axes_by_player[player].discard(event.key())
                handled = True
        if event.key() == Qt.Key_Tab:
            self.set_fast(False)
        elif not handled:
            super().keyReleaseEvent(event)

    def event(self, event):
        from PySide6.QtCore import QEvent
        if event.type() in (QEvent.KeyPress, QEvent.KeyRelease) and event.key() == Qt.Key_Tab:
            (self.keyPressEvent if event.type() == QEvent.KeyPress else self.keyReleaseEvent)(event)
            return True
        return super().event(event)

    def closeEvent(self, event):
        if self.closed:
            event.accept()
            return
        self.closed = True
        save_window_size(self, self.library, self.window_size_key,
                         state=self.windowed_state if self.isFullScreen() else None)
        self.timer.stop()
        self.battery_timer.stop()
        self.controls_timer.stop()
        QApplication.instance().applicationStateChanged.disconnect(self.application_focus_changed)
        if self.filter_timer.isActive():
            self.persist_shader_parameters()
        self.filter_timer.stop()
        if self.shader_dialog:
            self.shader_dialog.close()
        self.hud.stop()
        self.notice.timer.stop()
        self.game_mode.stop()
        self.save_auto()
        self.core.close()
        self.screen.close_renderer()
        for pad in self.pads:
            pad.close()
        if self.audio:
            self.audio.stop()
        self.lock.unlock()
        event.accept()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir", required=True)
    parser.add_argument("--game", required=True)
    parser.add_argument("--mute", action="store_true")
    parser.add_argument("--frames", type=int, default=0)
    parser.add_argument("--screenshot")
    start_mode = parser.add_mutually_exclusive_group()
    start_mode.add_argument("--state-file")
    start_mode.add_argument("--restart-auto", action="store_true")
    parser.add_argument("--core-id")
    parser.add_argument("--core-sha256")
    args = parser.parse_args()
    prefer_native_desktop()
    app = QApplication(sys.argv[:1])
    print("Desktop platform: " + app.platformName(), flush=True)
    configure_application(app)
    library = Library(args.data_dir)
    try:
        player = Player(library, args.game, args.mute, args.frames, args.screenshot,
                        args.state_file, args.restart_auto, args.core_id, args.core_sha256)
    except Exception as e:
        print(str(e), file=sys.stderr)
        QMessageBox.critical(None, "Could not start game", str(e))
        return 1
    def present_player():
        """Ask the desktop to expose and focus a newly launched game window."""
        if player.closed:
            return
        player.show()
        player.raise_()
        player.activateWindow()
        handle = player.windowHandle()
        if handle:
            handle.requestActivate()

    player.show()
    present_player()
    print("EMULUNA_GAME_READY", flush=True)
    # Wayland compositors may reject the first activation request from a child
    # process. Retry after the surface is mapped, then let the library expose
    # this window if the compositor still keeps it behind the parent.
    def verify_player_focus():
        present_player()
        if (QApplication.platformName().lower().startswith("wayland")
                and player.isVisible() and not player.isActiveWindow()):
            print("EMULUNA_GAME_NEEDS_FOCUS", flush=True)
    QTimer.singleShot(250, verify_player_focus)
    result = app.exec()
    library.close()
    return result


if __name__ == "__main__":
    sys.exit(main())
