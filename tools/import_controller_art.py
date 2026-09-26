#!/usr/bin/env python3
"""Prepare user-supplied Pinapple_Graphics artwork; never modify the originals.

Requires Pillow only when preparing assets, not at application runtime.
Hotspots use original-image coordinates; exported images retain that aspect.
"""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ART = ROOT / 'emuluna/data/controllers'
SOURCE = ROOT / 'Controller_Vectors_by_Pinapple_Graphics_(Normal_300ppi)_v2.1'


def button(bit, label, x, y, w, h, group='Buttons', ellipse=False, rotation=0):
    result = dict(action=f'button:{bit}', label=label, rect=[x-w/2, y-h/2, w, h],
                  group=group, ellipse=ellipse)
    if rotation:
        result['rotation'] = rotation
    return result


def dpad(x, y, step, width):
    return [button(bit, label, x+dx*step, y+dy*step, width, width, 'Directional pad')
            for bit, label, dx, dy in ((64,'↑',0,-1),(128,'↓',0,1),
                                      (32,'←',-1,0),(16,'→',1,0))]


def face(bit, label, x, y, diameter):
    return button(bit, label, x, y, diameter, diameter, ellipse=True)


def menu(bit, label, x, y, w, h, rotation=0, *, ellipse=False):
    return button(bit, label, x, y, w, h, 'Menu buttons', ellipse=ellipse, rotation=rotation)


def shoulder(bit, label, x, y, w, h, rotation=0):
    return button(bit, label, x, y, w, h, 'Shoulders & triggers', rotation=rotation)


def axis_button(axis, sign, label, x, y, w, h, group='Analog controls', ellipse=False):
    part = button(0, label, x, y, w, h, group, ellipse)
    part['action'] = f'axis:{axis}:{sign}'
    return part


def stick(x, y, radius, axis=0, click=None):
    """Four separate directional regions, with room for an optional stick click."""
    parts = [axis_button(a, sign, label, x+dx*radius*.6, y+dy*radius*.6,
                         radius*.7, radius*.7)
             for a, sign, label, dx, dy in ((axis+1,-1,'↑',0,-1),
                 (axis+1,1,'↓',0,1),(axis,-1,'←',-1,0),(axis,1,'→',1,0))]
    if click:
        parts.append(button(click, 'Stick click', x, y, radius*.45, radius*.45,
                            'Analog controls', ellipse=True))
    return parts


def coleco_keypad():
    actions = [2048,1024,512,256,4096,8192,16384,32768,'axis:1:1',8,'axis:0:1',4]
    result = []
    for index, (action, label) in enumerate(zip(actions,('1','2','3','4','5','6','7','8','9','*','0','#'))):
        part = button(action if isinstance(action,int) else 0, label,
                      (101,214,326)[index%3], (524,638,752,865)[index//3],94,94,'Keypad')
        if isinstance(action,str): part['action'] = action
        result.append(part)
    return result


# Every matching controller supplied for the current system catalog. Keep
# fallback SVGs for models absent from the source collection.
MODELS = {
    'nes': ('Nintendo NES.png', 'NES controller', '',
        dpad(192,288,63,62) + [menu(4,'Select',439,339,94,40),
        menu(8,'Start',581,339,94,40), face(2,'B',771,340,98), face(1,'A',915,340,98)]),
    'fds': ('Nintendo Famicom.png', 'Famicom controller', '',
        dpad(195,293,59,62) + [menu(4,'Select',439,350,88,36),
        menu(8,'Start',564,350,88,36), face(2,'B',762,340,99), face(1,'A',904,340,99)]),
    'snes': ('Super Nintendo.png', 'Super Nintendo controller', '',
        dpad(260,281,71,71) + [menu(4,'Select',508,320,95,39,-37),
        menu(8,'Start',640,320,95,39,-37), face(1,'A',1102,283,96),
        face(2,'B',982,374,96), face(1024,'X',982,186,96), face(2048,'Y',860,280,96),
        shoulder(512,'L',320,11,150,18), shoulder(256,'R',924,11,150,18)]),
    'genesis': ('Sega Genesis 6 Button.png', 'Mega Drive / Genesis six-button controller',
        'The Mode button is on the back; use its binding in the list.',
        dpad(295,263,69,67) + [menu(8,'Start',577,234,91,32),
        face(2048,'A',793,363,101), face(2,'B',902,303,101), face(1,'C',1018,270,101),
        face(512,'X',754,242,72), face(1024,'Y',846,195,72), face(256,'Z',946,172,72)]),
    'saturn': ('Sega Saturn A.png', 'Sega Saturn controller', '',
        dpad(303,357,79,79) + [menu(8,'Start',639,461,106,44),
        face(2,'A',868,469,110), face(1,'B',988,385,110), face(256,'C',1125,332,110),
        face(2048,'X',823,329,80), face(1024,'Y',928,256,80), face(512,'Z',1044,209,80),
        shoulder(4096,'L',286,23,152,22,-15), shoulder(8192,'R',993,23,152,22,15)]),
    'pce': ('NEC TurboGrafx-16.png', 'PC Engine / TurboGrafx-16 · TurboPad',
        'Two-button pad shown. Six-button controls and pad mode are available in the list.',
        dpad(191,397,69,61) + [menu(4,'Select',524,437,84,34),menu(8,'Run',666,437,84,34),
        face(2,'II',884,439,89),face(1,'I',1019,439,89)]),
    'psx': ('Sony PlayStation DualShock1.png', 'PlayStation · DualShock',
        'L2 and R2 are behind the shoulders; configure them in the list. Click the center of a stick for L3 or R3.',
        dpad(265,312,67,65) + [menu(4,'Select',530,312,54,33),menu(8,'Start',757,312,44,32),
        face(1024,'△',1021,217,76),face(1,'○',1117,313,76),
        face(2,'×',1021,408,76),face(2048,'□',925,313,76),
        shoulder(512,'L1',264,18,130,22),shoulder(256,'R1',1021,18,130,22)]
        + stick(454,505,82,click=16384) + stick(831,505,82,axis=2,click=32768)),
    'n64': ('Nintendo 64.png', 'Nintendo 64 controller',
        'The Z trigger is on the back; configure it in the list.',
        dpad(221,353,62,62) + [menu(8,'Start',578,388,80,80,ellipse=True),
        face(2048,'B',797,382,87),face(2,'A',877,462,87),
        axis_button(3,-1,'C up',966,246,64,64,'C buttons',True),
        axis_button(3,1,'C down',966,378,64,64,'C buttons',True),
        axis_button(2,-1,'C left',899,312,64,64,'C buttons',True),
        axis_button(2,1,'C right',1034,312,64,64,'C buttons',True),
        shoulder(512,'L',222,110,202,21,-11),shoulder(256,'R',934,110,202,21,11)]
        + stick(578,620,50)),
    'vb': ('Nintendo Virtual Boy.png', 'Virtual Boy controller',
        'The L and R triggers are on the back; configure them in the list.',
        dpad(175,181,57,51) + [
        button(4096,'Right pad up',962,124,51,51,'Right directional pad'),
        button(16384,'Right pad down',962,238,51,51,'Right directional pad'),
        button(8192,'Right pad left',905,181,51,51,'Right directional pad'),
        button(32768,'Right pad right',1019,181,51,51,'Right directional pad'),
        face(2,'B',719,296,68),face(1,'A',812,251,68),
        menu(4,'Select',327,251,68,68,ellipse=True),menu(8,'Start',420,296,68,68,ellipse=True)]),
    'gb': ('Nintendo Game Boy.png', 'Game Boy', '',
        dpad(123,674,46,44) + [face(2,'B',416,685,72),face(1,'A',514,641,72),
        menu(4,'Select',216,813,73,25,-28),menu(8,'Start',317,813,73,25,-28)]),
    'gbc': ('Nintendo Game Boy Color purple.png', 'Game Boy Color', '',
        dpad(126,641,43,43) + [face(2,'B',403,657,67),face(1,'A',509,622,67),
        menu(4,'Select',256,804,51,19),menu(8,'Start',344,804,51,19)]),
    'gba': ('Nintendo Game Boy Advance.png', 'Game Boy Advance', '',
        dpad(151,275,48,45) + [face(2,'B',963,295,73),face(1,'A',1069,257,73),
        menu(8,'Start',231,439,34,34,ellipse=True),menu(4,'Select',231,504,34,34,ellipse=True),
        shoulder(512,'L',137,47,144,29,-14),shoulder(256,'R',1033,47,144,29,14)]),
    'nds': ('Nintendo DS.png', 'Nintendo DS · DS Lite',
        'L and R are behind the hinge; configure them in the list.',
        dpad(117,753,46,42) + [face(1024,'X',897,672,49),face(1,'A',956,730,49),
        face(2,'B',897,790,49),face(2048,'Y',838,730,49),
        menu(8,'Start',823,918,25,25,ellipse=True),menu(4,'Select',823,977,25,25,ellipse=True)]),
    'psp': ('Sony PSP.png', 'PlayStation Portable', '',
        dpad(136,229,59,51) + [face(1024,'△',1125,156,62),face(1,'○',1199,229,62),
        face(2,'×',1125,303,62),face(2048,'□',1051,229,62),
        menu(4,'Select',899,491,65,25),menu(8,'Start',981,491,65,25),
        shoulder(512,'L',148,22,95,28),shoulder(256,'R',1106,22,95,28)]
        + stick(141,399,42)),
    'gg': ('Sega Game Gear.png', 'Game Gear', '',
        dpad(136,258,46,43) + [face(2,'1',1023,305,65),face(1,'2',1115,235,65),
        button(8,'Start',1037,143,28,45,'Menu buttons',ellipse=True)]),
    'lynx': ('Atari Lynx.png', 'Atari Lynx',
        'Both A/B pairs are available for the two holding orientations.',
        dpad(210,240,39,34) + [face(2,'B',1026,86,63),face(1,'A',1108,73,63),
        face(2,'B',1026,394,63),face(1,'A',1108,407,63),
        menu(512,'Option 1',921,173,46,16),menu(8,'Pause',921,240,46,16),
        menu(256,'Option 2',921,306,46,16)]),
    'ngpc': ('SNK Neo Geo Pocket Color.png', 'Neo Geo Pocket Color', '',
        dpad(152,284,47,40) + [face(1,'A',896,283,72),face(2,'B',1000,211,72),
        menu(8,'Option',994,72,40,40,ellipse=True)]),
    'wsc': ('Bandai Wonderswan Color Crystal Blue Violet.png', 'WonderSwan Color · SwanCrystal',
        'Screen rotation is available in the binding list.',
        [button(8192,'Y1',145,113,49,42,'Y buttons'),button(4096,'Y3',145,233,49,42,'Y buttons'),
        button(512,'Y4',86,174,40,51,'Y buttons'),button(256,'Y2',204,174,40,51,'Y buttons'),
        button(64,'X1',149,411,49,42,'X buttons'),button(128,'X3',149,535,49,42,'X buttons'),
        button(32,'X4',89,473,40,51,'X buttons'),button(16,'X2',208,473,40,51,'X buttons'),
        button(1,'A',1024,458,41,54),button(2,'B',965,517,52,38),menu(8,'Start',480,575,61,29)]),
    'sms': ('Sega Master System.png', 'Master System control pad',
        'Pause is on the console; configure it in the list.',
        dpad(290,230,84,72) + [face(2,'1',775,285,106),face(1,'2',961,285,106)]),
    'atari2600': ('Atari 2600.png', 'Atari 2600 · CX40 joystick',
        'Select, Reset, difficulty and color switches are on the console; configure them in the list.',
        dpad(417,417,74,66) + [face(2,'Fire',144,144,128)]),
    'atari7800': ('Atari 7800.png', 'Atari 7800 · CX78 gamepad',
        'Select, Pause, Reset and difficulty switches are on the console; configure them in the list.',
        dpad(185,155,74,54) + [face(2,'1',547,370,130),face(1,'2',792,370,130)]),
    'colecovision': ('Coleco Colecovision.png', 'ColecoVision hand controller', '',
        dpad(214,137,57,49) + [button(2,'Left side button',32,305,54,84),
        button(1,'Right side button',390,305,54,84)] + coleco_keypad()),
    'intellivision': ('Mattel Intellivision.png', 'Intellivision hand controller',
        'Other keypad numbers use the core’s on-screen keypad; its controls are in the list.',
        [button(32768,'5',244,341,65,65,'Keypad'),button(4096,'Clear',140,545,65,65,'Keypad'),
        button(16384,'0',244,545,65,65,'Keypad'),button(8192,'Enter',348,545,65,65,'Keypad'),
        button(2048,'Top action',7,330,12,48),button(2048,'Top action',481,330,12,48),
        button(2,'Left action',7,389,12,48),button(1,'Right action',481,389,12,48)] + stick(244,825,110)),
    'vectrex': ('GCE Vectrex General Consumer Electronics.png', 'Vectrex controller', '',
        dpad(267,197,35,29) + [face(1,'1',510,196,77),face(2,'2',649,196,77),
        face(1024,'3',788,196,77),face(2048,'4',928,196,77)]),
}

EXTRA_GROUPS = {
    'psx': {f'button:{bit}':'Shoulders & triggers' for bit in (4096,8192)},
    'n64': {'button:4096':'Shoulders & triggers'},
    'nds': {f'button:{bit}':'Shoulders & triggers' for bit in (512,256)},
    'vb': {f'button:{bit}':'Shoulders & triggers' for bit in (512,256)},
    'pce': {f'button:{bit}':'Six-button pad' for bit in (2048,1024,512,256,4096)},
}

# Exact matches are absent from the supplied collection. Do not substitute a
# different console or color model and silently mislabel its physical layout.
FALLBACKS = {
    'atari5200': 'No Atari 5200 controller in the supplied folder.',
    'odyssey2': 'No Odyssey² controller in the supplied folder.',
    'sg1000': 'No SG-1000 controller in the supplied folder.',
    'ngp': 'Only the Neo Geo Pocket Color model was supplied.',
    'ws': 'Only the color SwanCrystal model was supplied.',
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=SOURCE)
    parser.add_argument('--only', nargs='+', choices=MODELS)
    parser.add_argument('--replace-local-edits', action='store_true',
                        help='Explicitly replace locally edited app images with fresh source conversions')
    args = parser.parse_args()
    destination = ART
    destination.mkdir(exist_ok=True)
    layout_path = ART / 'imported_layouts.json'
    manifest_path = destination / 'artwork_manifest.json'
    layouts = json.loads(layout_path.read_text()) if layout_path.exists() else {}
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    for key in args.only or MODELS:
        filename, title, note, controls = MODELS[key]
        original = args.source / filename
        output = destination / f'{key}.webp'
        record = manifest.get(key, {})
        if output.exists() and not args.replace_local_edits:
            digest = hashlib.sha256(output.read_bytes()).hexdigest()
            if record.get('local_edit') or digest != record.get('image_sha256'):
                print(f'{key}: keeping existing local artwork edits')
                if record:
                    record.setdefault('converted_image_sha256',record.get('image_sha256'))
                    record.setdefault('local_edit','Existing local artwork edits retained; not regenerated')
                    record['image_sha256'] = digest
                    record['image_bytes'] = output.stat().st_size
                    with Image.open(output) as edited:
                        record['image_size'] = list(edited.size)
                continue
        with Image.open(original) as image:
            image = image.convert('RGBA')
            size = list(image.size)
            image.thumbnail((1100,1100), Image.Resampling.LANCZOS)
            image.save(output, 'WEBP', lossless=True, method=6)
            manifest[key] = dict(artist='Pinapple_Graphics', source=filename,
                collection=SOURCE.name, source_sha256=hashlib.sha256(original.read_bytes()).hexdigest(),
                source_size=size, source_bytes=original.stat().st_size,
                image_size=list(image.size), image_bytes=output.stat().st_size,
                image_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                processing='Fit within 1100 × 1100 with Lanczos; lossless WebP; transparency preserved')
        layouts[key] = dict(title=title, size=size, note=note, controls=controls,
                            image=f'{key}.webp', extra_groups=EXTRA_GROUPS.get(key, {}))
        print(f'{key}: {original.stat().st_size:,} → {output.stat().st_size:,} bytes')
    layout_path.write_text(json.dumps(layouts, indent=2)+'\n')
    manifest_path.write_text(json.dumps(manifest, indent=2)+'\n')


if __name__ == '__main__':
    main()
