"""Bundled community presets, with a small Slang-to-desktop-GLSL adapter.

Sources remain unchanged on disk. This supports the shipped preset vocabulary,
not arbitrary Slang imports. The draw backend lives in shader_renderer.py.
"""
from functools import lru_cache
import json
import math
from pathlib import Path
import re
import shlex

ROOT = Path(__file__).parent / 'data' / 'shaders'
PRESETS = json.loads((ROOT / 'manifest.json').read_text())
PARAMETER = re.compile(r'^\s*#pragma parameter\s+(\w+)\s+"([^"]+)"\s+([-\d.eE+]+)\s+([-\d.eE+]+)\s+([-\d.eE+]+)(?:\s+([-\d.eE+]+))?', re.M)


class ShaderError(RuntimeError):
    pass


def read_config(path):
    result = {}
    for line in path.read_text().splitlines():
        if '=' not in line or line.lstrip().startswith('#'):
            continue
        name, value = line.split('=', 1)
        tokens = shlex.split(value, comments=True)
        result[name.strip()] = ' '.join(tokens)
    return result


def shader_source(path, stack=()):
    path = path.resolve()
    if not path.is_relative_to(ROOT.resolve()) or path in stack or len(stack) > 32:
        raise ShaderError('Invalid shader include: ' + path.name)
    source = path.read_text()
    return re.sub(r'^\s*#include\s+"([^"]+)"[^\n]*',
                  lambda m: shader_source(path.parent / m[1], (*stack, path)), source, flags=re.M)


@lru_cache(maxsize=32)
def preset(key):
    spec = PRESETS[key]
    path = ROOT / spec['preset']
    config = read_config(path)
    passes = []
    for i in range(int(config['shaders'])):
        def field(name, default=None):
            return config.get(name + str(i), default)
        passes.append(dict(source=path.parent / field('shader'), linear=field('filter_linear','false') == 'true',
            wrap=field('wrap_mode', field('texture_wrap_mode','clamp_to_border')),
            mipmap=field('mipmap_input','false') == 'true',
            float=field('float_framebuffer','false') == 'true',
            srgb=field('srgb_framebuffer','false') == 'true', alias=field('alias',''),
            mod=int(field('frame_count_mod','0')),
            # scale_type_0 is an upstream typo, not part of the preset format.
            scale_x=float(field('scale_x',field('scale','1'))),
            scale_y=float(field('scale_y',field('scale','1'))),
            type_x=field('scale_type_x',field('scale_type','viewport' if i == int(config['shaders'])-1 else 'source')),
            type_y=field('scale_type_y',field('scale_type','viewport' if i == int(config['shaders'])-1 else 'source'))))
    textures = {}
    for name in filter(None, config.get('textures','').split(';')):
        textures[name] = dict(path=path.parent/config[name], linear=config.get(name+'_linear','true') == 'true',
                              mipmap=config.get(name+'_mipmap','false') == 'true',
                              wrap=config.get(name+'_wrap_mode','clamp_to_border'))
    return dict(passes=passes,textures=textures,config=config)


@lru_cache(maxsize=32)
def parameters(key):
    info = preset(key)
    result = {}
    for render_pass in info['passes']:
        for name,label,default,low,high,step in PARAMETER.findall(shader_source(render_pass['source'])):
            # Upstream uses zero-range parameters as section labels. Do not make
            # uneditable spin boxes for these decorative entries.
            if float(low) == float(high):
                continue
            result[name] = dict(key=name,label=label,default=float(info['config'].get(name,default)),
                                minimum=float(low),maximum=float(high),step=float(step or '.01'))
    return list(result.values())


def parameter_values(key, saved=None):
    saved = saved if isinstance(saved,dict) else {}
    values = {}
    for spec in parameters(key):
        try:
            value = float(saved.get(spec['key'],spec['default']))
            if not math.isfinite(value):
                raise ValueError('Nonfinite parameter')
        except (TypeError,ValueError,OverflowError):
            value = spec['default']
        values[spec['key']] = max(spec['minimum'],min(spec['maximum'],value))
    return values


def stage_source(path, stage, dialect='slang'):
    source = shader_source(path)
    source = re.sub(r'^\s*#version[^\n]*','',source,flags=re.M)
    source = re.sub(r'^\s*#pragma (?:parameter|name|format)[^\n]*','',source,flags=re.M)
    if dialect == 'glsl':
        return '#version 330\n#define '+stage.upper()+'\n#define PARAMETER_UNIFORM\n'+source
    # Slang common code precedes its vertex and fragment sections. Includes may
    # introduce stage markers too, so select sections after expanding includes.
    selected, current = [], None
    for line in source.splitlines():
        match = re.match(r'\s*#pragma stage (\w+)',line)
        if match:
            current = match[1]
        elif current in (None,stage):
            selected.append(line)
    source = '\n'.join(selected)
    # Desktop GL uniforms replace Vulkan descriptor sets/push constants. Keep
    # member names and shader expressions identical to the upstream source.
    source = re.sub(r'layout\s*\([^)]*\)\s*uniform\s+(\w+)\s*\{(.*?)\}\s*(\w+)\s*;',
                    lambda m: 'struct '+m[1]+' { '+m[2]+' };\nuniform '+m[1]+' '+m[3]+';',source,flags=re.S)
    # Binding/location qualifiers aren't needed: locations are bound by name,
    # and GL links varyings by name. This also runs on desktop OpenGL 3.3.
    source = re.sub(r'layout\s*\([^)]*\)\s*','',source)
    return '#version 330\n#extension GL_ARB_shading_language_420pack : enable\n#extension GL_ARB_arrays_of_arrays : enable\n#extension GL_ARB_gpu_shader5 : enable\n'+source


def is_software_renderer(name):
    return any(word in name.lower() for word in ('llvmpipe','softpipe','swrast','software','gdi generic','swiftshader','basic render'))


# Keep the former public import without coupling catalog loading to GL classes.
def __getattr__(name):
    if name == 'ShaderRenderer':
        from .shader_renderer import ShaderRenderer
        return ShaderRenderer
    raise AttributeError(name)
