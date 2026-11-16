#!/usr/bin/env python3
"""Export current default and custom binding behavior for Rust parity checks.

Read-only catalogs; never opens the library. Qt/Python are test-time reference
code only and are not called by native playback.
"""
from copy import deepcopy
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from PySide6.QtCore import Qt
sys.path.insert(0, str(ROOT / "archive/legacy-python"))  # Reference-only parity tooling
from emuluna import controller_profiles as profiles
from emuluna.controls import combined_axes


def export(destination):
    controls=json.loads((ROOT/'data/controls.json').read_text())
    names=sorted({name for layout in controls.values() for source in ('keys','axes') for name in layout.get(source,{})}|{'Key_C','Key_V','Key_B'})
    groups=[]
    for system in profiles.SPECS:
        default=profiles.defaults(system,0)
        custom=deepcopy(default)
        primary=profiles.SPECS[system]['primary']
        profiles.bind(custom,'keyboard',f'button:{primary[0]}',f'key:{int(Qt.Key_C)}')
        profiles.bind(custom,'gamepad',f'button:{primary[0]}','button:0')
        if len(primary)>1:
            profiles.bind(custom,'keyboard',f'button:{primary[1]}',f'key:{int(Qt.Key_B)}')
            profiles.bind(custom,'gamepad',f'button:{primary[1]}','button:15')
        if profiles.SPECS[system].get('analog'):
            profiles.bind(custom,'keyboard','axis:0:-1',f'key:{int(Qt.Key_V)}')
            profiles.bind(custom,'gamepad','axis:0:-1','axis:2:-1')
        custom['device']='index:3'
        for saved,record in [(None,default),(default,default),(custom,custom)]:
            layout=profiles.keyboard_layout(system,record)
            group=dict(system=system,saved=saved,requested_device=3 if saved is custom else None,cases=[])
            def sample(keys=(),buttons=(),axes=(0,0,0,0,0,0)):
                pressed={str(int(getattr(Qt,name))) for name in keys}
                kb=0
                for key,bit in layout['keys'].items():
                    if key in pressed: kb|=bit
                pb,pa=profiles.gamepad_state(record['gamepad'],set(buttons),axes)
                result=combined_axes(layout['axes'],pressed,pa)
                group['cases'].append(dict(keys=list(keys),pad_buttons=sum(1<<n for n in buttons),pad_axes=axes,buttons=kb|pb,axes=result))
            sample()
            for name in names: sample(keys=(name,))
            for index in range(21): sample(buttons=(index,))
            for index in range(6):
                for value in (-32768,-8000,-7999,7999,8000,32767):
                    axes=[0]*6;axes[index]=value;sample(axes=axes)
            sample(keys=('Key_Left','Key_Right'),axes=(12000,16000,-12000,16000,0,0))
            sample(keys=('Key_Return','Key_Enter'),buttons=(6,))
            sample(keys=tuple(names),buttons=tuple(range(21)),axes=(32767,)*6)
            groups.append(group)
    destination.parent.mkdir(parents=True,exist_ok=True)
    destination.write_text(json.dumps(dict(format=1,profiles=groups)))
    print(f'Exported {sum(len(g["cases"]) for g in groups)} cases / {len(groups)} default and saved profiles / {len(profiles.SPECS)} systems to {destination}')
if __name__=='__main__':
    export(Path(sys.argv[1]) if len(sys.argv)>1 else ROOT/'build/rust/input-reference.json')
