"""Unpack a PKLITE-compressed DOS executable by running its own decompression
stub under emulation.

This is deliberately not a reimplementation of PKLITE's format. The stub *is* the
specification, so executing it cannot disagree with it. We stop the moment
execution enters memory the program itself wrote, which is the generic signal
that a self-extractor has finished and jumped into its payload.
"""
import struct, sys
from unicorn import *
from unicorn.x86_const import *

MEM = 0x110000            # a little over 1 MB, so segment wrap near the top is safe
PSP_SEG = 0x0100
LOAD_SEG = PSP_SEG + 0x10

def load(path):
    d = open(path, 'rb').read()
    (sig, lastpage, pages, nreloc, hdrpar, minal, maxal,
     ss, sp, csum, ip, cs, relocoff, ovl) = struct.unpack_from('<2sHHHHHHHHHHHHH', d, 0)
    assert sig == b'MZ', sig
    hdr = hdrpar * 16
    imgsize = (pages - 1) * 512 + (lastpage or 512) - hdr
    img = d[hdr:hdr + imgsize]
    relocs = [struct.unpack_from('<HH', d, relocoff + 4 * i) for i in range(nreloc)]
    return dict(img=bytearray(img), relocs=relocs, cs=cs, ip=ip, ss=ss, sp=sp)

def unpack(path, verbose=True):
    e = load(path)
    uc = Uc(UC_ARCH_X86, UC_MODE_16)
    uc.mem_map(0, MEM)

    base = LOAD_SEG * 16
    uc.mem_write(base, bytes(e['img']))

    # Relocations: the loader adds the load segment to each listed word.
    for off, seg in e['relocs']:
        a = base + seg * 16 + off
        v = struct.unpack_from('<H', uc.mem_read(a, 2), 0)[0]
        uc.mem_write(a, struct.pack('<H', (v + LOAD_SEG) & 0xffff))

    # A minimal PSP, enough that anything poking at it sees something sane.
    uc.mem_write(PSP_SEG * 16, b'\xcd\x20' + struct.pack('<H', 0x9fff))

    cs = (LOAD_SEG + e['cs']) & 0xffff
    ss = (LOAD_SEG + e['ss']) & 0xffff
    uc.reg_write(UC_X86_REG_CS, cs)
    uc.reg_write(UC_X86_REG_IP, e['ip'])
    uc.reg_write(UC_X86_REG_SS, ss)
    uc.reg_write(UC_X86_REG_SP, e['sp'])
    uc.reg_write(UC_X86_REG_DS, PSP_SEG)
    uc.reg_write(UC_X86_REG_ES, PSP_SEG)

    stub_lo, stub_hi = base, base + len(e['img'])
    state = dict(written=set(), lo=None, hi=0, stop=None, steps=0, ints=[])
    PAGE = 0x40

    def on_write(uc, access, addr, size, value, data):
        for a in range(addr, addr + size, PAGE):
            state['written'].add(a // PAGE)
        state['lo'] = addr if state['lo'] is None else min(state['lo'], addr)
        state['hi'] = max(state['hi'], addr + size)

    # A self-extractor may relocate its own decompressor and jump into it before
    # it unpacks anything, which also counts as "execution in written memory". So
    # treat the first such jump as a stage boundary: remember that region, forget
    # what has been written, and carry on. The next one is the real payload.
    state['stages'] = []
    MAX_STAGES = 4

    def on_code(uc, addr, size, data):
        state['steps'] += 1
        if addr // PAGE not in state['written']:
            return
        if len(state['stages']) < MAX_STAGES - 1 and state['hi'] - (state['lo'] or 0) < 0x4000:
            state['stages'].append((state['lo'], state['hi'], addr))
            state['written'].clear()
            state['lo'], state['hi'] = None, 0
            return
        state['stop'] = addr
        uc.emu_stop()

    def on_block(uc, addr, size, data):
        if state['steps'] > 40_000_000:
            state['stop'] = -1
            uc.emu_stop()

    def on_intr(uc, intno, data):
        ax = uc.reg_read(UC_X86_REG_AX)
        state['ints'].append((intno, ax))
        if intno == 0x21 and (ax >> 8) in (0x4c, 0x00):
            state['stop'] = -2
            uc.emu_stop()

    uc.hook_add(UC_HOOK_MEM_WRITE, on_write)
    uc.hook_add(UC_HOOK_CODE, on_code)
    uc.hook_add(UC_HOOK_INTR, on_intr)

    start = cs * 16 + e['ip']
    try:
        uc.emu_start(start, MEM, count=0)
    except UcError as err:
        if state['stop'] is None:
            state['stop'] = -3
            state['err'] = str(err)

    res = dict(uc=uc, state=state, base=base,
               entry_cs=uc.reg_read(UC_X86_REG_CS), entry_ip=uc.reg_read(UC_X86_REG_IP),
               ss=uc.reg_read(UC_X86_REG_SS), sp=uc.reg_read(UC_X86_REG_SP))
    if verbose:
        print(f"steps          {state['steps']:,}")
        print(f"stopped at     {state['stop'] if isinstance(state['stop'],int) and state['stop']<0 else hex(state['stop'] or 0)}")
        print(f"wrote          {hex(state['lo'] or 0)} .. {hex(state['hi'])}")
        print(f"entry          {res['entry_cs']:04x}:{res['entry_ip']:04x}  "
              f"(linear {res['entry_cs']*16+res['entry_ip']:#x})")
        print(f"stack          {res['ss']:04x}:{res['sp']:04x}")
        print(f"interrupts     {state['ints'][:8]}")
        for i, (lo, hi, tgt) in enumerate(state['stages']):
            print(f"stage {i}        wrote {lo:#x}..{hi:#x}, jumped to {tgt:#x}")
        print(f"original image {hex(stub_lo)} .. {hex(stub_hi)}")
    return res

if __name__ == '__main__':
    r = unpack(sys.argv[1])
    st = r['state']
    lo = min(r['base'], st['lo'] or r['base'])
    hi = max(st['hi'], r['entry_cs'] * 16 + r['entry_ip'] + 0x100)
    img = bytes(r['uc'].mem_read(lo, hi - lo))
    out = sys.argv[2] if len(sys.argv) > 2 else '/tmp/main.unpacked.bin'
    open(out, 'wb').write(img)
    print(f"\ndumped {len(img):,} bytes from {hex(lo)} to {out}")
    print(f"entry is at file offset {r['entry_cs']*16+r['entry_ip']-lo:#x}")
