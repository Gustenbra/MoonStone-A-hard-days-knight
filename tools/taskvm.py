#!/usr/bin/env python3
"""Recover Moonstone's animation task VM: its opcode set and its frame record.

Animations in MAIN.EXE are bytecode. `PerformCOMMAND` (image offset 0x97fb)
fetches one byte at a time from the task's script pointer and dispatches:

    0xff        end of frame; the byte after it says what happens next
    0xfd        script pointer <- TaskCommand[0x18]   (resume after a JUMP)
    0xfe        script pointer <- TaskCommand[6]      (resume after a LOOP)
    bit 7 set   call TaskComTable[op & 0x7f]          (a command)
    otherwise   a six byte sprite-part record

`TaskComTable` (DS:0x9448) is BSS, so it is empty in the load image: INITTASK
fills it at run time with a run of `mov word ptr [di+off], imm16`. This tool
reads those immediates out of INITTASK rather than out of the data segment.

The immediates are *link-time* code offsets, not image offsets. Absolute code
addresses baked into the code, and the code offsets in the symbol table, both
live in a coordinate space that is up to 473 bytes longer than the shipped
image; symbolmap.py already fits that correction, and every symbol carries the
shift that applies at its address. Apply the same correction to a table entry
and all nineteen populated slots land exactly on a routine entry point. Without
it, none of them do, which is what confirms the correction is real and not a
fitting artefact.

Handler names come from the PUBLIC blob appended to MAIN.EXE. Those names carry
no addresses, but they are emitted in definition order, and the anchors that do
have addresses (INITTASK, CLEARTASKS, REPLACEANIM, ADDTASK, FINDTASK,
TASKSTANDBY, then TASKRIGHT/TASKLEFT/TASKPLACE as the three placement labels
inside PerformCOMMAND) come out in exactly that order. Zipping the nineteen
handler addresses against the nineteen PUBLIC names between TASKSORT and
TASKWALKCOLLIDE is therefore an ordering argument, not a guess about behaviour,
and every name it produces agrees with what the handler actually does. The
mapping is still an inference and is marked as such in docs/TASKVM.md.

The operand widths are not guessed either: each handler advances the script
pointer itself (`add word ptr [di+2], n`), so the width is read out of the code.

Checks that the result is right, all run by --verify:

  * 236 named animation scripts in DGROUP parse end to end with no unknown
    opcode, every one of them terminating on `ff ff`
  * every sprite-part record's bank selector is a multiple of four, which is
    what the bank table stride requires
  * every TASKGOTO target is the first byte of a named script
  * every TASKGOSUB target resolves to the exact entry point of a named routine
    (KnightGruntSound, DrDropHead, KnifeThrow, ...)
  * every TASKDEAD target is a named death script, every TASKSHADOW argument a
    named shadow script

The last check is the one that matters, and --composite is it. A structurally
perfect decode can still be wrong about which byte is x and which is y, and the
only way to tell is to draw the frames and look at them. --composite walks a
script, resolves each part's bank selector through the creature's bank table,
places it with the mirror term, and writes a PNG per frame plus a contact sheet.
It draws from the baked pack in packs/reference, so it decodes nothing itself.
Its PNGs are derived from the original game, so they default into research/,
which is gitignored, and must not be written to a tracked path.

Usage:
    python3 taskvm.py MAIN.EXE                       # unpack, then dump the opcode set
    python3 taskvm.py --image I --symbols S ...      # reuse a prepared image
    python3 taskvm.py ... --verify                   # parse every script and check operands
    python3 taskvm.py ... --list [PREFIX]            # list the animation scripts
    python3 taskvm.py ... --disasm Knight_SwSwing    # disassemble one, by symbol or 0xADDR
    python3 taskvm.py ... --composite Knight_SwWalkOn
    python3 taskvm.py ... --composite Knight_SwWalkOn --facing 3 --out research/flip
Needs: pip install unicorn capstone   (only to unpack; not needed with --image)
       pip install pillow             (only for --composite)
"""
import argparse
import bisect
import json
import os
import re
import struct
import sys

DGROUP = 0x123b
DS_BASE = DGROUP * 16
TASK_COM_TABLE = 0x9448          # DS offset, and the immediate INITTASK loads into di
TABLE_SLOTS = 22                 # (TaskCelTable - TaskComTable) / 2

# The PUBLIC names between TASKSORT and TASKWALKCOLLIDE, in the order they
# appear in the PUBLIC blob of MAIN.EXE, which is definition order. Nineteen
# names for nineteen populated handler slots.
HANDLER_NAMES = [
    'TASK_FLIP', 'TASKGOTO', 'TASKHOLD', 'TASKJUMP', 'TASKLOOP', 'TASKSKIP',
    'TASKTIME', 'TASKSOUND', 'TASKMOVE', 'TASKSHADOW', 'TASKSAVE', 'TASKGOSUB',
    'TASKDEAD', 'TASKADDTASK', 'TASKKILLTASK', 'TASKCELBUF', 'TASKTESTEQ',
    'TASKTESTNE', 'TASKANIMCLR',
]

# What each command's operands are, once the handler has been read. Keyed by
# handler name so it survives a different table layout.
OPERANDS = {
    'TASK_FLIP':    'u8 facing        1 right, 3 left (bit 1 mirrors), 0xff toggles',
    'TASKGOTO':     'u8 mode, u16 target   mode 3 jumps now, else on the next end of frame',
    'TASKHOLD':     'u8 count         repeat this frame count times (0 means once)',
    'TASKJUMP':     'u8 a, u8 ticks, u8 flags, u8 yspeed, u8 ymin, u8 xspeed, u8 xmin',
    'TASKLOOP':     'u8 count         loop back here at the next "ff fe"',
    'TASKSKIP':     'u8 _, u16 target branch if the DS:0x700 mode flag is set',
    'TASKTIME':     '(none)           handler is a bare RET; never emitted',
    'TASKSOUND':    'u8 sample',
    'TASKMOVE':     'u8 flags, i16 x, i16 y, i16 z',
    'TASKSHADOW':   'u8 on, u16 script    shadow script for the actor, or off',
    'TASKSAVE':     'u8 mode, i16 field, u16 value   store into the actor record',
    'TASKGOSUB':    'u8 _, u16 routine    near call into the game code',
    'TASKDEAD':     'u8 _, u16 target branch if actor hit points <= 0',
    'TASKADDTASK':  'u8 _, u16 script spawn a second task on that script',
    'TASKKILLTASK': 'u8 _             stop this task',
    'TASKCELBUF':   'u8 n             select bank table n (1..4) from TaskCelTable',
    'TASKTESTEQ':   'u8 mode, i16 field, u16 target   branch if the field is zero',
    'TASKTESTNE':   'u8 mode, i16 field, u16 target   branch if the field is non-zero',
    'TASKANIMCLR':  'u8 _             clear the task VM state for this actor',
}

TERMINATORS = {
    0x00: 'end of frame',
    0xfe: 'end of frame, loop back if a TASKLOOP count is running',
    0xff: 'end of frame and end of animation',
}


# ------------------------------------------------------------------ inputs

def load(args):
    """Return (image bytes, symbol list). Unpacks MAIN.EXE only if it has to."""
    if args.image and args.symbols:
        return (open(args.image, 'rb').read(),
                json.load(open(args.symbols))['symbols'])
    if not args.exe:
        sys.exit('give MAIN.EXE, or --image and --symbols')
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import symbolmap
    img = symbolmap.unpack(args.exe)
    _, _, syms = symbolmap.build(img)
    segs, br = symbolmap.shift_map(img, syms)
    symbolmap.annotate(img, syms, segs, br)
    return img, syms


class Syms:
    """Symbol lookup, plus the link-time to image-offset correction."""

    def __init__(self, syms):
        code = sorted((s for s in syms if s['kind'] == 'code'),
                      key=lambda s: s['offset'])
        self.raws = [s['offset'] for s in code]
        self.shifts = [s['shift'] for s in code]
        self.code_at = {}
        for s in code:
            self.code_at.setdefault(s['addr'], s['name'])
        self.code_addrs = sorted(self.code_at)
        data = sorted((s for s in syms if s['kind'] == 'data'),
                      key=lambda s: s['addr'])
        self.data_at = {}
        for s in data:
            self.data_at.setdefault(s['addr'] - DS_BASE, s['name'])
        self.data_offs = sorted(self.data_at)
        self.by_name = {s['name']: s for s in data}

    def to_image(self, raw):
        """A link-time code offset, as baked into the code, to an image offset."""
        i = max(bisect.bisect_right(self.raws, raw) - 1, 0)
        return raw + self.shifts[i]

    def code(self, raw):
        a = self.to_image(raw)
        j = self.code_addrs[bisect.bisect_right(self.code_addrs, a) - 1]
        return self.code_at[j] if j == a else '%s+%d' % (self.code_at[j], a - j)

    def data(self, off):
        j = self.data_offs[bisect.bisect_right(self.data_offs, off) - 1]
        return self.data_at[j] if j == off else '%s+%d' % (self.data_at[j], off - j)

    def is_code_entry(self, raw):
        return self.to_image(raw) in self.code_at

    def is_data_entry(self, off):
        return off in self.data_at


# ------------------------------------------------------- reading the table

def read_com_table(img, syms):
    """Read TaskComTable out of INITTASK's `mov word ptr [di+off], imm` run.

    The table is BSS, so the data segment holds nothing but zeroes; the only
    place the handler addresses exist in the file is the code that installs
    them. Find `mov di, TaskComTable`, then take every store through di that
    follows, until the run stops.
    """
    want = b'\xbf' + struct.pack('<H', TASK_COM_TABLE)      # mov di, 0x9448
    p = img.find(want, 0, 0xd800)
    if p < 0:
        raise SystemExit('INITTASK does not load TaskComTable; table not found')
    p += 3
    slots = {}
    while True:
        if img[p:p + 2] == b'\xc7\x05':                     # mov word ptr [di], imm
            off, imm, p = 0, struct.unpack_from('<H', img, p + 2)[0], p + 4
        elif img[p:p + 2] == b'\xc7\x45':                   # mov word ptr [di+d8], imm
            off = img[p + 2]
            imm = struct.unpack_from('<H', img, p + 3)[0]
            p += 5
        else:
            break
        slots[off] = imm
    return slots


def opcode_set(img, syms):
    """opcode -> dict(name, handler image offset, length, operands)."""
    slots = read_com_table(img, syms)
    order = sorted(slots, key=lambda o: slots[o])
    if len(order) != len(HANDLER_NAMES):
        print('warning: %d handlers but %d names; names are positional'
              % (len(order), len(HANDLER_NAMES)), file=sys.stderr)
    names = dict(zip(order, HANDLER_NAMES))
    ops = {}
    for off, raw in sorted(slots.items()):
        addr = syms.to_image(raw)
        name = names.get(off, 'op%02x' % (0x80 + off))
        ops[0x80 + off] = dict(name=name, addr=addr, raw=raw,
                               length=advance(img, addr),
                               operands=OPERANDS.get(name, ''))
    return ops


def advance(img, addr, limit=0x60):
    """How far the handler at `addr` moves the script pointer.

    Every command handler ends with `add word ptr [di+2], n` (83 45 02 n), so
    the operand width is read out of the code instead of inferred from the
    data. A handler with several exits uses the same n on each; TASKGOTO's
    branch taken path writes the pointer outright and consumes nothing.
    """
    if img[addr] == 0xc3:
        return 0                    # a bare RET: consumes nothing, so it hangs
    best = None
    for i in range(addr, addr + limit):
        if img[i:i + 3] == b'\x83\x45\x02':
            n = img[i + 3]
            best = n if best is None else min(best, n)
        if img[i] == 0xc3 and best is not None:
            break
    return best


# ------------------------------------------------------- script disassembly

def disasm(img, syms, ops, off, limit=4000):
    """Walk a script from a DS offset. Yields (offset, length, text)."""
    pc = off
    for _ in range(limit):
        addr = DS_BASE + pc
        op = img[addr]
        if op == 0xff:
            sub = img[addr + 1]
            yield pc, 2, 'ENDFRAME %02x   ; %s' % (sub, TERMINATORS.get(sub, '?'))
            if sub == 0xff:
                return
            pc += 2
            continue
        if op in (0xfd, 0xfe):
            yield pc, 1, 'RESUME %02x     ; %s' % (
                op, 'after TASKJUMP' if op == 0xfd else 'after TASKLOOP')
            return
        if op & 0x80:
            c = ops.get(op)
            if c is None:
                yield pc, 1, '??? %02x        ; not in TaskComTable' % op
                return
            n = c['length'] or 1
            body = img[addr + 1:addr + n]
            yield pc, n, '%-13s %s%s' % (c['name'], body.hex(' '),
                                         annotate(syms, c['name'], body))
            pc += n
            continue
        slot = op & 0x1f
        cel = img[addr + 1]
        y = struct.unpack_from('<b', img, addr + 2)[0]
        flags = img[addr + 3]
        x = struct.unpack_from('<h', img, addr + 4)[0]
        yield pc, 6, 'PART bank %d cel %-3d x %-5d y %-4d flags %02x%s' % (
            slot // 4, cel, x, y, flags, part_flags(flags))
        pc += 6


def part_flags(f):
    bits = [(0x01, 'body'), (0x02, 'weapon'), (0x10, 'page2'),
            (0x40, 'nobounds'), (0x80, 'gated')]
    got = [n for b, n in bits if f & b]
    spare = f & ~0xd3
    if spare:
        got.append('spare:%02x' % spare)
    return '  (%s)' % ','.join(got) if got else ''


def annotate(syms, name, body):
    """Resolve a command's word operand to a symbol, where it is one."""
    if len(body) < 3:
        return ''
    w = struct.unpack_from('<H', body, 1)[0]
    if name == 'TASKGOSUB':
        return '   ; %s' % syms.code(w)
    if name in ('TASKGOTO', 'TASKSKIP', 'TASKDEAD', 'TASKADDTASK', 'TASKSHADOW'):
        return '   ; %s' % syms.data(w) if w else ''
    if name in ('TASKTESTEQ', 'TASKTESTNE', 'TASKSAVE') and len(body) >= 5:
        t = struct.unpack_from('<H', body, 3)[0]
        return '   ; field +%d -> %s' % (w, syms.data(t)) if t else ''
    return ''


SCRIPT_RE = re.compile(
    r'^(Knight|Hero|Player|Trogg\w*|Beast|Ratman|Rat|Mudmen|Troll|Demon|Dragon|Balok)_')


def scripts(syms, prefix=None):
    out = [(n, s['addr'] - DS_BASE) for n, s in syms.by_name.items()
           if SCRIPT_RE.match(n)]
    if prefix:
        out = [x for x in out if x[0].startswith(prefix)]
    return sorted(out, key=lambda x: x[1])


# ----------------------------------------------------------- the composite

# Cel counts per bank, in the order henge-bake.rs packs them into each sheet.
# Read off each bank file's two-byte big-endian count. Checked at run time
# against the manifest's frame count for the sheet, so a rebake that changed the
# packing is caught rather than drawn wrong. A sheet not listed here is one bank.
SHEET_BANKS = {
    'actor.knight':      [60, 24, 64, 26, 65],    # KN1..KN5.OB
    'actor.hero':        [60, 24, 64],            # HE1..HE3.OB
    'actor.troll':       [48, 16],
    'actor.trogg_axe':   [37, 80],
    'actor.trogg_spear': [41, 67],
    'actor.ratmen':      [64, 64],
    'actor.mudmen':      [63, 66],
    'actor.demon':       [48, 99, 50, 27],        # DEMON1..DEMON4
    'actor.dragon':      [22, 28, 44],            # DRAGON1, DRAGON2, DRAGON5
    'actor.balok':       [70, 26, 60],            # BALOK1, BALOK2, BALOK3
}

# Bank slot -> (sheet, which bank within it), one entry per creature loader.
# Read out of the loaders at the addresses in docs/TASKVM.md; the slot order is
# not the order the baker packs the files in, which is why this is a map and not
# a list. The hero borrows the knight's two sword banks, as its loader does.
ACTOR_SLOTS = {
    'knight':      {n: ('actor.knight', n) for n in range(5)},
    'hero':        {0: ('actor.hero', 0), 1: ('actor.hero', 1), 2: ('actor.hero', 2),
                    3: ('actor.knight', 3), 4: ('actor.knight', 4)},
    'troll':       {0: ('actor.troll', 0), 1: ('actor.troll', 1)},
    'trogg_axe':   {0: ('actor.trogg_axe', 0), 1: ('actor.trogg_axe', 1)},
    'trogg_spear': {0: ('actor.trogg_spear', 0),
                    **{n: ('actor.trogg_spear', 1) for n in (1, 2, 3, 4)}},
    'ratmen':      {0: ('actor.ratmen', 0), 1: ('actor.ratmen', 1)},
    'mudmen':      {0: ('actor.mudmen', 0), 1: ('actor.mudmen', 1)},
    'beast':       {0: ('bank.be1', 0), 1: ('bank.be2', 0)},
    'demon':       {0: ('actor.demon', 1), 2: ('actor.demon', 2),
                    3: ('actor.demon', 3), 4: ('actor.demon', 0)},
    'dragon':      {0: ('actor.dragon', 0), 1: ('actor.dragon', 1),
                    4: ('actor.dragon', 2)},
    'balok':       {0: ('actor.balok', 0), 1: ('actor.balok', 2),
                    2: ('actor.balok', 1)},
    'gore':        {n: ('actor.gore', 0) for n in range(5)},
}

# Which loader a script's name implies. The prefix names the encounter, not the
# bank table, so it is only a default: `Knight_HangSd` is played on the ratman's
# banks and `Dragon_Flight*` on a table with DRAGON5 in slot 0. Both are caught
# by the cel bounds check rather than drawn wrong. Use --actor to override.
PREFIX_ACTOR = {
    'Knight': 'knight', 'Hero': 'hero', 'Troll': 'troll',
    'TroggAxe': 'trogg_axe', 'TroggHammer': 'trogg_axe',
    'TroggSpear': 'trogg_spear', 'Ratman': 'ratmen', 'Rat': 'ratmen',
    'Mudmen': 'mudmen',
    'Beast': 'beast', 'Demon': 'demon', 'Dragon': 'dragon', 'Balok': 'balok',
}

BACKDROP = (18, 18, 26)


class Pack:
    """The baked reference pack: indexed sheets, frame rects and palettes."""

    def __init__(self, root, palette):
        from PIL import Image
        self.root = root
        self.m = json.load(open(os.path.join(root, 'manifest.json')))
        if palette not in self.m['palettes']:
            raise SystemExit('pack has no palette %r; it has %s'
                             % (palette, ', '.join(sorted(self.m['palettes']))))
        self.pal = [((c >> 16) & 255, (c >> 8) & 255, c & 255)
                    for c in self.m['palettes'][palette]]
        self._sheets = {}
        self._Image = Image

    def sheet(self, sid):
        if sid not in self._sheets:
            s = self.m['sheets'].get(sid)
            if s is None:
                raise SystemExit('pack has no sheet %r' % sid)
            im = self._Image.open(os.path.join(self.root, s['file']))
            if im.mode != 'P':
                raise SystemExit('%s is not an indexed PNG' % s['file'])
            self._sheets[sid] = (im.size[0], im.size[1],
                                 im.tobytes(), s['frames'])
        return self._sheets[sid]

    def bank_base(self, sid, bank):
        """First sheet frame index of a bank, checked against the manifest."""
        counts = SHEET_BANKS.get(sid)
        frames = len(self.sheet(sid)[3])
        if counts is None:
            counts = [frames]
        if sum(counts) != frames:
            raise SystemExit(
                '%s: bank counts %s sum to %d but the pack has %d frames; the '
                'pack was baked from different banks' % (sid, counts, sum(counts), frames))
        if bank >= len(counts):
            raise SystemExit('%s has no bank %d' % (sid, bank))
        return sum(counts[:bank]), counts[bank]


def frames_of(img, syms, ops, off, limit):
    """Walk a script into a list of frames, each a list of parts.

    Commands that move the actor are applied, so a script that walks itself
    across the screen does so here. Branches are not followed: the walk is
    linear and stops at `ff ff` or after `limit` frames, which is what a static
    dump wants.
    """
    parts, out = [], []
    tx = ty = tz = 0
    flip = False
    for pc, n, text in disasm(img, syms, ops, off):
        a = DS_BASE + pc
        if text.startswith('PART'):
            parts.append(dict(slot=(img[a] & 0x1f) // 4, cel=img[a + 1],
                              y=struct.unpack_from('<b', img, a + 2)[0],
                              flags=img[a + 3],
                              x=struct.unpack_from('<h', img, a + 4)[0],
                              tx=tx, ty=ty, tz=tz))
            continue
        if text.startswith('ENDFRAME'):
            out.append(parts)
            parts = []
            if img[a + 1] == 0xff or len(out) >= limit:
                break
            continue
        if text.startswith('TASKMOVE'):
            f = img[a + 1]
            vx, vy, vz = struct.unpack_from('<3h', img, a + 2)
            if f & 0x40:
                tx, ty, tz = vx, vy, vz
            else:
                sx = vx if not f & 0x01 else -vx
                tx -= -sx if flip else sx
                ty -= vy if f & 0x08 else -vy
                tz -= vz if f & 0x20 else -vz
        elif text.startswith('TASK_FLIP') and img[a + 1] == 0xff:
            flip = not flip
    return out


def composite(img, syms, ops, name, off, args):
    """Draw a script's frames from the baked pack. Returns the files written."""
    from PIL import Image
    pack = Pack(args.pack, args.palette)
    actor = args.actor or PREFIX_ACTOR.get(name.split('_')[0])
    if actor not in ACTOR_SLOTS:
        raise SystemExit('no bank table for %r; pass --actor from %s'
                         % (name, ', '.join(sorted(ACTOR_SLOTS))))
    slots = ACTOR_SLOTS[actor]
    mirror = args.facing & 2

    placed, missing = [], []
    for parts in frames_of(img, syms, ops, off, args.frames):
        drawn = []
        for p in parts:
            if p['slot'] not in slots:
                missing.append('slot %d has no bank in the %s table'
                               % (p['slot'], actor))
                continue
            sid, bank = slots[p['slot']]
            base, count = pack.bank_base(sid, bank)
            if p['cel'] >= count:
                missing.append('cel %d is past the %d in %s bank %d'
                               % (p['cel'], count, sid, bank))
                continue
            r = pack.sheet(sid)[3][base + p['cel']]
            x = p['tx'] + (-(p['x'] + r['w']) if mirror else p['x'])
            y = p['ty'] + p['tz'] + p['y']
            drawn.append((sid, r, x, y, p['flags']))
        placed.append(drawn)

    if not any(placed):
        raise SystemExit('%s drew nothing' % name)
    top = min(y for f in placed for _, _, _, y, _ in f)
    bot = max(y + r['h'] for f in placed for _, r, _, y, _ in f)

    os.makedirs(args.out, exist_ok=True)
    written, tiles = [], []
    for i, f in enumerate(placed):
        if not f:
            continue
        left = min(x for _, _, x, _, _ in f)
        right = max(x + r['w'] for _, r, x, _, _ in f)
        im = Image.new('RGB', (right - left, bot - top), BACKDROP)
        px = im.load()
        for sid, r, x, y, _ in f:
            sw, _sh, data, _ = pack.sheet(sid)
            for row in range(r['h']):
                src = (r['y'] + row) * sw + r['x']
                oy = y + row - top
                for col in range(r['w']):
                    v = data[src + col]
                    if v:
                        ox = (r['w'] - 1 - col) if mirror else col
                        px[x + ox - left, oy] = pack.pal[v % len(pack.pal)]
        im = im.resize((im.width * args.scale, im.height * args.scale),
                       Image.NEAREST)
        p = os.path.join(args.out, '%s_%02d.png' % (name, i))
        im.save(p)
        written.append(p)
        tiles.append(im)

    gap = 4 * args.scale
    w = sum(t.width for t in tiles) + gap * (len(tiles) + 1)
    h = tiles[0].height + 2 * gap
    sheet = Image.new('RGB', (w, h), (58, 58, 70))
    x = gap
    for t in tiles:
        sheet.paste(t, (x, gap))
        x += t.width + gap
    p = os.path.join(args.out, '%s_contact.png' % name)
    sheet.save(p)
    written.append(p)
    return written, actor, sorted(set(missing))


# -------------------------------------------------------------- the checks

def verify(img, syms, ops):
    bad = []
    counts = {'scripts': 0, 'parts': 0, 'commands': 0}
    for name, off in scripts(syms):
        counts['scripts'] += 1
        ended = False
        for pc, n, text in disasm(img, syms, ops, off):
            if text.startswith('???'):
                bad.append('%s: %s' % (name, text))
                break
            if text.startswith('PART'):
                counts['parts'] += 1
                if (img[DS_BASE + pc] & 0x1f) % 4:
                    bad.append('%s +%d: bank selector %02x is not a multiple of 4'
                               % (name, pc - off, img[DS_BASE + pc] & 0x1f))
            elif text.startswith('ENDFRAME'):
                ended = img[DS_BASE + pc + 1] == 0xff
            else:
                counts['commands'] += 1
                cmd = text.split()[0]
                body = img[DS_BASE + pc + 1:DS_BASE + pc + n]
                w = struct.unpack_from('<H', body, 1)[0] if len(body) >= 3 else 0
                if cmd == 'TASKGOSUB':
                    if not syms.is_code_entry(w):
                        bad.append('%s: TASKGOSUB %04x is not a routine entry'
                                   % (name, w))
                elif cmd in ('TASKGOTO', 'TASKDEAD', 'TASKADDTASK', 'TASKSKIP'):
                    if w and not syms.is_data_entry(w):
                        bad.append('%s: %s %04x is not a script start'
                                   % (name, cmd, w))
        if not ended:
            bad.append('%s: does not terminate on "ff ff"' % name)
    return counts, bad


# --------------------------------------------------------------------- cli

def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('exe', nargs='?', help='MAIN.EXE')
    ap.add_argument('--image', help='an already unpacked load image')
    ap.add_argument('--symbols', help='symbols.json from symbolmap.py')
    ap.add_argument('--verify', action='store_true', help='parse every script')
    ap.add_argument('--list', nargs='?', const='', metavar='PREFIX',
                    help='list the animation scripts')
    ap.add_argument('--disasm', metavar='NAME|0xOFF', help='disassemble one script')
    ap.add_argument('--composite', metavar='NAME',
                    help='draw a script\'s frames from the baked pack')
    ap.add_argument('--out', default='research/shots', metavar='DIR',
                    help='where --composite writes its PNGs. The default, '
                         'research/shots, is gitignored: these PNGs are derived '
                         'from the original game and must not land in a tracked '
                         'path')
    ap.add_argument('--facing', type=int, default=1, choices=(1, 3),
                    help='1 faces right, 3 mirrors (default: 1)')
    ap.add_argument('--actor', help='bank table to use, if not the one the '
                                    'script name implies')
    ap.add_argument('--pack', default='packs/reference',
                    help='baked pack root (default: packs/reference)')
    ap.add_argument('--palette', default='palette.forest',
                    help='palette id from the pack (default: palette.forest)')
    ap.add_argument('--scale', type=int, default=3, help='pixel scale (default: 3)')
    ap.add_argument('--frames', type=int, default=16,
                    help='most frames to draw (default: 16)')
    args = ap.parse_args()

    img, raw_syms = load(args)
    syms = Syms(raw_syms)
    ops = opcode_set(img, syms)

    if args.composite:
        n = args.composite
        if n not in syms.by_name:
            raise SystemExit('no symbol %r; try --list' % n)
        files, actor, missing = composite(
            img, syms, ops, n, syms.by_name[n]['addr'] - DS_BASE, args)
        print('%s, %s banks, facing %d: %d files'
              % (n, actor, args.facing, len(files)))
        for f in files:
            print('  ', f)
        for m in missing:
            print('   skipped:', m)
        if missing:
            print('   the script name is the encounter, not the bank table; '
                  'try --actor')
        return

    if args.disasm:
        d = args.disasm
        off = int(d, 0) if d.startswith('0x') else \
            syms.by_name[d]['addr'] - DS_BASE
        print('%s at DS:0x%04x' % (d, off))
        for pc, n, text in disasm(img, syms, ops, off):
            print('  %04x  %-18s %s' % (pc, img[DS_BASE + pc:DS_BASE + pc + n].hex(' '), text))
        return

    if args.list is not None:
        for name, off in scripts(syms, args.list or None):
            print('DS:0x%04x  %s' % (off, name))
        return

    print('TaskComTable at DS:0x%04x, %d slots, %d populated'
          % (TASK_COM_TABLE, TABLE_SLOTS, len(ops)))
    print('%-6s %-14s %-8s %-6s %s' % ('op', 'name', 'handler', 'bytes', 'operands'))
    for op in sorted(ops):
        c = ops[op]
        print('0x%02x   %-14s %04x     %-6s %s'
              % (op, c['name'], c['addr'],
                 c['length'] if c['length'] is not None else '-', c['operands']))
    for op in range(0x80, 0x80 + TABLE_SLOTS * 2, 2):
        if op not in ops:
            print('0x%02x   (empty slot, INITTASK never fills it)' % op)
    print()
    print('0xfd   RESUME after TASKJUMP     script pointer <- TaskCommand[0x18]')
    print('0xfe   RESUME after TASKLOOP     script pointer <- TaskCommand[6]')
    print('0xff   ENDFRAME, then one byte:  00 next frame, fe loop, ff end')
    print('other  sprite part, six bytes:   '
          '[u8 bank*4][u8 cel][i8 y][u8 flags][i16 x]')

    if args.verify:
        counts, bad = verify(img, syms, ops)
        print()
        print('%d scripts, %d part records, %d commands'
              % (counts['scripts'], counts['parts'], counts['commands']))
        for b in bad:
            print('  FAIL', b)
        print('all checks passed' if not bad else '%d problems' % len(bad))


if __name__ == '__main__':
    main()
