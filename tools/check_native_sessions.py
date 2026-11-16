#!/usr/bin/env python3
"""Check native persistent progress with original cores and private files only.

Requires a built native player plus EMULUNA_RUST_HOST, INPUT_FIXTURE and HW_FIXTURE.
Opens short windows. Checks both restart/resume and graceful parent-close behavior.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
ROOT=Path(__file__).resolve().parents[1]
PLAYER=ROOT/'target/release/emuluna-native-player'

def state_frame(path):
    magic,header,payload=path.read_bytes().split(b'\n',2)
    assert magic==b'EMULUNA1'
    assert json.loads(header)['sha256']==hashlib.sha256(payload).hexdigest()
    return int.from_bytes(payload[:4],byteorder='little')

def check():
    with tempfile.TemporaryDirectory(prefix='emuluna-native-resume-') as folder:
        root=Path(folder)
        for kind,variable in [('software','EMULUNA_RUST_INPUT_FIXTURE'),('opengl','EMULUNA_RUST_HW_FIXTURE')]:
            work=root/kind;work.mkdir();(work/'bios').mkdir();(work/'original.bin').write_bytes(b'\x5a\xa5')
            states=work/'states';automatic=states/'auto.emuluna-state'
            command=[str(PLAYER),os.environ['EMULUNA_RUST_HOST'],os.environ.get('EMULUNA_RUST_INIT_INPUT_FIXTURE' if kind=='software' else 'EMULUNA_RUST_INIT_HW_FIXTURE',os.environ[variable]),str(work/'original.bin'),str(work/'saves'),str(work/'bios'),'30','--core-id','nestopia','--system','nes','--persistent','--state-dir',str(states),'--auto-resume','--overlay','--mute']
            if kind=='opengl':command+=['--hardware-gl']
            selected=work/"selected.emuluna-state"
            for expected in [30,60]:
                result=subprocess.run(command,cwd=ROOT,capture_output=True,text=True,timeout=20)
                assert result.returncode==0,(kind,result.stderr)
                assert state_frame(automatic)==expected,(kind,expected,state_frame(automatic))
                preview=states/'auto.png'
                assert preview.read_bytes().startswith(b'\x89PNG\r\n\x1a\n'),kind
                assert preview.stat().st_mtime_ns>=automatic.stat().st_mtime_ns,kind
                if expected==30:
                    contents=automatic.read_bytes()
                    selected.write_bytes(contents)
                    previous=automatic.with_suffix('.oesavestate')
                    previous.write_bytes(contents.replace(b'EMULUNA1\n',b'OELINUX1\n',1))
                    automatic.unlink()
                if expected==60:assert 'Native automatic state restored' in result.stderr
            original=selected.read_bytes()
            explicit=subprocess.run(command+['--resume-file',str(selected)],cwd=ROOT,capture_output=True,text=True,timeout=20)
            assert explicit.returncode==0,explicit.stderr
            assert state_frame(automatic)==60 # Explicit frame 30 takes priority over automatic frame 60.
            assert selected.read_bytes()==original
            # Parent close requests an ordinary quit, allowing autosave/flush.
            interactive=command+['--interactive','--parent-control']
            with (work/'parent.log').open('w') as log:
                child=subprocess.Popen(interactive,cwd=ROOT,stdin=subprocess.PIPE,stdout=log,stderr=log)
                try:
                    until=time.monotonic()+5
                    while 'Native playback started' not in (work/'parent.log').read_text() and time.monotonic()<until:
                        time.sleep(.02)
                    assert child.poll() is None
                    assert "Native playback started" in (work/"parent.log").read_text(), (kind,(work/"parent.log").read_text())
                    duplicate=subprocess.run(command,cwd=ROOT,capture_output=True,text=True,timeout=5)
                    assert duplicate.returncode!=0 and 'already in use' in duplicate.stderr
                    time.sleep(.3)
                    child.stdin.write(b'q');child.stdin.flush()
                    assert child.wait(timeout=10)==0,(work/'parent.log').read_text()
                finally:
                    if child.poll() is None:child.kill();child.wait()
                    child.stdin.close()
            assert state_frame(automatic)>60,(kind,state_frame(automatic),(work/"parent.log").read_text())
            # Damaged resume input must fail without overwriting it on close.
            damaged=b'deliberately damaged resume file'
            automatic.write_bytes(damaged)
            failed=subprocess.run(command,cwd=ROOT,capture_output=True,text=True,timeout=10)
            assert failed.returncode!=0
            assert automatic.read_bytes()==damaged
            recovery=subprocess.run(command+['--start-from-boot'],cwd=ROOT,capture_output=True,text=True,timeout=20)
            assert recovery.returncode==0,recovery.stderr
            assert state_frame(automatic)==30
            print(f'{kind}: restart/resume, exclusive save lock, parent close and damaged-state preservation passed',flush=True)
    print('NATIVE_PERSISTENT_SESSIONS_OK',flush=True)
if __name__=='__main__':check()
