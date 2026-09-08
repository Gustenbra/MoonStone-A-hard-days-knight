#!/usr/bin/env python3
"""Recover the symbol table of Moonstone's MAIN.EXE.

MAIN.EXE is packed TWICE: PKLITE on the outside, Microsoft EXEPACK inside.
Unpacking only the PKLITE layer (what tools/unpack_pklite.py does) leaves an
EXEPACK'd image whose data is still RLE-compressed, which is why the symbol
table there looks like unstructured strings with junk between them.

Peel both layers and the image contains a real, complete TASM symbol table:
11 blocks, one per source module, in link order, each a run of records

    [u8 reclen][u8 kind][payload][u8 namelen][name]      reclen counts everything after itself

    kind 0x05  payload = u16 offset, u16 segment, u16 type   (data / addressed symbol)
    kind 0x0b  payload = u16 offset, u8 0                    (near label, module's code segment)

Blocks are 16-byte aligned and separated by zero padding.

Addresses are link-time and relative to the start of the unpacked image:

    code   offset  ->  image offset  (code segment base is 0; entry is 0000:0000)
    data   seg:off ->  seg*16 + off  (DGROUP is 0x123b, i.e. image offset 0x123b0)

Usage:  python3 symbolmap.py MAIN.EXE [out.json] [--image out.bin]
Needs:  pip install unicorn capstone
"""
import json, struct, sys, os

MODULES = ['MOON', 'GFX', '_KBD', '_DOS', '_LOADER', '_EMM',
           '_TASK', '_MAP', '_TAVERN', '_WIZARD', '_STATUS']
DGROUP = 0x123b
CODE_END = 0xd800


# ---------------------------------------------------------------- unpacking

def unpack(exe_path, tools_dir=None):
    """PKLITE, then EXEPACK. Returns the fully unpacked load image."""
    for cand in ([tools_dir] if tools_dir else []) + [
            os.path.dirname(os.path.abspath(__file__)),
            os.path.join(os.path.dirname(os.path.abspath(exe_path)), '..', 'tools')]:
        if cand and os.path.exists(os.path.join(cand, 'unpack_pklite.py')):
            sys.path.insert(0, cand); break
    from unicorn import Uc, UC_ARCH_X86, UC_MODE_16, UC_HOOK_CODE, UcError
    from unicorn.x86_const import (UC_X86_REG_CS, UC_X86_REG_IP, UC_X86_REG_SS,
                                   UC_X86_REG_SP, UC_X86_REG_DS, UC_X86_REG_ES,
                                   UC_X86_REG_AX, UC_X86_REG_BX, UC_X86_REG_CX,
                                   UC_X86_REG_DX, UC_X86_REG_SI, UC_X86_REG_DI,
                                   UC_X86_REG_BP)
    import unpack_pklite as up

    r = up.unpack(exe_path, verbose=False)
    old, base = r['uc'], r['base']
    cs, ip = r['entry_cs'], r['entry_ip']

    hdr = bytes(old.mem_read(cs * 16, 18))
    if hdr[16:18] != b'RB':
        # No EXEPACK layer; the PKLITE output is already the program.
        return bytes(old.mem_read(base, r['state']['hi'] - base))
    real_ip, real_cs, _, _, _, _, dest_len, _ = struct.unpack('<8H', hdr[:16])

    # Fresh emulator so unpack_pklite's stop-on-written-memory hook is gone.
    uc = Uc(UC_ARCH_X86, UC_MODE_16)
    uc.mem_map(0, up.MEM)
    uc.mem_write(0, bytes(old.mem_read(0, up.MEM)))
    for reg in (UC_X86_REG_SS, UC_X86_REG_SP, UC_X86_REG_DS, UC_X86_REG_ES,
                UC_X86_REG_AX, UC_X86_REG_BX, UC_X86_REG_CX, UC_X86_REG_DX,
                UC_X86_REG_SI, UC_X86_REG_DI, UC_X86_REG_BP):
        uc.reg_write(reg, old.reg_read(reg))
    uc.reg_write(UC_X86_REG_CS, cs)
    uc.reg_write(UC_X86_REG_IP, ip)

    target = base + real_cs * 16 + real_ip          # the program's real entry
    state = {'n': 0}

    def on_code(u, addr, size, data):
        state['n'] += 1
        if addr == target or state['n'] > 60_000_000:
            u.emu_stop()

    uc.hook_add(UC_HOOK_CODE, on_code)
    try:
        uc.emu_start(cs * 16 + ip, up.MEM, count=0)
    except UcError:
        pass                                        # faults after the payload is written
    image = bytearray(uc.mem_read(base, dest_len * 16))

    # The emulated stub stops short, and the last thing it fails to write is the
    # bottom of DGROUP. See finish_exepack: the stream is finished in Python and
    # DGROUP is taken from that, which is what makes SelectPAL, CCOL, CRText,
    # MapIconsTABLE, LairLocation and the rest of MOON's low data readable.
    packed = bytes(old.mem_read(base, dest_len * 16))
    full = finish_exepack(packed, cs * 16 - base, dest_len * 16)
    dg = DGROUP * 16
    late = [a for a in range(dg, len(image)) if image[a] != full[a]]
    early = sum(1 for a in range(dg) if image[a] != full[a])
    if late:
        image[dg:] = full[dg:]
        print('DGROUP: %d bytes recovered by finishing the stream (%#x..%#x); '
              '%d bytes of code left in the emulated stub\'s coordinates'
              % (len(late), late[0], late[-1] + 1, early))
    return bytes(image)


def finish_exepack(packed, packed_end, total):
    """Decompress the EXEPACK stream the whole way, in Python.

    The emulated stub does not finish. Its last act before jumping to the
    program is a `rep movsb` that leaves the destination pointer 21,400 bytes
    above the bottom of the image, so everything below that keeps whatever the
    packed file had there: the source bytes of that same copy, which is why the
    unwritten span reads as a byte-for-byte duplicate of the region 0x5398
    higher. It is stale, not uninitialised, and it was read as uninitialised
    for a long time.

    Microsoft EXEPACK's stream is read backwards from just below the header,
    past the 0xff padding: a command byte, a 16-bit count, and for a fill one
    more byte. `0xb0`/`0xb1` fill, `0xb2`/`0xb3` copy, and bit 0 set is the last
    command. Run to the end it stops with source and destination pointers equal,
    which is the packer's own signal that everything below is already in place.

    Only DGROUP is taken from the result. The rest of the unwritten span is
    past the end of the last code module, and correcting the code as well would
    move every code address in `docs/` by up to 473 bytes, since the shift map
    fitted below is exactly this displacement. That is a change of its own.
    """
    out = bytearray(packed)
    si = packed_end - 1
    while out[si] == 0xff:              # the packer pads the stream up to the header
        si -= 1
    di = total - 1
    while True:
        cmd = out[si]; si -= 1
        count = out[si - 1] | (out[si] << 8); si -= 2
        if cmd & 0xfe == 0xb0:
            fill = out[si]; si -= 1
            out[di - count + 1:di + 1] = bytes([fill]) * count
            di -= count
        elif cmd & 0xfe == 0xb2:
            for _ in range(count):      # may overlap, so byte at a time
                out[di] = out[si]; di -= 1; si -= 1
        else:
            raise ValueError('EXEPACK command %#04x at %#x is neither a fill nor a copy'
                             % (cmd, si + 1))
        if cmd & 1:
            break
    if si != di:
        raise ValueError('EXEPACK stream ended with si %#x and di %#x, which should meet'
                         % (si, di))
    return bytes(out)


# ---------------------------------------------------------------- parsing

PAYLOAD = {0x05: 6, 0x0b: 3}


def _record(d, p):
    if p + 3 > len(d):
        return None
    reclen, kind = d[p], d[p + 1]
    if kind not in PAYLOAD:
        return None
    pl = PAYLOAD[kind]
    namelen = reclen - pl - 2
    if namelen < 1 or p + 1 + reclen > len(d) or d[p + 2 + pl] != namelen:
        return None
    name = d[p + 3 + pl:p + 1 + reclen]
    if not all(33 <= c < 127 for c in name):
        return None
    return reclen, kind, d[p + 2:p + 2 + pl], name.decode('latin1')


def find_blocks(d, min_records=8):
    """Locate the per-module symbol blocks by scanning for record runs."""
    blocks, p = [], 0
    while p < len(d):
        if _record(d, p):
            start, items = p, []
            while True:
                r = _record(d, p)
                if not r:
                    break
                items.append((p,) + r)
                p += 1 + r[0]
            if len(items) >= min_records:
                blocks.append((start, p, items))
            else:
                p = start + 1
        else:
            p += 1
    return blocks


def build(d):
    blocks = find_blocks(d)
    names = MODULES if len(blocks) == len(MODULES) else \
            ['module%d' % i for i in range(len(blocks))]
    out = []
    for bi, (start, end, items) in enumerate(blocks):
        for p, reclen, kind, payload, name in items:
            if kind == 0x0b:
                off, _ = struct.unpack('<HB', payload)
                out.append(dict(name=name, module=names[bi], kind='code',
                                seg=0, offset=off, rec=p))
            else:
                off, seg, typ = struct.unpack('<HHH', payload)
                out.append(dict(name=name, module=names[bi], kind='data',
                                seg=seg, offset=off, type=typ, rec=p))
    return blocks, names, out


def _branch_targets(d):
    """Every offset reached by a direct rel8/rel16 branch in the code region."""
    import collections
    br = collections.Counter()
    for i in range(min(CODE_END, len(d)) - 2):
        op = d[i]
        if op in (0xe8, 0xe9):
            t = i + 3 + struct.unpack('<h', d[i + 1:i + 3])[0]
        elif op == 0xeb or 0x70 <= op <= 0x7f or op in (0xe0, 0xe1, 0xe2, 0xe3):
            t = i + 2 + struct.unpack('<b', d[i + 1:i + 2])[0]
        else:
            continue
        if 0 <= t < CODE_END:
            br[t] += 1
    return br


def shift_map(d, syms, candidates=range(-600, 1), change_penalty=3.0):
    """Recover the code-offset correction.

    The symbol table's code offsets are NOT image offsets: they come from a
    coordinate space 473 bytes longer than the shipped code, and the difference
    accumulates in a handful of steps. Recover it as a monotone non-increasing
    piecewise-constant function, fitted by maximising the number of symbols that
    land on a real branch target (DP over the sorted symbols).
    """
    br = _branch_targets(d)
    code = sorted({s['offset'] for s in syms if s['kind'] == 'code'})
    cand = list(candidates)
    prev, paths = {0: (0.0, None)}, []
    for o in code:
        cur = {}
        for sh in cand:
            best, bp = None, None
            for psh, (sc, _) in prev.items():
                if psh < sh:
                    continue                      # monotone non-increasing
                v = sc - (change_penalty if psh != sh else 0.0)
                if best is None or v > best:
                    best, bp = v, psh
            if best is None:
                continue
            cur[sh] = (best + (1.0 if br.get(o + sh, 0) else 0.0), bp)
        paths.append(cur)
        prev = cur
    sh = max(prev.items(), key=lambda kv: kv[1][0])[0]
    seq = []
    for i in range(len(code) - 1, -1, -1):
        seq.append(sh)
        sh = paths[i][sh][1]
    seq.reverse()
    segs, st = [], 0
    for i in range(1, len(seq) + 1):
        if i == len(seq) or seq[i] != seq[i - 1]:
            segs.append((code[st], code[i - 1], seq[st]))
            st = i
    return segs, br


def apply_shift(segs, off):
    for lo, hi, sh in segs:
        if off <= hi:
            return sh
    return segs[-1][2]


# ---------------------------------------------------------------- evidence

def annotate(d, syms, segs, br):
    """Resolve addresses and attach the corroboration that justifies each one."""
    import collections
    words = collections.Counter()
    for i in range(min(CODE_END, len(d)) - 1):
        words[d[i] | (d[i + 1] << 8)] += 1
    for s in syms:
        if s['kind'] == 'code' or s['seg'] == 0:
            # code segment: offsets need the recovered correction
            s['shift'] = apply_shift(segs, s['offset'])
            s['addr'] = s['offset'] + s['shift']
            s['refs'] = br.get(s['addr'], 0) if s['kind'] == 'code' else words.get(s['offset'], 0)
        else:
            s['shift'] = 0
            s['addr'] = s['seg'] * 16 + s['offset']
            s['refs'] = words.get(s['offset'], 0)
        blob = d[s['addr']:s['addr'] + 40].split(b'\x00')[0]
        if 3 <= len(blob) <= 38 and all(32 <= c < 127 for c in blob):
            s['string'] = blob.decode('latin1')
        s['confirmed'] = s['refs'] > 0
    return syms


def main():
    exe = sys.argv[1] if len(sys.argv) > 1 else 'MAIN.EXE'
    out = sys.argv[2] if len(sys.argv) > 2 else 'symbols.json'
    img = unpack(exe)
    if '--image' in sys.argv:
        open(sys.argv[sys.argv.index('--image') + 1], 'wb').write(img)
    blocks, names, syms = build(img)
    segs, br = shift_map(img, syms)
    annotate(img, syms, segs, br)
    print('image %d bytes, %d blocks, %d symbols' % (len(img), len(blocks), len(syms)))
    for (a, b, items), n in zip(blocks, names):
        code = [struct.unpack('<HB', r[3])[0] for r in items if r[2] == 0x0b]
        print('  %-9s %4d symbols  code %04x-%04x' %
              (n, len(items), min(code), max(code)) if code else '  %-9s %4d symbols' % (n, len(items)))
    ok = sum(1 for s in syms if s['confirmed'])
    print('code-offset shift map:')
    for lo, hi, sh in segs:
        print('   sym %04x-%04x  %+d' % (lo, hi, sh))
    print('%d/%d symbols independently corroborated' % (ok, len(syms)))
    json.dump(dict(image_bytes=len(img), dgroup_seg=DGROUP, modules=names,
                   code_shift_map=[list(x) for x in segs],
                   symbols=sorted(syms, key=lambda s: (s['kind'], s['addr']))),
              open(out, 'w'), indent=1)
    print('wrote', out)


if __name__ == '__main__':
    main()
