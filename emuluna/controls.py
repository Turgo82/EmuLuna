"""System control layouts expressed as frontend buttons and two analog sticks."""
import json
from PySide6.QtCore import Qt
from .systems import DATA, SYSTEMS

LAYOUTS = json.loads((DATA / 'controls.json').read_text())


def layout_for(system):
    return LAYOUTS[SYSTEMS[system].input_profile]


def keyboard_buttons(layout):
    return {int(name) if name.isdigit() else getattr(Qt, name): value for name, value in layout['keys'].items()}


def keyboard_axes(layout):
    return {int(name) if name.isdigit() else getattr(Qt, name): tuple(value) for name, value in layout.get('axes', {}).items()}


def combined_axes(bindings, pressed, gamepad_axes):
    values = list(gamepad_axes)
    for axis in range(4):
        inputs = [value for key, (index, value) in bindings.items() if index == axis and key in pressed]
        if inputs:
            values[axis] = max(-32768, min(32767, sum(inputs)))
    return tuple(values)
