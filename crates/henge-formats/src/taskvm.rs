//! Reading the animation task VM's scripts out of the unpacked `MAIN.EXE`.
//!
//! This is the Rust port of what `tools/taskvm.py` does, so that baking a pack
//! does not have to shell out to python. It reads the same two inputs the tool
//! does: the fully unpacked load image, and the symbol table `symbolmap.py`
//! recovers from it. Neither can be produced here, because unpacking `MAIN.EXE`
//! means running two decompression stubs under emulation; the baker looks for
//! the files that tool writes and says plainly when they are not there.
//!
//! Nothing about the instruction set is hardcoded. The handler table is BSS, so
//! its addresses exist in the file only as immediates in `INITTASK`, and each
//! command's operand width is read out of that handler's own
//! `add word ptr [di+2], n`. Both are recovered here at bake time and then
//! checked against what `docs/TASKVM.md` records, so a different or wrongly
//! unpacked image is caught rather than parsed into plausible rubbish.
//!
//! The output is [`henge_core::taskvm`] types, which is the point: the baker
//! cannot write a script the engine would not accept, because it builds the
//! engine's own values and serializes those.

use henge_core::taskvm::{End, Instr, Part, Script, ScriptSet};
use std::collections::{BTreeMap, BTreeSet};

/// `DGROUP`, so a data symbol's address minus this is its `DS` offset.
const DGROUP: u32 = 0x123b;
const DS_BASE: u32 = DGROUP * 16;

/// The `TaskComTable` offset in `DS`, which is also the immediate `INITTASK`
/// loads into `di` before filling it.
const TASK_COM_TABLE: u16 = 0x9448;

/// The nineteen `PUBLIC` names between `TASKSORT` and `TASKWALKCOLLIDE`, in the
/// order the `PUBLIC` blob emits them, which is definition order.
///
/// Zipping these against the nineteen handler addresses in table order is an
/// ordering argument rather than a proof, and `docs/TASKVM.md` says so. Nothing
/// below depends on a name being right: the opcode numbers, the widths and the
/// behaviour all come from the code.
const HANDLER_NAMES: [&str; 19] = [
    "TASK_FLIP", "TASKGOTO", "TASKHOLD", "TASKJUMP", "TASKLOOP", "TASKSKIP", "TASKTIME",
    "TASKSOUND", "TASKMOVE", "TASKSHADOW", "TASKSAVE", "TASKGOSUB", "TASKDEAD", "TASKADDTASK",
    "TASKKILLTASK", "TASKCELBUF", "TASKTESTEQ", "TASKTESTNE", "TASKANIMCLR",
];

/// What the opcode set has to come out as. Recovered once and written up in
/// `docs/TASKVM.md`; asserted here so that an image which decodes to anything
/// else is rejected instead of silently producing different animations.
const EXPECTED: [(u8, &str, usize); 19] = [
    (0x80, "TASK_FLIP", 2),
    (0x82, "TASKGOTO", 4),
    (0x84, "TASKHOLD", 2),
    (0x86, "TASKJUMP", 8),
    (0x8a, "TASKLOOP", 2),
    (0x8c, "TASKSKIP", 4),
    (0x8e, "TASKTIME", 0),
    (0x90, "TASKMOVE", 8),
    (0x92, "TASKSOUND", 2),
    (0x94, "TASKSAVE", 6),
    (0x96, "TASKSHADOW", 4),
    (0x98, "TASKGOSUB", 4),
    (0x9a, "TASKDEAD", 4),
    (0x9c, "TASKADDTASK", 4),
    (0x9e, "TASKKILLTASK", 2),
    (0xa0, "TASKCELBUF", 2),
    (0xa4, "TASKTESTEQ", 6),
    (0xa6, "TASKTESTNE", 6),
    (0xa8, "TASKANIMCLR", 2),
];

/// A script's name is its encounter followed by an underscore. These are the
/// prefixes the 221 animation scripts in `DGROUP` use; nothing else in the
/// symbol table shares the shape.
const SCRIPT_PREFIXES: [&str; 10] = [
    "Knight", "Hero", "Player", "Beast", "Ratman", "Mudman", "Troll", "Demon", "Dragon", "Balok",
];

fn is_script_name(name: &str) -> bool {
    let Some(head) = name.split('_').next() else { return false };
    if head.len() == name.len() {
        return false; // no underscore at all
    }
    SCRIPT_PREFIXES.contains(&head) || head.starts_with("Trogg")
}

// ------------------------------------------------------------------- symbols

/// The symbol table `tools/symbolmap.py` writes, with the link-time to
/// image-offset correction it fits.
pub struct Symbols {
    /// Image offset to the routine that starts there.
    code: BTreeMap<u32, String>,
    /// `[lo, hi, shift]` over link-time code offsets.
    shifts: Vec<(u32, u32, i32)>,
    /// `DS` offset to the data symbol that starts there.
    data: BTreeMap<u32, String>,
}

impl Symbols {
    pub fn parse(json: &str) -> anyhow::Result<Symbols> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let seg = v["dgroup_seg"].as_u64().unwrap_or(0) as u32;
        anyhow::ensure!(
            seg == DGROUP,
            "the symbol table says DGROUP is {seg:#x}, not {DGROUP:#x}: wrong image"
        );
        let mut shifts = Vec::new();
        for r in v["code_shift_map"].as_array().into_iter().flatten() {
            let a = r.as_array().ok_or_else(|| anyhow::anyhow!("bad code_shift_map row"))?;
            shifts.push((
                a[0].as_i64().unwrap_or(0) as u32,
                a[1].as_i64().unwrap_or(0) as u32,
                a[2].as_i64().unwrap_or(0) as i32,
            ));
        }
        shifts.sort_unstable();

        let mut code = BTreeMap::new();
        let mut data = BTreeMap::new();
        for s in v["symbols"].as_array().into_iter().flatten() {
            let Some(name) = s["name"].as_str() else { continue };
            let addr = s["addr"].as_i64().unwrap_or(-1);
            if addr < 0 {
                continue;
            }
            match s["kind"].as_str() {
                // The first symbol at an address wins, which is what the tool
                // does: several names can share an entry point.
                Some("code") => {
                    code.entry(addr as u32).or_insert_with(|| name.to_string());
                }
                Some("data") if addr as u32 >= DS_BASE => {
                    data.entry(addr as u32 - DS_BASE).or_insert_with(|| name.to_string());
                }
                _ => {}
            }
        }
        anyhow::ensure!(!code.is_empty() && !data.is_empty(), "the symbol table is empty");
        Ok(Symbols { code, shifts, data })
    }

    /// A link-time code offset, as baked into the code, to an image offset.
    ///
    /// Absolute code addresses in this binary live in a coordinate space up to
    /// 473 bytes longer than the shipped image. Skip this and no handler
    /// address and no `TASKGOSUB` target lands on an instruction boundary.
    pub fn to_image(&self, raw: u16) -> u32 {
        let raw = raw as u32;
        let shift = self
            .shifts
            .iter()
            .rev()
            .find(|(lo, _, _)| *lo <= raw)
            .map_or(0, |(_, _, s)| *s);
        (raw as i64 + shift as i64).max(0) as u32
    }

    /// The routine a link-time offset names, if it is exactly an entry point.
    pub fn routine(&self, raw: u16) -> Option<&str> {
        self.code.get(&self.to_image(raw)).map(String::as_str)
    }

    /// The data symbol at a `DS` offset, if one starts exactly there.
    pub fn datum(&self, off: u16) -> Option<&str> {
        self.data.get(&(off as u32)).map(String::as_str)
    }

    /// Every animation script, by name, with its `DS` offset.
    pub fn scripts(&self) -> Vec<(String, u16)> {
        let mut v: Vec<(String, u16)> = self
            .data
            .iter()
            .filter(|(off, name)| is_script_name(name) && **off <= u16::MAX as u32)
            .map(|(off, name)| (name.clone(), *off as u16))
            .collect();
        v.sort();
        v
    }
}

// -------------------------------------------------------------- the code side

/// One command, as recovered from the image rather than from a table here.
#[derive(Clone, Debug)]
pub struct Command {
    pub name: &'static str,
    /// Image offset of the handler.
    pub handler: u32,
    /// How many bytes it consumes, read off its own `add word ptr [di+2], n`.
    pub len: usize,
}

/// The nineteen handler addresses, read out of `INITTASK`'s run of
/// `mov word ptr [di+off], imm16`.
///
/// `TaskComTable` is BSS, so the data segment holds nothing but zeroes and the
/// only copy of these addresses in the file is the code that installs them.
fn read_com_table(img: &[u8]) -> anyhow::Result<BTreeMap<u8, u16>> {
    let want = [0xbf, TASK_COM_TABLE as u8, (TASK_COM_TABLE >> 8) as u8]; // mov di, 0x9448
    let mut p = img
        .windows(3)
        .take(0xd800)
        .position(|w| w == want)
        .ok_or_else(|| anyhow::anyhow!("INITTASK does not load TaskComTable; wrong image"))?
        + 3;
    let mut slots = BTreeMap::new();
    loop {
        let (off, imm, next) = match img.get(p..p + 2) {
            // mov word ptr [di], imm16
            Some([0xc7, 0x05]) => (0u8, u16::from_le_bytes([img[p + 2], img[p + 3]]), p + 4),
            // mov word ptr [di+d8], imm16
            Some([0xc7, 0x45]) => {
                (img[p + 2], u16::from_le_bytes([img[p + 3], img[p + 4]]), p + 5)
            }
            _ => break,
        };
        slots.insert(off, imm);
        p = next;
    }
    Ok(slots)
}

/// How far a handler moves the script pointer.
///
/// Every command handler ends with `add word ptr [di+2], n`, so the operand
/// width is read out of the code rather than inferred from the data. A handler
/// with several exits uses the same `n` on each; `TASKGOTO`'s taken branch
/// writes the pointer outright and consumes nothing, so the smallest `n` in the
/// body is the one that matters.
fn advance(img: &[u8], at: u32) -> usize {
    let at = at as usize;
    if img.get(at) == Some(&0xc3) {
        return 0; // a bare RET: consumes nothing, so the original would spin
    }
    let mut best = None;
    for i in at..(at + 0x60).min(img.len().saturating_sub(4)) {
        if img[i..i + 3] == [0x83, 0x45, 0x02] {
            let n = img[i + 3] as usize;
            best = Some(best.map_or(n, |b: usize| b.min(n)));
        }
        if img[i] == 0xc3 && best.is_some() {
            break;
        }
    }
    best.unwrap_or(0)
}

/// The opcode set, recovered from the image and then checked against what was
/// documented.
pub fn opcode_set(img: &[u8], syms: &Symbols) -> anyhow::Result<BTreeMap<u8, Command>> {
    let slots = read_com_table(img)?;
    anyhow::ensure!(
        slots.len() == HANDLER_NAMES.len(),
        "INITTASK fills {} handler slots, expected {}",
        slots.len(),
        HANDLER_NAMES.len()
    );
    // The names are emitted in definition order, so they line up with the
    // handlers sorted by address, not by opcode.
    let mut by_addr: Vec<(u8, u16)> = slots.iter().map(|(o, a)| (*o, *a)).collect();
    by_addr.sort_by_key(|(_, a)| *a);
    let mut named: BTreeMap<u8, &'static str> = BTreeMap::new();
    for ((off, _), name) in by_addr.iter().zip(HANDLER_NAMES) {
        named.insert(*off, name);
    }

    let mut ops = BTreeMap::new();
    for (off, raw) in slots {
        let handler = syms.to_image(raw);
        ops.insert(
            0x80 + off,
            Command { name: named[&off], handler, len: advance(img, handler) },
        );
    }

    // This is the check that the link-time correction was applied and that this
    // is the image the VM was decoded from. Each width is read out of the
    // handler's own `add word ptr [di+2], n`, so a wrong address reads a
    // different instruction and the widths come out as anything but these.
    // Uncorrected, the addresses are 473 bytes out and land mid-instruction.
    let got: Vec<(u8, &str, usize)> =
        ops.iter().map(|(op, c)| (*op, c.name, c.len)).collect();
    let want: Vec<(u8, &str, usize)> = EXPECTED.iter().map(|(o, n, l)| (*o, *n, *l)).collect();
    anyhow::ensure!(
        got == want,
        "the opcode set recovered from this image is not the one in docs/TASKVM.md:\n  \
         got  {got:?}\n  want {want:?}"
    );
    Ok(ops)
}

// ------------------------------------------------------------------- parsing

fn i8_at(img: &[u8], at: u32) -> i16 {
    img[at as usize] as i8 as i16
}

fn u16_at(img: &[u8], at: u32) -> u16 {
    u16::from_le_bytes([img[at as usize], img[at as usize + 1]])
}

fn i16_at(img: &[u8], at: u32) -> i16 {
    u16_at(img, at) as i16
}

/// Resolve a word operand that names a script. Zero is the "none" form that
/// `TASKSHADOW` uses to turn a shadow off.
fn script_target(syms: &Symbols, w: u16, what: &str, script: &str) -> anyhow::Result<String> {
    if w == 0 {
        return Ok(String::new());
    }
    syms.datum(w)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{script}: {what} {w:#06x} is not the start of a script"))
}

/// Walk one script from a `DS` offset into a list of instructions.
pub fn parse_script(
    img: &[u8],
    syms: &Symbols,
    ops: &BTreeMap<u8, Command>,
    name: &str,
    off: u16,
) -> anyhow::Result<Script> {
    let mut code = Vec::new();
    let mut pc = off as u32;
    for _ in 0..4096 {
        let at = DS_BASE + pc;
        anyhow::ensure!((at as usize) < img.len(), "{name}: script runs off the image");
        let op = img[at as usize];

        if op == 0xff {
            let end = match img[at as usize + 1] {
                0x00 => End::Next,
                0xfe => End::Loop,
                0xff => End::Stop,
                other => anyhow::bail!("{name}: unknown end-of-frame byte {other:#04x}"),
            };
            code.push(Instr::EndFrame { end });
            if end == End::Stop {
                return Ok(Script::new(code));
            }
            pc += 2;
            continue;
        }
        if op == 0xfd {
            code.push(Instr::ResumeJump);
            pc += 1;
            continue;
        }
        if op == 0xfe {
            code.push(Instr::ResumeLoop);
            pc += 1;
            continue;
        }
        if op & 0x80 == 0 {
            // A six-byte sprite part: [u8 bank*4][u8 cel][i8 y][u8 flags][i16 x].
            let sel = op & 0x1f;
            anyhow::ensure!(
                sel % 4 == 0,
                "{name}: bank selector {sel:#04x} is not a multiple of four"
            );
            code.push(Instr::Part(Part {
                // TASKCELBUF picks the table at run time, so a part records
                // only its slot. The interpreter stamps the table on it.
                table: 1,
                bank: sel / 4,
                cel: img[at as usize + 1],
                y: i8_at(img, at + 2),
                flags: img[at as usize + 3],
                x: i16_at(img, at + 4),
            }));
            pc += 6;
            continue;
        }

        let cmd = ops
            .get(&op)
            .ok_or_else(|| anyhow::anyhow!("{name}: opcode {op:#04x} is not in TaskComTable"))?;
        let a = |n: u32| img[(at + n) as usize];
        let w = |n: u32| u16_at(img, at + n);
        let instr = match cmd.name {
            "TASK_FLIP" => Instr::Flip { facing: a(1) },
            "TASKGOTO" => Instr::Goto {
                mode: a(1),
                target: script_target(syms, w(2), "TASKGOTO", name)?,
            },
            "TASKHOLD" => Instr::Hold { count: a(1) },
            "TASKJUMP" => Instr::Jump {
                arg: a(1),
                ticks: a(2),
                flags: a(3),
                y_speed: a(4),
                y_limit: a(5),
                x_speed: a(6),
                x_limit: a(7),
            },
            "TASKLOOP" => Instr::Loop { count: a(1) },
            "TASKSKIP" => Instr::Skip {
                target: script_target(syms, w(2), "TASKSKIP", name)?,
            },
            "TASKTIME" => Instr::Time,
            "TASKSOUND" => Instr::Sound { sample: a(1) },
            "TASKMOVE" => Instr::Move {
                flags: a(1),
                x: i16_at(img, at + 2),
                y: i16_at(img, at + 4),
                z: i16_at(img, at + 6),
            },
            "TASKSHADOW" => Instr::Shadow {
                on: a(1) != 0,
                script: script_target(syms, w(2), "TASKSHADOW", name)?,
            },
            "TASKSAVE" => Instr::Save { mode: a(1), field: i16_at(img, at + 2), value: w(4) },
            "TASKGOSUB" => Instr::Gosub {
                routine: syms.routine(w(2)).map(str::to_string).ok_or_else(|| {
                    anyhow::anyhow!("{name}: TASKGOSUB {:#06x} is not a routine entry", w(2))
                })?,
            },
            "TASKDEAD" => Instr::Dead {
                target: script_target(syms, w(2), "TASKDEAD", name)?,
            },
            "TASKADDTASK" => Instr::AddTask {
                target: script_target(syms, w(2), "TASKADDTASK", name)?,
            },
            "TASKKILLTASK" => Instr::KillTask,
            "TASKCELBUF" => Instr::CelBuf { table: a(1) },
            "TASKTESTEQ" => Instr::TestEq {
                mode: a(1),
                field: i16_at(img, at + 2),
                target: script_target(syms, w(4), "TASKTESTEQ", name)?,
            },
            "TASKTESTNE" => Instr::TestNe {
                mode: a(1),
                field: i16_at(img, at + 2),
                target: script_target(syms, w(4), "TASKTESTNE", name)?,
            },
            "TASKANIMCLR" => Instr::AnimClr,
            other => anyhow::bail!("{name}: no reader for command {other}"),
        };
        code.push(instr);
        anyhow::ensure!(cmd.len > 0, "{name}: {} consumes nothing and would spin", cmd.name);
        pc += cmd.len as u32;
    }
    anyhow::bail!("{name}: does not terminate on ff ff within 4096 instructions")
}

/// What came out of a whole-image parse, for the baker to print.
#[derive(Debug, Default)]
pub struct Report {
    pub scripts: usize,
    pub parts: usize,
    pub commands: usize,
}

/// Parse every animation script in the image.
///
/// Also checks the set is closed: every `TASKGOTO`, `TASKDEAD`, `TASKSKIP`,
/// `TASKADDTASK` and `TASKSHADOW` target has to be one of the scripts exported
/// alongside it, or an animation would run into a name nothing defines.
pub fn all_scripts(img: &[u8], syms: &Symbols) -> anyhow::Result<(ScriptSet, Report)> {
    let ops = opcode_set(img, syms)?;
    let mut out = ScriptSet::new();
    let mut report = Report::default();
    for (name, off) in syms.scripts() {
        let s = parse_script(img, syms, &ops, &name, off)?;
        for i in &s.code {
            match i {
                Instr::Part(_) => report.parts += 1,
                Instr::EndFrame { .. } => {}
                _ => report.commands += 1,
            }
        }
        out.insert(name, s);
        report.scripts += 1;
    }

    let names: BTreeSet<&String> = out.keys().collect();
    for (name, s) in &out {
        for i in &s.code {
            let target = match i {
                Instr::Goto { target, .. }
                | Instr::Skip { target }
                | Instr::Dead { target }
                | Instr::AddTask { target }
                | Instr::TestEq { target, .. }
                | Instr::TestNe { target, .. } => target,
                Instr::Shadow { script, .. } => script,
                _ => continue,
            };
            anyhow::ensure!(
                target.is_empty() || names.contains(target),
                "{name} branches to {target}, which is not one of the exported scripts"
            );
        }
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_names_are_the_ones_with_an_encounter_prefix() {
        assert!(is_script_name("Knight_SwSwing"));
        assert!(is_script_name("TroggAxe_Walk1"), "Trogg* is a family of prefixes");
        assert!(is_script_name("Balok_Blink"));
        assert!(!is_script_name("KnightGruntSound"), "a routine, not a script");
        assert!(!is_script_name("Knight"), "no underscore");
        assert!(!is_script_name("CelFile1"));
    }

    #[test]
    fn the_shift_map_is_applied_from_the_last_range_that_starts_below() {
        let s = Symbols {
            code: BTreeMap::new(),
            shifts: vec![(100, 200, 0), (300, 400, -12)],
            data: BTreeMap::new(),
        };
        assert_eq!(s.to_image(150), 150);
        assert_eq!(s.to_image(350), 338);
        assert_eq!(s.to_image(250), 250, "a gap keeps the range before it");
        assert_eq!(s.to_image(50), 50, "below every range, no correction");
    }
}
