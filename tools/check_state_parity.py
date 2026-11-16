#!/usr/bin/env python3
"""Check Rust reads both archived and current state envelopes, using temporary files."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
sys.path.insert(0, str(ROOT / "archive/legacy-python"))  # Reference-only parity tooling
from emuluna.core import Core
PAYLOAD=b'Original state interoperability fixture\0\n\xff'*301
class NativeStub:
    def el_state_size_limit(self): return 128*1024*1024
    def el_save_state(self,handle,path):
        Path(os.fsdecode(path)).write_bytes(PAYLOAD)
        return 1
    def el_load_state(self,handle,path):
        assert Path(os.fsdecode(path)).read_bytes()==PAYLOAD
        return 1

def main():
    binary=ROOT/'target/release/examples/state_probe'
    if not binary.is_file(): raise RuntimeError('Build the state-file state_probe example first')
    with tempfile.TemporaryDirectory(prefix='emuluna-state-parity-') as directory:
        root=Path(directory)
        for renderer in ('software','opengl','vulkan'):
            core=Core.__new__(Core)
            core.hardware=None;core.handle=object();core.lib=NativeStub()
            core.name='libretro:fixture:'+'a'*64;core.digest='b'*64;core.state_renderer=renderer
            legacy=root/f'{renderer}.legacy'
            core.save_state(legacy)
            raw=root/f'{renderer}.raw'
            common=['fixture','a'*64,'b'*64,renderer]
            subprocess.run([str(binary),'verify',str(legacy),str(raw),*common],check=True)
            assert raw.read_bytes()==PAYLOAD
            native=root/f'{renderer}.native'
            subprocess.run([str(binary),'wrap',str(raw),str(native),*common],check=True)
            assert native.read_bytes().startswith(b'EMULUNA1\n')
            restored=root/f'{renderer}.restored'
            subprocess.run([str(binary),'verify',str(native),str(restored),*common],check=True)
            assert restored.read_bytes()==PAYLOAD
            print(f'{renderer}: archived and current states passed')
    print('NATIVE_STATE_PARITY_OK')
if __name__=='__main__': main()
