"""CC0 diagnostic NROM: blue idle, red A, green B, yellow A+B, pulse tone."""
import struct


def nes(port=0):
    assert port in (0, 1)
    code, labels, fixups = bytearray(), {}, []
    def emit(*values): code.extend(values)
    def label(name): labels[name] = len(code)
    def branch(op, name):
        emit(op, 0)
        fixups.append((len(code)-1, name, True))
    def jump(name):
        emit(0x4c, 0, 0)
        fixups.append((len(code)-2, name, False))
    emit(0x78, 0xd8, 0xa2, 0xff, 0x9a)  # SEI, CLD, stack
    emit(0xa9, 0, 0x8d, 0, 0x20, 0x8d, 1, 0x20)
    for name in ('vblank1', 'vblank2'):
        label(name); emit(0x2c, 2, 0x20); branch(0x10, name)
    for address,value in ((0x4015,1),(0x4000,0xbf),(0x4002,0xfd),(0x4003,8),(0x2001,8)):
        emit(0xa9,value,0x8d,address&255,address>>8)
    label('loop'); emit(0x2c,2,0x20); branch(0x10,'loop')
    emit(0xa9,1,0x8d,0x16,0x40,0xa9,0,0x8d,0x16,0x40)
    # The first two serial bits are A and B. Read both independently so tests
    # catch swapped buttons and accidentally mapping both controls to A.
    emit(0xad,0x16+port,0x40,0x29,1,0x85,0)
    emit(0xad,0x16+port,0x40,0x29,1); branch(0xf0,'no_b')
    emit(0xa5,0); branch(0xf0,'green')
    emit(0xa2,0x28); jump('paint')
    label('green'); emit(0xa2,0x1a); jump('paint')
    label('no_b'); emit(0xa5,0); branch(0xf0,'blue')
    emit(0xa2,0x16); jump('paint')
    label('blue'); emit(0xa2,0x21)
    label('paint')
    emit(0xa9,0x3f,0x8d,6,0x20,0xa9,0,0x8d,6,0x20,0x8e,7,0x20)
    emit(0xa9,0,0x8d,6,0x20,0x8d,6,0x20,0x8d,5,0x20,0x8d,5,0x20)
    jump('loop')
    for offset,name,relative in fixups:
        if relative: code[offset]=(labels[name]-offset-1)&255
        else: code[offset:offset+2]=struct.pack('<H',0x8000+labels[name])
    prg=bytearray(16384); prg[:len(code)]=code
    for offset in (0x3ffa,0x3ffc,0x3ffe): prg[offset:offset+2]=struct.pack('<H',0x8000)
    return b'NES\x1a'+bytes([1,1,0,0])+bytes(8)+prg+bytes(8192)
