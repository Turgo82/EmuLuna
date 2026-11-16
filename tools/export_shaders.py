#!/usr/bin/env python3
"""Build a native shader pack from the existing audited preset adapter.

Python runs only during asset preparation. Native playback consumes exported
GLSL and PNG assets directly, without Python, Qt, image readback or per-frame
transpilation. Original source/attribution comments are retained in the stages.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
sys.path.insert(0, str(ROOT / "archive/legacy-python"))  # Reference-only parity tooling
from emuluna.shaders import ROOT as SHADERS, PRESETS, preset, parameters, parameter_values, stage_source


def export(destination):
    destination.mkdir(parents=True,exist_ok=True)
    records = {}
    for key,spec in PRESETS.items():
        info = preset(key)
        passes = []
        for render_pass in info['passes']:
            entry = {name:value for name,value in render_pass.items() if name != 'source'}
            entry['vertex'] = stage_source(render_pass['source'],'vertex',spec['format'])
            entry['fragment'] = stage_source(render_pass['source'],'fragment',spec['format'])
            passes.append(entry)
        textures = {}
        for name,texture in info['textures'].items():
            path = texture['path'].resolve()
            if not path.is_relative_to(SHADERS.resolve()):
                raise ValueError('Shader texture is outside the bundled shader root')
            filename = hashlib.sha256(path.read_bytes()).hexdigest()+path.suffix.lower()
            shutil.copyfile(path,destination/filename)
            textures[name] = {**texture,'path':filename}
        record = dict(name=spec['name'],passes=passes,textures=textures,
            parameters=parameters(key),values=parameter_values(key),
            minimum_viewport=spec.get('minimum_viewport',[1,1]))
        filename=hashlib.sha256(key.encode()).hexdigest()+'.json'
        (destination/filename).write_text(json.dumps(record,ensure_ascii=False,separators=(',',':')),encoding='utf-8')
        records[key]=dict(name=spec['name'],file=filename,
            author=spec.get('author','See original source notices'),
            license=spec.get('license','See original source notices'))
    for notice in SHADERS.glob('COPYING*'):
        shutil.copyfile(notice,destination/notice.name)
    text=json.dumps(dict(version=1,presets=records),ensure_ascii=False,separators=(',',':'))
    (destination/'pack.json').write_text(text,encoding='utf-8')
    print(f'Exported {len(records)} presets to {destination}; index {len(text.encode())} bytes')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'build/rust/shader-pack')
    export(parser.parse_args().output)
