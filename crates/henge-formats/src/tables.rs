//! The controller tables and the actor records, read out of the image by
//! running the routines that write them.
//!
//! `KnightAttSw`, `KnightHitSw`, `KnightDamSw`, `KnightWalSw`, `KnightBloSw`
//! and every creature's `*Att`, `*Hit`, `*Dam` and `*Wal` table are `BSS`:
//! forty-eight zero bytes apiece in the load image. What fills them is code.
//! `MOON:SetKnightAnims` (image 0x1771) zeroes the block and falls into
//! `SetUpKnight` (0x1786), which writes the knight's five tables one word at
//! a time; `SetMonsterAnims` (0x186b) does the same for the bestiary. Each
//! `Set*Tables` routine then writes an actor record: the addresses of those
//! tables at `+0x14` (hit), `+0x16` (attack), `+0x1a` (damage), `+0x1c`
//! (walk) and `+0x1e` (block), the stance and recovery scripts at `+0x10`
//! and `+0x12`, the bank table at `+0x18`, the kind at `+0x35`, hit points
//! at `+0x38` and `+0x3c`, and the tracker's ranges at `+0x52`, `+0x54` and
//! `+0x56`.
//!
//! All of those routines are straight-line `mov`s, a few `loop`s, and in
//! `SetRatmenTables` one pair of compares against the moon. Rather than
//! transcribe the numbers, this runs the routines: a small interpreter of
//! exactly the instructions they use, which stops on anything else and says
//! where. The words it collects are the tables as the game has them the
//! moment a fight starts, and every script address in them is resolved
//! through the symbol table to a name.

use crate::taskvm::Symbols;
use std::collections::BTreeMap;

/// `DGROUP` times sixteen: where `DS` offset 0 sits in the image.
const DS_BASE: u32 = 0x123b * 16;

/// The `di` a `Set*Tables` routine is entered with, standing in for the
/// actor record it writes. Anything at or above this is a record field.
pub const RECORD: u32 = 0x1_0000;

/// A `DS` word the moon check reads: `[0x8989]`, tonight's phase.
pub const PHASE: u16 = 0x8989;

/// Everything a run of one of the fill routines wrote, by address.
#[derive(Clone, Debug, Default)]
pub struct Writes {
    /// `DS` bytes below [`RECORD`], record bytes at and above it.
    mem: BTreeMap<u32, u8>,
}

impl Writes {
    /// A word of `DS`, as the routines left it. Only what was written.
    pub fn word(&self, off: u16) -> Option<u16> {
        let lo = self.mem.get(&(off as u32))?;
        let hi = self.mem.get(&(off as u32 + 1)).copied().unwrap_or(0);
        Some(u16::from_le_bytes([*lo, hi]))
    }

    /// A word of the actor record.
    pub fn record_word(&self, field: u8) -> Option<u16> {
        let a = RECORD + field as u32;
        let lo = self.mem.get(&a)?;
        let hi = self.mem.get(&(a + 1)).copied().unwrap_or(0);
        Some(u16::from_le_bytes([*lo, hi]))
    }

    /// A byte of the actor record.
    pub fn record_byte(&self, field: u8) -> Option<u8> {
        self.mem.get(&(RECORD + field as u32)).copied()
    }

    fn write_byte(&mut self, addr: u32, v: u8) {
        self.mem.insert(addr, v);
    }

    fn write_word(&mut self, addr: u32, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        self.mem.insert(addr, lo);
        self.mem.insert(addr + 1, hi);
    }
}

// Register numbers as the instruction encoding has them.
const AX: usize = 0;
const CX: usize = 1;
const BX: usize = 3;
const BP: usize = 5;
const SI: usize = 6;
const DI: usize = 7;

/// The interpreter: eight registers, a zero flag and the writes.
struct Machine<'a> {
    img: &'a [u8],
    regs: [u32; 8],
    zero: bool,
    writes: Writes,
    /// `DS` words presented to the routine as though they were already
    /// there: the moon.
    env: BTreeMap<u16, u16>,
}

/// A decoded memory operand: the effective address, or a register.
enum Operand {
    Reg(usize),
    Mem(u32),
}

impl<'a> Machine<'a> {
    fn byte_at(&self, pc: u32) -> anyhow::Result<u8> {
        self.img
            .get(pc as usize)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("ran off the image at {pc:#x}"))
    }

    fn word_at(&self, pc: u32) -> anyhow::Result<u16> {
        Ok(u16::from_le_bytes([
            self.byte_at(pc)?,
            self.byte_at(pc + 1)?,
        ]))
    }

    /// What a read of `DS` sees: the routine's own writes first, then the
    /// environment, then the load image.
    fn read_byte(&self, addr: u32) -> u8 {
        if let Some(b) = self.writes.mem.get(&addr) {
            return *b;
        }
        if addr < RECORD {
            let off = addr as u16;
            if let Some(w) = self.env.get(&off) {
                return w.to_le_bytes()[0];
            }
            if let Some(w) = self.env.get(&off.wrapping_sub(1)) {
                return w.to_le_bytes()[1];
            }
            return self
                .img
                .get((DS_BASE + addr) as usize)
                .copied()
                .unwrap_or(0);
        }
        0
    }

    fn read_word(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read_byte(addr), self.read_byte(addr + 1)])
    }

    /// Decode a 16-bit ModR/M byte and its displacement. Returns the
    /// operand, the `reg` field, and how many bytes were consumed after the
    /// opcode.
    fn modrm(&self, pc: u32) -> anyhow::Result<(Operand, u8, u32)> {
        let m = self.byte_at(pc)?;
        let (md, reg, rm) = (m >> 6, (m >> 3) & 7, m & 7);
        if md == 3 {
            return Ok((Operand::Reg(rm as usize), reg, 1));
        }
        let base = match rm {
            4 => self.regs[SI],
            5 => self.regs[DI],
            6 if md == 0 => 0,
            6 => self.regs[BP],
            7 => self.regs[BX],
            _ => anyhow::bail!("unsupported addressing mode {m:#04x} at {pc:#x}"),
        };
        let (disp, used) = match md {
            0 if rm == 6 => (self.word_at(pc + 1)? as u32, 3),
            0 => (0, 1),
            1 => (self.byte_at(pc + 1)? as i8 as i32 as u32, 2),
            _ => (self.word_at(pc + 1)? as u32, 3),
        };
        Ok((Operand::Mem(base.wrapping_add(disp)), reg, used))
    }

    fn get16(&self, op: &Operand) -> u16 {
        match op {
            Operand::Reg(r) => self.regs[*r] as u16,
            Operand::Mem(a) => self.read_word(*a),
        }
    }

    fn set16(&mut self, op: &Operand, v: u16) {
        match op {
            Operand::Reg(r) => self.regs[*r] = v as u32,
            Operand::Mem(a) => self.writes.write_word(*a, v),
        }
    }

    fn set8(&mut self, op: &Operand, v: u8) {
        match op {
            Operand::Reg(r) => {
                // al, cl, dl, bl only; the routines write no high byte.
                self.regs[*r & 3] = (self.regs[*r & 3] & 0xff00) | v as u32;
            }
            Operand::Mem(a) => self.writes.write_byte(*a, v),
        }
    }

    /// Run from `pc` until a `ret`, or until `stop` is reached.
    fn run(&mut self, mut pc: u32, stop: Option<u32>) -> anyhow::Result<()> {
        for _ in 0..100_000 {
            if Some(pc) == stop {
                return Ok(());
            }
            let op = self.byte_at(pc)?;
            match op {
                // push es / pop es / push ds / pop ds, push r16 / pop r16.
                0x06 | 0x07 | 0x1e | 0x1f | 0x50..=0x5f => pc += 1,
                // mov r16, imm16
                0xb8..=0xbf => {
                    self.regs[(op - 0xb8) as usize] = self.word_at(pc + 1)? as u32;
                    pc += 3;
                }
                // mov r/m16, imm16
                0xc7 => {
                    let (dst, _, used) = self.modrm(pc + 1)?;
                    let imm = self.word_at(pc + 1 + used)?;
                    self.set16(&dst, imm);
                    pc += 1 + used + 2;
                }
                // mov r/m8, imm8
                0xc6 => {
                    let (dst, _, used) = self.modrm(pc + 1)?;
                    let imm = self.byte_at(pc + 1 + used)?;
                    self.set8(&dst, imm);
                    pc += 1 + used + 1;
                }
                // mov r16, r/m16
                0x8b => {
                    let (src, reg, used) = self.modrm(pc + 1)?;
                    let v = self.get16(&src);
                    self.regs[reg as usize] = v as u32;
                    pc += 1 + used;
                }
                // mov r/m16, r16
                0x89 => {
                    let (dst, reg, used) = self.modrm(pc + 1)?;
                    let v = self.regs[reg as usize] as u16;
                    self.set16(&dst, v);
                    pc += 1 + used;
                }
                // xor r16, r/m16: only ever a register against itself.
                0x33 => {
                    let (src, reg, used) = self.modrm(pc + 1)?;
                    let v = self.regs[reg as usize] as u16 ^ self.get16(&src);
                    self.regs[reg as usize] = v as u32;
                    self.zero = v == 0;
                    pc += 1 + used;
                }
                // rep stosb
                0xf3 if self.byte_at(pc + 1)? == 0xaa => {
                    let al = self.regs[AX] as u8;
                    while self.regs[CX] != 0 {
                        let di = self.regs[DI];
                        self.writes.write_byte(di, al);
                        self.regs[DI] = di + 1;
                        self.regs[CX] -= 1;
                    }
                    pc += 2;
                }
                // group 1, r/m16, imm8 sign-extended: add (/0), sub (/5), cmp (/7)
                0x83 => {
                    let (dst, sub, used) = self.modrm(pc + 1)?;
                    let imm = self.byte_at(pc + 1 + used)? as i8 as i16 as u16;
                    let cur = self.get16(&dst);
                    match sub {
                        0 => {
                            let v = cur.wrapping_add(imm);
                            self.set16(&dst, v);
                            self.zero = v == 0;
                        }
                        5 => {
                            let v = cur.wrapping_sub(imm);
                            self.set16(&dst, v);
                            self.zero = v == 0;
                        }
                        7 => self.zero = cur == imm,
                        _ => anyhow::bail!("group 1 /{sub} at {pc:#x} is not read here"),
                    }
                    pc += 1 + used + 1;
                }
                // loop rel8
                0xe2 => {
                    let rel = self.byte_at(pc + 1)? as i8 as i32;
                    self.regs[CX] = (self.regs[CX] as u16).wrapping_sub(1) as u32;
                    pc = if self.regs[CX] != 0 {
                        (pc as i32 + 2 + rel) as u32
                    } else {
                        pc + 2
                    };
                }
                // je / jne / jmp short
                0x74 | 0x75 | 0xeb => {
                    let rel = self.byte_at(pc + 1)? as i8 as i32;
                    let taken = match op {
                        0x74 => self.zero,
                        0x75 => !self.zero,
                        _ => true,
                    };
                    pc = if taken {
                        (pc as i32 + 2 + rel) as u32
                    } else {
                        pc + 2
                    };
                }
                // ret
                0xc3 => return Ok(()),
                _ => anyhow::bail!(
                    "opcode {op:#04x} at image {pc:#x} is not one the fill routines use"
                ),
            }
        }
        anyhow::bail!("the routine did not return");
    }
}

/// Run a routine and collect what it wrote.
///
/// `di` is set to [`RECORD`] so a `Set*Tables` routine's `[di+n]` stores come
/// out as record fields; `env` supplies any `DS` word the routine compares
/// against, which is only ever the moon. `stop` ends the run at an address
/// before the `ret`, for a routine that goes on to call something.
pub fn run(
    img: &[u8],
    start: u32,
    stop: Option<u32>,
    env: &BTreeMap<u16, u16>,
) -> anyhow::Result<Writes> {
    run_over(img, start, stop, env, Writes::default())
}

/// [`run`], continuing over what an earlier run wrote: for a record two
/// stretches of code write in turn.
pub fn run_over(
    img: &[u8],
    start: u32,
    stop: Option<u32>,
    env: &BTreeMap<u16, u16>,
    writes: Writes,
) -> anyhow::Result<Writes> {
    let mut m = Machine {
        img,
        regs: [0; 8],
        zero: false,
        writes,
        env: env.clone(),
    };
    m.regs[DI] = RECORD;
    m.run(start, stop)
        .map_err(|e| anyhow::anyhow!("running the routine at image {start:#x}: {e}"))?;
    Ok(m.writes)
}

/// Run a routine by name.
pub fn run_named(
    img: &[u8],
    syms: &Symbols,
    name: &str,
    stop: Option<&str>,
    env: &BTreeMap<u16, u16>,
) -> anyhow::Result<Writes> {
    let start = syms
        .entry(name)
        .ok_or_else(|| anyhow::anyhow!("no routine called {name} in the symbol table"))?;
    let stop = match stop {
        Some(s) => Some(
            syms.entry(s)
                .ok_or_else(|| anyhow::anyhow!("no routine called {s} in the symbol table"))?,
        ),
        None => None,
    };
    run(img, start, stop, env)
}

/// The controller tables after `SetKnightAnims` and `SetMonsterAnims` have
/// run, which `InitGameStart` does once at start-up.
pub fn controller_tables(img: &[u8], syms: &Symbols) -> anyhow::Result<Writes> {
    let env = BTreeMap::new();
    // `SetKnightAnims` (0x1771) falls straight into `SetUpKnight` (0x1786)
    // and returns from its end (0x186a), so one run covers both.
    let mut w = run_named(img, syms, "SetKnightAnims", None, &env)?;
    let m = run_named(img, syms, "SetMonsterAnims", None, &env)?;
    w.mem.extend(m.mem);
    Ok(w)
}

/// One actor as its `Set*Tables` routine and the controller tables leave it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorTables {
    /// `+0x10`.
    pub stance: String,
    /// `+0x12`.
    pub recover: String,
    /// `+0x35`.
    pub kind: Option<u8>,
    /// `+0x38` and `+0x3c`.
    pub health: Option<i32>,
    pub max_health: Option<i32>,
    /// `+0x52`, `+0x54`, `+0x56`.
    pub approach: Option<i32>,
    pub back_off: Option<i32>,
    pub plane: Option<i32>,
    /// `+0x18`, which of the `TASKCELBUF` tables the parts index: `0x8933`
    /// is table 1 and `0x8949` table 2.
    pub bank_table: Option<u16>,
    /// The `*Att` table, by attack kind. Kind 0 is the stance.
    pub attacks: BTreeMap<u8, String>,
    /// The `*Hit` table, by the attacker's kind.
    pub hits: BTreeMap<u8, String>,
    /// The `*Dam` table, by kind.
    pub damage: BTreeMap<u8, i32>,
    /// The `*Blo` table: the guard that stops each kind.
    pub blocks: BTreeMap<u8, u8>,
    /// The `*Wal` table's three rows: right at `+0`, up at `+0x10`, down at
    /// `+0x20`, each read to its zero.
    pub walk: [Vec<String>; 3],
}

/// The address of the `TASKCELBUF` table the knight's records name.
pub const TABLE_1: u16 = 0x8933;
/// The one every creature's records name.
pub const TABLE_2: u16 = 0x8949;

fn script_name(syms: &Symbols, off: u16, what: &str) -> anyhow::Result<String> {
    syms.datum(off)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{what} holds {off:#x}, which names no data symbol"))
}

/// Nine words from `off`, by their kind (the byte offset into the table).
fn by_kind(
    w: &Writes,
    off: u16,
    syms: &Symbols,
    what: &str,
) -> anyhow::Result<BTreeMap<u8, String>> {
    let mut out = BTreeMap::new();
    for kind in (0..=0x10u16).step_by(2) {
        match w.word(off + kind) {
            Some(0) | None => {}
            Some(v) => {
                out.insert(kind as u8, script_name(syms, v, what)?);
            }
        }
    }
    Ok(out)
}

/// The same nine, as numbers.
fn numbers(w: &Writes, off: u16) -> BTreeMap<u8, i32> {
    (0..=0x10u16)
        .step_by(2)
        .filter_map(|k| w.word(off + k).map(|v| (k as u8, v as i32)))
        .collect()
}

/// A walk row: up to eight words, read to the first zero, the way
/// `NextWalk` (0x4ef7) steps over the tail.
fn walk_row(w: &Writes, off: u16, syms: &Symbols, what: &str) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for i in 0..8u16 {
        match w.word(off + i * 2) {
            Some(0) | None => break,
            Some(v) => out.push(script_name(syms, v, what)?),
        }
    }
    Ok(out)
}

/// Read one actor's tables off a record a `Set*Tables` routine wrote, and
/// the controller tables it points into. `tables` is what the two fill
/// routines wrote and `record` what the actor's own routine did; the second
/// is laid over the first, since `SetRatmenTables` writes `RatmenDam` itself
/// and nothing else does.
pub fn actor_tables(
    tables: &Writes,
    record: &Writes,
    syms: &Symbols,
) -> anyhow::Result<ActorTables> {
    let mut merged = tables.clone();
    merged.mem.extend(record.mem.iter().map(|(a, b)| (*a, *b)));
    let tables = &merged;
    let mut t = ActorTables::default();
    if let Some(v) = record.record_word(0x10) {
        t.stance = script_name(syms, v, "+0x10")?;
    }
    if let Some(v) = record.record_word(0x12) {
        t.recover = script_name(syms, v, "+0x12")?;
    }
    t.kind = record.record_byte(0x35);
    t.health = record.record_word(0x38).map(|v| v as i16 as i32);
    t.max_health = record.record_word(0x3c).map(|v| v as i16 as i32);
    t.approach = record.record_word(0x52).map(|v| v as i16 as i32);
    t.back_off = record.record_word(0x54).map(|v| v as i16 as i32);
    t.plane = record.record_word(0x56).map(|v| v as i16 as i32);
    t.bank_table = record.record_word(0x18);
    if let Some(off) = record.record_word(0x16) {
        t.attacks = by_kind(tables, off, syms, "the attack table")?;
    }
    if let Some(off) = record.record_word(0x14) {
        t.hits = by_kind(tables, off, syms, "the hit table")?;
    }
    if let Some(off) = record.record_word(0x1a) {
        t.damage = numbers(tables, off);
    }
    if let Some(off) = record.record_word(0x1e) {
        t.blocks = numbers(tables, off)
            .into_iter()
            .map(|(k, v)| (k, v as u8))
            .collect();
    }
    if let Some(off) = record.record_word(0x1c) {
        // `TrollWal` and `MudmenWal` are eighteen bytes each, one row, and
        // the next table starts where their up row would: a row is only
        // read where the symbol table says the table still is.
        let end = syms.next_datum(off).unwrap_or(u16::MAX);
        let row = |at: u16, what: &str| -> anyhow::Result<Vec<String>> {
            if at < end {
                walk_row(tables, at, syms, what)
            } else {
                Ok(Vec::new())
            }
        };
        t.walk = [
            row(off, "the walk table")?,
            row(off + 0x10, "the walk table's up row")?,
            row(off + 0x20, "the walk table's down row")?,
        ];
    }
    Ok(t)
}

/// The `Set*Tables` routines that write a whole record, by the actor they
/// set up.
pub const RECORD_ROUTINES: &[(&str, &str)] = &[
    ("knight", "SetKnightSwTables"),
    ("trogg_axe", "SetTroggAxeTables"),
    ("trogg_hammer", "SetTroggHammerTables"),
    ("trogg_spear", "SetTroggSpTables"),
    ("beast", "SetBeastTables"),
    ("ratmen", "SetRatmenTables"),
    ("dragon", "SetUpDragonTables"),
    ("balok", "SetBalokTables"),
    ("mudmen", "SetUpMudmenTables"),
    ("troll", "SetTrollTable"),
];

/// Every actor's tables: the ten `Set*Tables` routines, plus the demon,
/// whose record `InitKnightvsDemon` writes inline between its `FindTABLE`
/// and its `AddPlayer` (0x2771 to 0x27af).
pub fn all_actor_tables(
    img: &[u8],
    syms: &Symbols,
    phase: u16,
) -> anyhow::Result<BTreeMap<String, ActorTables>> {
    let tables = controller_tables(img, syms)?;
    let env: BTreeMap<u16, u16> = BTreeMap::from([(PHASE, phase)]);
    let mut out = BTreeMap::new();
    for (actor, routine) in RECORD_ROUTINES {
        let record = run_named(img, syms, routine, None, &env)?;
        let t =
            actor_tables(&tables, &record, syms).map_err(|e| anyhow::anyhow!("{routine}: {e}"))?;
        out.insert((*actor).to_string(), t);
    }
    // `InitKnightvsDemon` (0x273d): after `call FindTABLE` at 0x276a the
    // record is written a field at a time up to `call AddPlayer` at 0x27af.
    // The image offsets are found from the symbol rather than assumed.
    let demon = syms
        .entry("InitKnightvsDemon")
        .ok_or_else(|| anyhow::anyhow!("no InitKnightvsDemon"))?;
    let start = demon + 0x34;
    let stop = demon + 0x72;
    anyhow::ensure!(
        img.get(start as usize) == Some(&0xc7) && img.get(stop as usize) == Some(&0xe8),
        "InitKnightvsDemon does not have the shape this was read from"
    );
    let record = run(img, start, Some(stop), &env)?;
    let t = actor_tables(&tables, &record, syms)
        .map_err(|e| anyhow::anyhow!("InitKnightvsDemon: {e}"))?;
    out.insert("demon".to_string(), t);
    // `InitKnightvsDragon` (0x2438) builds each claw in three stretches:
    // the seat and fifty hit points (0x249c to 0x24b5), `SetUpDragonTables`
    // over them, then the kind, `Dragon_Claw` for stance and recovery and a
    // plane of ten (0x24b8 to 0x24cb). The second claw is the same at
    // 0x24d5, with a deeper seat.
    let dragon = syms
        .entry("InitKnightvsDragon")
        .ok_or_else(|| anyhow::anyhow!("no InitKnightvsDragon"))?;
    let setup = syms
        .entry("SetUpDragonTables")
        .ok_or_else(|| anyhow::anyhow!("no SetUpDragonTables"))?;
    let (a, b, c, d) = (dragon + 0x64, dragon + 0x7d, dragon + 0x80, dragon + 0x93);
    anyhow::ensure!(
        img.get(a as usize) == Some(&0xc7)
            && img.get(b as usize) == Some(&0xe8)
            && img.get(c as usize) == Some(&0xc6)
            && img.get(d as usize) == Some(&0xe8),
        "InitKnightvsDragon does not have the shape this was read from"
    );
    let claw = run(img, a, Some(b), &env)?;
    let claw = run_over(img, setup, None, &env, claw)?;
    let claw = run_over(img, c, Some(d), &env, claw)?;
    let t = actor_tables(&tables, &claw, syms)
        .map_err(|e| anyhow::anyhow!("InitKnightvsDragon, the claw: {e}"))?;
    out.insert("dragon_claw".to_string(), t);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Option<(Vec<u8>, Symbols)> {
        let img = std::fs::read("../../research/main.final.bin").ok()?;
        let syms = std::fs::read_to_string("../../research/symbols.json").ok()?;
        Some((img, Symbols::parse(&syms).ok()?))
    }

    /// `SetUpKnight` (0x1786): `KnightAttSw[4]` is `Knight_SwSwing`,
    /// `KnightHitSw[4]` is `Knight_SwWaistHit`, `KnightDamSw[0x10]` is 4,
    /// `KnightBloSw[4]` is 8, and `KnightWalSw` holds three rows of four.
    #[test]
    fn the_knight_tables_read_as_set_up_knight_writes_them() {
        let Some((img, syms)) = image() else { return };
        let all = all_actor_tables(&img, &syms, 0x2e).unwrap();
        let k = &all["knight"];
        assert_eq!(k.stance, "Knight_SwStance");
        assert_eq!(k.recover, "Knight_SwRecover");
        assert_eq!(k.attacks[&4], "Knight_SwSwing");
        assert_eq!(k.attacks[&0x10], "Knight_SwChop");
        assert_eq!(k.attacks[&0], "Knight_SwStance");
        assert_eq!(k.hits[&4], "Knight_SwWaistHit");
        assert_eq!(k.hits[&0xe], "Knight_SwRecover");
        assert_eq!(k.damage[&0x10], 4);
        assert_eq!(k.damage[&0xa], 2);
        assert_eq!(k.blocks[&4], 8);
        assert_eq!(k.blocks[&0x10], 0xe);
        assert_eq!(k.walk[0].len(), 4);
        assert_eq!(k.walk[1][0], "Knight_SwWalkU1");
        assert_eq!(k.walk[2][3], "Knight_SwWalkD4");
        assert_eq!(
            (k.approach, k.back_off, k.plane),
            (Some(100), Some(80), Some(4))
        );
        assert_eq!(k.bank_table, Some(TABLE_1));
    }

    /// `SetRatmenTables` (0x23ae) and `RatNewMoon` (0x241b): five and a
    /// slash of one on most nights, seven and three under 0x2d, twelve and
    /// five under 0x31; the bite is three, six and eight.
    #[test]
    fn the_ratman_reads_the_moon() {
        let Some((img, syms)) = image() else { return };
        for (phase, health, slash, bite) in [(0x2e, 5, 1, 3), (0x2d, 7, 3, 6), (0x31, 12, 5, 8)] {
            let all = all_actor_tables(&img, &syms, phase).unwrap();
            let r = &all["ratmen"];
            assert_eq!(r.health, Some(health), "phase {phase:#x}");
            assert_eq!(r.damage[&4], slash);
            assert_eq!(r.damage[&2], bite);
        }
    }

    /// `SetMonsterAnims+0x207` (0x1a72): `BeastWal` is copied out of `BEWAL`
    /// (DS:0x956), five words to the right row and five to the up row, so
    /// the run is `Beast_Run1..4` and the drool sits where a walk up would.
    #[test]
    fn the_beast_walk_is_copied_from_bewal() {
        let Some((img, syms)) = image() else { return };
        let all = all_actor_tables(&img, &syms, 0x2e).unwrap();
        let b = &all["beast"];
        assert_eq!(
            b.walk[0],
            ["Beast_Run1", "Beast_Run2", "Beast_Run3", "Beast_Run4"]
        );
        assert_eq!(b.walk[1][0], "Beast_Drool1");
        assert!(b.walk[2].is_empty());
        assert_eq!(b.stance, "Beast_Drool1");
        assert_eq!(b.recover, "Beast_TurnAround");
        assert_eq!(b.bank_table, Some(TABLE_2));
    }

    /// `InitKnightvsDragon` (0x249c to 0x24cb): the claw is
    /// `SetUpDragonTables` with `Dragon_Claw` over the stance and the
    /// recovery, kind 0x16 and a plane of ten; its fifty hit points are
    /// written before the routine puts 200 and 120 back over them.
    #[test]
    fn the_claw_is_the_dragon_record_with_its_own_stance() {
        let Some((img, syms)) = image() else { return };
        let all = all_actor_tables(&img, &syms, 0x2e).unwrap();
        let c = &all["dragon_claw"];
        assert_eq!(c.stance, "Dragon_Claw");
        assert_eq!(c.recover, "Dragon_Claw");
        assert_eq!(c.kind, Some(0x16));
        assert_eq!(c.plane, Some(10));
        assert_eq!((c.health, c.max_health), (Some(200), Some(120)));
        assert_eq!(c.hits[&4], "Dragon_Hit");
        assert_eq!(all["dragon"].kind, Some(0xa));
    }

    /// `InitKnightvsDemon` (0x2771): the demon's stance slot is its
    /// entrance, and its record has no tables at all.
    #[test]
    fn the_demon_record_is_written_inline() {
        let Some((img, syms)) = image() else { return };
        let all = all_actor_tables(&img, &syms, 0x2e).unwrap();
        let d = &all["demon"];
        assert_eq!(d.stance, "Demon_Evolve");
        assert_eq!(d.recover, "Demon_Stance1");
        assert_eq!(d.health, Some(250));
        assert_eq!(
            (d.approach, d.back_off, d.plane),
            (Some(95), Some(90), Some(2))
        );
        assert!(d.hits.is_empty() && d.walk[0].is_empty());
    }
}
