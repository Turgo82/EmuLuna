"""Persistent per-console/player bindings; independent of devices and widgets."""
import json
from copy import deepcopy
from PySide6.QtCore import Qt
from PySide6.QtGui import QKeySequence
from .controls import layout_for
from .systems import DATA

SPECS = json.loads((DATA / 'controller_profiles.json').read_text())
BUTTONS = {64:'Up', 128:'Down', 32:'Left', 16:'Right', 8:'Start', 4:'Select',
           1:'A', 2:'B', 1024:'X', 2048:'Y', 512:'L', 256:'R',
           4096:'L2', 8192:'R2', 16384:'L3', 32768:'R3'}
PAD_BUTTONS = ['South (A / Cross)', 'East (B / Circle)', 'West (X / Square)',
    'North (Y / Triangle)', 'Back / Select', 'Guide', 'Start', 'Left stick click',
    'Right stick click', 'Left shoulder', 'Right shoulder', 'D-pad up', 'D-pad down',
    'D-pad left', 'D-pad right', 'Misc', 'Paddle 1', 'Paddle 2', 'Paddle 3', 'Paddle 4', 'Touchpad']
AXES = ['Left stick horizontal', 'Left stick vertical', 'Right stick horizontal',
        'Right stick vertical', 'Left trigger', 'Right trigger']


def axis_label(index, sign):
    directions = {
        0: ('Left stick ←', 'Left stick →'),
        1: ('Left stick ↑', 'Left stick ↓'),
        2: ('Right stick ←', 'Right stick →'),
        3: ('Right stick ↑', 'Right stick ↓'),
        4: ('Left trigger reverse', 'Left trigger'),
        5: ('Right trigger reverse', 'Right trigger'),
    }
    return directions[index][sign > 0]


def actions(system):
    spec = SPECS[system]
    result = [(f'button:{bit}', spec.get('labels', {}).get(str(bit), BUTTONS[bit]))
              for bit in spec['primary']]
    if spec.get('analog'):
        sticks = spec.get('sticks', ['Analog stick'])
        axis_count = len(sticks) * 2
        for axis in range(axis_count):
            for sign, direction in ((-1, 'left' if axis % 2 == 0 else 'up'),
                                    (1, 'right' if axis % 2 == 0 else 'down')):
                stick = sticks[axis // 2]
                result.append((f'axis:{axis}:{sign}', f'{stick} {direction}'))
    result.extend(spec.get('extra_actions', {}).items())
    return result


def defaults(system, player):
    layout = layout_for(system)
    spec = SPECS[system]
    record = {'device':'auto', 'keyboard':{}, 'gamepad':{}}
    def add(source, action, token):
        record[source].setdefault(action, []).append(token)
    if player == 0:
        for name, bit in layout['keys'].items():
            add('keyboard', f'button:{bit}', f'key:{int(getattr(Qt, name))}')
        for name, (axis, value) in layout.get('axes', {}).items():
            add('keyboard', f'axis:{axis}:{1 if value > 0 else -1}', f'key:{int(getattr(Qt, name))}')
    for button, bit in layout['gamepad'].items():
        add('gamepad', f'button:{bit}', f'button:{button}')
    for axis, bit in layout.get('triggers', {}).items():
        add('gamepad', f'button:{bit}', f'axis:{axis}:1')
    for axis in range(4):
        for sign in (-1, 1):
            add('gamepad', f'axis:{axis}:{sign}', f'axis:{axis}:{sign}')
    if layout.get('stick_as_dpad') and not SPECS[system].get('analog'):
        for bit, axis, sign in ((32,0,-1),(16,0,1),(64,1,-1),(128,1,1)):
            add('gamepad', f'button:{bit}', f'axis:{axis}:{sign}')
    # Map the remaining standard controller buttons/triggers without changing
    # any existing console-specific defaults (notably N64's Z trigger).
    for bit, token in ((4096,'axis:4:1'),(8192,'axis:5:1'),(16384,'button:7'),(32768,'button:8')):
        if bit not in spec.get('unbound_gamepad', ()):
            record['gamepad'].setdefault(f'button:{bit}', [token])
    allowed = {action for action, _ in actions(system)}
    for source in ('keyboard', 'gamepad'):
        record[source] = {action: tokens for action, tokens in record[source].items()
                          if action in allowed}
    return record


def valid_token(token, source):
    try:
        parts = token.split(':')
        if source == 'keyboard':
            return len(parts) == 2 and parts[0] == 'key' and 0 < int(parts[1]) < 0x02000000
        if parts[0] == 'button': return len(parts) == 2 and 0 <= int(parts[1]) < len(PAD_BUTTONS)
        return len(parts) == 3 and parts[0] == 'axis' and 0 <= int(parts[1]) < 6 and int(parts[2]) in (-1,1)
    except (ValueError, TypeError, AttributeError):
        return False


def load_profile(library, system, player):
    record = defaults(system, player)
    migration_key = f'controls_migration.safe_special_buttons.{system}.{player}'
    migrate = bool(SPECS[system].get('unbound_gamepad')) and library.setting(migration_key, '0') != '1'
    migrated = False
    try:
        saved = json.loads(library.setting(f'controls.{system}.{player}', '{}'))
        if not isinstance(saved, dict): return record
        device = saved.get('device')
        if device == 'auto' or (isinstance(device, str) and device.startswith('index:')
                                and device[6:].isdigit() and int(device[6:]) < 256):
            record['device'] = device
        allowed = dict(actions(system))
        for source in ('keyboard','gamepad'):
            entries = saved.get(source, {})
            if not isinstance(entries, dict): continue
            for action, tokens in entries.items():
                if (migrate and source == 'gamepad' and action == 'button:4096'
                        and 4096 in SPECS[system].get('unbound_gamepad', ())
                        and tokens == ['axis:4:1']):
                    migrated = True
                    continue
                if action in allowed and isinstance(tokens, list) and len(tokens) <= 4 and all(valid_token(t, source) for t in tokens):
                    record[source][action] = tokens
    except (TypeError, ValueError):
        pass
    if migrate:
        library.set_setting(migration_key, '1')
        if migrated:
            library.set_setting(f'controls.{system}.{player}', json.dumps(record))
    return record


def save_profile(library, system, player, record):
    if system not in SPECS or not 0 <= player < SPECS[system]['players']:
        raise ValueError('Invalid console or player')
    library.set_setting(f'controls.{system}.{player}', json.dumps(record))


def bind(record, source, action, token):
    if token is not None and not valid_token(token, source): raise ValueError('Invalid input binding')
    if token:
        for other, tokens in record[source].items():
            if other != action and token in tokens:
                record[source][other] = [value for value in tokens if value != token]
    record[source][action] = [token] if token else []


def keyboard_layout(system, record):
    layout = deepcopy(layout_for(system))
    layout['keys'], layout['axes'] = {}, {}
    for action, tokens in record['keyboard'].items():
        parts = action.split(':')
        for token in tokens:
            key = token.split(':')[1]
            if parts[0] == 'button': layout['keys'][key] = layout['keys'].get(key, 0) | int(parts[1])
            else: layout['axes'][key] = (int(parts[1]), int(parts[2]) * 32767)
    return layout


def gamepad_state(bindings, buttons, axes):
    bits, result = 0, [0,0,0,0]
    for action, tokens in bindings.items():
        strength = 0
        for token in tokens:
            parts = token.split(':')
            if parts[0] == 'button': value = 32767 if int(parts[1]) in buttons else 0
            else: value = max(0, axes[int(parts[1])] * int(parts[2]))
            strength = max(strength, value if value >= 8000 else 0)
        parts = action.split(':')
        if parts[0] == 'button':
            if strength: bits |= int(parts[1])
        else:
            result[int(parts[1])] += strength * int(parts[2])
    return bits, tuple(max(-32768,min(32767,v)) for v in result)


def binding_label(tokens):
    labels = []
    for token in tokens:
        parts = token.split(':')
        if parts[0] == 'key':
            labels.append(QKeySequence(int(parts[1])).toString(QKeySequence.NativeText))
        elif parts[0] == 'button': labels.append(PAD_BUTTONS[int(parts[1])])
        else: labels.append(axis_label(int(parts[1]), int(parts[2])))
    return ' / '.join(labels) or 'Not assigned'
