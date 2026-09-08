"""Lift the note data out of Moonstone's music by running the game's own driver.

The tune files are not a format. Each `xTUNEn.BIN` is a **relocatable x86
driver blob with the song welded into it**: the game loads one at a fixed
segment, points `int 60h` at offset zero, and its timer interrupt calls it
every tick. `ah = 0` starts, `ah = 1` is one tick, `ah = 2` stops.

There are three of each tune, one per sound card, and the letter says which:

    a...  AdLib. `out 0x388, reg` then `out 0x389, value`, with the usual
          register-select delay of six dummy reads between them.
    b...  the PC speaker. Ports 0x43, 0x42 and 0x61: timer 2 square waves.
    r...  Roland, through an MPU-401 at 0x330 and 0x331.

**The Roland one is plain MIDI.** So rather than reverse the song format, this
runs the driver under emulation and writes down what it sends: the same thing a
MIDI cable would have carried in 1992. `MusicTable` in `MAIN.EXE` is eighteen
records, six tunes by three cards, and the tune numbers here are its own.

    python3 tools/tunes.py "path/to/Moonstone" research/tunes.json

Needs `unicorn`. It looks for `RTUNE1.BIN` to `RTUNE6.BIN` in the game folder
and in its `DISKA` and `DISKB` subfolders, which is where they are on a real
installation: 1 and 6 shipped on disk A with the intro, 2 to 5 on disk B.

The output is note events on the driver's own tick, which is the game's timer:
1193182 / 0x5555, or 54.62 Hz. `Install_Timer` programs that divisor and the
handler's first instructions are `mov ah, 1; int 60h`, so one tick of a tune is
one tick of that timer and nothing else.
"""
import json
import os
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_16, UC_HOOK_INSN, UC_HOOK_INTR, UcError
from unicorn.x86_const import (
    UC_X86_INS_IN, UC_X86_INS_OUT, UC_X86_REG_AX, UC_X86_REG_CS, UC_X86_REG_DS,
    UC_X86_REG_ES, UC_X86_REG_IP, UC_X86_REG_SP, UC_X86_REG_SS,
)

SEG = 0x2000            # where the driver is loaded; it sets ds = cs itself
STACK = 0x1000
RETSEG, RETOFF = 0x0f00, 0x0000

# `Install_Timer`: mode 3, divisor 0x5555.
TICK_HZ_NUM = 1193182
TICK_HZ_DEN = 0x5555

MPU_DATA, MPU_STATUS = 0x330, 0x331

# How long to run each tune before giving up on finding its loop. 20000 ticks
# is about six minutes, and the longest of the six comes round twice inside it.
TICKS = 20000

MIDI_ARGS = {0x8: 2, 0x9: 2, 0xa: 2, 0xb: 2, 0xc: 1, 0xd: 1, 0xe: 2}


class Driver:
    """One tune blob, running."""

    def __init__(self, path):
        self.blob = open(path, 'rb').read()
        self.writes = []
        self.tick = 0
        uc = Uc(UC_ARCH_X86, UC_MODE_16)
        uc.mem_map(0, 0x110000)
        uc.mem_write(SEG * 16, self.blob)
        uc.mem_write(RETSEG * 16, b'\xf4')          # hlt, where the iret lands
        uc.hook_add(UC_HOOK_INSN, self._out, None, 1, 0, UC_X86_INS_OUT)
        uc.hook_add(UC_HOOK_INSN, self._in, None, 1, 0, UC_X86_INS_IN)
        uc.hook_add(UC_HOOK_INTR, lambda *a: None)   # a debug int 21h path
        self.uc = uc

    def _out(self, uc, port, size, value, user):
        self.writes.append((self.tick, port, value & 0xff))

    def _in(self, uc, port, size, user):
        # An MPU-401 that is always ready and always acknowledges. Bit 6 of the
        # status port is "cannot take data" and bit 7 is "nothing to read"; the
        # driver spins on both, so both are answered clear.
        if port == MPU_STATUS:
            return 0x00
        if port == MPU_DATA:
            return 0xfe                              # ACK
        return 0x00

    def call(self, ah, limit=8_000_000):
        uc = self.uc
        uc.reg_write(UC_X86_REG_SS, STACK)
        sp = 0xff00
        for w in (0x0202, RETSEG, RETOFF):           # flags, cs, ip for the iret
            sp -= 2
            uc.mem_write(STACK * 16 + sp, struct.pack('<H', w))
        uc.reg_write(UC_X86_REG_SP, sp)
        uc.reg_write(UC_X86_REG_CS, SEG)
        uc.reg_write(UC_X86_REG_DS, SEG)
        uc.reg_write(UC_X86_REG_ES, SEG)
        uc.reg_write(UC_X86_REG_AX, ah << 8)
        try:
            uc.emu_start(SEG * 16, RETSEG * 16, count=limit)
        except UcError as e:
            raise SystemExit(f'driver faulted at ah={ah}: {e}')

    def run(self, ticks):
        self.call(0)
        for t in range(1, ticks + 1):
            self.tick = t
            self.call(1)
        return self.writes


def messages(writes):
    """The MPU data port's bytes, parsed as MIDI with running status."""
    data = [(t, v) for t, p, v in writes if p == MPU_DATA]
    out, i, status = [], 0, None
    while i < len(data):
        t, b = data[i]
        if b & 0x80:
            status = b
            i += 1
            if b >= 0xf0:                            # system messages: not ours
                continue
        if status is None:
            i += 1
            continue
        n = MIDI_ARGS[status >> 4]
        if i + n > len(data):
            break
        out.append((t, status, [data[i + k][1] for k in range(n)]))
        i += n
    return out


def notes(msgs):
    """Note on and note off paired into (start, duration, channel, note, velocity)."""
    open_notes, out, programs = {}, [], []
    for t, status, args in msgs:
        kind, chan = status >> 4, status & 0xf
        if kind == 0xc:
            programs.append([t, chan, args[0]])
        elif kind == 0x9 and args[1] > 0:
            open_notes.setdefault((chan, args[0]), []).append((t, args[1]))
        elif kind == 0x8 or (kind == 0x9 and args[1] == 0):
            stack = open_notes.get((chan, args[0]))
            if stack:
                start, vel = stack.pop(0)
                out.append([start, max(1, t - start), chan, args[0], vel])
    # Anything still sounding when the run stopped is given the rest of the run.
    end = msgs[-1][0] if msgs else 0
    for (chan, note), stack in open_notes.items():
        for start, vel in stack:
            out.append([start, max(1, end - start), chan, note, vel])
    out.sort(key=lambda n: (n[0], n[2], n[3]))
    return out, programs


def find_loop(ns):
    """The tick a tune comes round on, or None.

    Compared by what is sounding at each tick rather than by counting events:
    two passes of a tune play the same notes on the same channels the same
    number of ticks apart, whatever else changed. Candidate periods are the
    ticks whose chord matches the opening chord, which is what keeps this from
    being a search over every possible period.
    """
    if len(ns) < 8:
        return None
    at = {}
    for start, _dur, chan, note, _vel in ns:
        at.setdefault(start, []).append((chan, note))
    for v in at.values():
        v.sort()
    keys = sorted(at)
    span = keys[-1]
    first = keys[0]
    opening = at[first]

    def holds(period):
        for t in keys:
            if t + period > span - 50:
                break
            if at.get(t) != at.get(t + period):
                return False
        return True

    for t in keys:
        period = t - first
        if period < 32 or period > span // 2:
            continue
        if at[t] == opening and holds(period):
            return period
    return None


def find(root, name):
    for sub in ('', 'DISKA', 'DISKB', 'DISKC', 'diska', 'diskb', 'diskc'):
        for cased in (name, name.lower()):
            p = os.path.join(root, sub, cased) if sub else os.path.join(root, cased)
            if os.path.isfile(p):
                return p
    return None


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__.strip().splitlines()[0] +
                         '\n\n  python3 tools/tunes.py <game folder> [out.json]')
    root = sys.argv[1]
    out_path = sys.argv[2] if len(sys.argv) > 2 else 'research/tunes.json'

    tunes = {}
    for n in range(1, 7):
        path = find(root, f'RTUNE{n}.BIN')
        if path is None:
            print(f'tune {n}: no RTUNE{n}.BIN found, skipped')
            continue
        writes = Driver(path).run(TICKS)
        msgs = messages(writes)
        ns, programs = notes(msgs)
        if not ns:
            print(f'tune {n}: the driver played nothing, skipped')
            continue
        loop = find_loop(ns)
        if loop:
            ns = [k for k in ns if k[0] < loop]
            for k in ns:
                k[1] = min(k[1], loop - k[0])
        tunes[f'tune{n}'] = {
            'file': os.path.basename(path),
            'loop_ticks': loop or (ns[-1][0] + ns[-1][1]),
            'looping': loop is not None,
            'programs': programs,
            'notes': ns,
        }
        chans = sorted({k[2] for k in ns})
        secs = tunes[f'tune{n}']['loop_ticks'] * TICK_HZ_DEN / TICK_HZ_NUM
        print(f'tune {n}: {len(ns):5} notes, channels {chans}, '
              f'{"loops" if loop else "runs"} at {secs:.1f}s')

    doc = {
        'source': 'RTUNE1..6.BIN, run under emulation and read off the MPU-401 port',
        'tick_hz_num': TICK_HZ_NUM,
        'tick_hz_den': TICK_HZ_DEN,
        'tunes': tunes,
    }
    os.makedirs(os.path.dirname(out_path) or '.', exist_ok=True)
    with open(out_path, 'w') as f:
        json.dump(doc, f)
    print(f'wrote {out_path}: {len(tunes)} tunes')


if __name__ == '__main__':
    main()
