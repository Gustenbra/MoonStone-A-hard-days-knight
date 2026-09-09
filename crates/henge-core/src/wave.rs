//! How many creatures a fight holds, how many it holds *at once*, and when the
//! next one walks in.
//!
//! Build order item 81. Every part of this is a named routine or a named word in
//! `MOON` and all of it reads cleanly, so it is transcription rather than
//! design. Four words of BSS carry the whole thing:
//!
//! ```text
//! DS:0x96a  TotalMonsters    how many more are still owed to the fight
//! DS:0x96c  MaxMonsters      how many may stand on the screen at one time
//! DS:0x96e  NumberInCombat   how many are standing on it now
//! DS:0x970  SIDE             which side of the screen the next one comes from
//! DS:0x972  INITMO           the routine that puts one more in
//! DS:0x974  INITANIM         the creature's own `Set*Tables`
//! DS:0xa68  AddCNT           the standing-depth rotation, see `arena::Arrivals`
//! ```
//!
//! and five routines move them:
//!
//! * **`InitKnightvs*`** writes the three counts for the kind of fight it is
//!   setting up. `InitKnightvsTroggAxe` (image `0x20b0`) is the pattern:
//!   `MaxMonsters` 1, `TotalMonsters` 3, `NumberInCombat` 0, one more on
//!   `TotalMonsters` when four people are playing (`cmp [0x91e], 4` at
//!   `0x20cb`, and `0x91e` is the player count `Adjplayers` clamps to 1..4),
//!   then `INITMO` and `INITANIM`.
//! * **`AdjustLevel`** (`0x2824`) moves all of it about by what the knight has
//!   become, and for a lair replaces `TotalMonsters` outright with the lair
//!   record's own `+4` (`0x287e`). See [`Wave::open`].
//! * **`SetMonsterCombat`** (`0x27e4`) is `cx = [MaxMonsters]` passes of
//!   `InitNewMO`, so a fight opens with as many on the screen as it may hold.
//! * **`InitNewMO`** (`0x27ee`) adds one to `NumberInCombat`, copies x, y, z and
//!   facing out of an eight-byte seat record, calls `INITANIM` for the stat
//!   block and `AddPlayer` for the standing depth.
//! * **`CountTheDead`** (`0x213`) is what every creature's own death script
//!   calls through `TASKGOSUB`, and it is the whole of the wave logic:
//!
//! ```text
//! 00213  mov  si, [KnightTable]      ; the player's own actor record
//! 00217  mov  ax, [si+0x38]          ; his hit points
//! 0021a  or   ax, ax
//! 0021c  jle  StopCombat             ; he is down: the fight is over
//! 0021e  sub  word [NumberInCombat], 1
//! 00223  sub  word [TotalMonsters], 1
//! 00228  jg   CountDone              ; more are still owed: top the screen up
//! 0022a  cmp  word [NumberInCombat], 0
//! 0022f  jne  CountDone              ; none owed, but some still standing
//! 00231 StopCombat:
//!        mov  byte [0x8987], 0x23    ; thirty five more frames, then out
//!        mov  byte [0x897d], 0       ; and the combat flag down
//! 00243 CountDone:
//!        mov  ax, [MaxMonsters]
//!        cmp  ax, [NumberInCombat]
//!        je   ret                    ; the screen is full
//!        mov  si, [INITMO]
//!        cmp  word [TotalMonsters], 0
//!        jle  ret                    ; nothing left to send
//!        call si                     ; one more walks in
//!        jmp  CountDone              ; and again, until one or the other
//! ```
//!
//! Two things fall out of that which are worth stating plainly, because they
//! are what a player sees.
//!
//! **A fight is one creature at a time for almost everything.** `MaxMonsters`
//! is 1 in every `InitKnightvs*` but the ratmen's, which is 2, so a lair of
//! fourteen troggs is fourteen troggs fought one after another and not fourteen
//! troggs at once. The next one walks in on the frame the last one's death
//! script reaches its `CountTheDead`, from the side `SIDE` says, at the depth
//! `AddCNT` hands out.
//!
//! **`TotalMonsters` is not the head count.** It is how many more are owed, and
//! `CountDone` sends one for each death that leaves it above zero, so a fight
//! set up with `max` on the screen and `total` owed puts `max + total - 1`
//! creatures in front of you altogether. For the ratmen's 2 and 2 that is
//! three; for a lair's `max` of 1 and `total` of 14 it is fourteen.
//!
//! Nothing here reads a clock or rolls anything. Which seat an arrival takes is
//! `SIDE`, which is one bit flipped per arrival; what depth it stands at is
//! `AddCNT`, which is [`crate::arena::Arrivals`]. Both are deterministic from
//! the state the bout already carries.

use serde::{Deserialize, Serialize};

/// What one creature's `InitKnightvs*` and its row of `lev_adjust` say, carried
/// on the actor definition so a pack decides rather than a match on an id.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WaveDef {
    /// `MaxMonsters`, DS:`0x96c`, as this creature's `InitKnightvs*` writes it:
    /// 1 for everything but the ratmen, which is 2. Zero means the pack has
    /// nothing to say about this actor, and a caller must leave its own seat
    /// count alone rather than open a wave on it: `CountDone`'s first test is
    /// `cmp ax, [NumberInCombat] / je`, so a `MaxMonsters` of nought sends
    /// nothing and nothing ever ends the fight.
    pub max: i32,
    /// `TotalMonsters`, DS:`0x96a`, the same way: 3 for the three troggs and
    /// the beast, 2 for the ratmen, Balok and the mudmen, 1 for the troll, the
    /// demon and the dragon.
    pub heads: i32,
    /// What `AdjustLevel` will not let `MaxMonsters` past for this creature,
    /// or zero for no ceiling. Recovered, and there are exactly three:
    /// `0x2882` and `0x2890` force it back to 1 when `INITMO` is `InitBalok`
    /// or `InitMudmen`, and `0x289e` holds it at 2 when `INITANIM` is
    /// `SetTrollTable`.
    pub cap: i32,
    /// Whether this creature's `INITMO` flips `SIDE`. `InitTrogg` (`0x225b`),
    /// `InitBeast` (`0x22d2`), `InitRatmen` (`0x239e`) and `InitMudmen`
    /// (`0x2655`) all open `xor word [SIDE], 1`; `InitBalok` (`0x25cf`) does
    /// not, and every Balok comes in at the same seat.
    pub alternates: bool,
    /// Whether `INITMO` puts anything in at all. The knight's, the demon's and
    /// the dragon's is `0x2059`, which is one `ret`, so those three fights have
    /// no reinforcements whatever `TotalMonsters` says.
    pub reinforced: bool,
    /// Whether the fight's *opening* goes through `INITMO` rather than through
    /// `SetMonsterCombat`. `InitKnightvsMudmen` (`0x264c`) and
    /// `InitKnightvsTroll` (`0x26f4`) call `INITMO` directly instead of
    /// `SetMonsterCombat`, so the first one in is chosen by `SIDE` like every
    /// later one and comes in from the far side of the screen.
    pub opens_with_side: bool,
    /// This creature's row of `lev_adjust`, DS:`0xa0a`, eight signed bytes.
    ///
    /// `AdjustLevel` finds the row by looking `INITANIM` up in `KLTAB`
    /// (DS:`0xa4c`, eight words), whose order is `SetBalokTables`,
    /// `SetRatmenTables`, `SetTroggAxeTables`, `SetTroggHammerTables`,
    /// `SetTroggSpTables`, `SetUpMudmenTables`, `SetTrollTable`,
    /// `SetBeastTables`; the row is subtracted from `TotalMonsters`. The
    /// entries fall from positive to negative across a row, so a weak knight
    /// is sent fewer and a strong one more. Empty for a creature `KLTAB` does
    /// not name, which is what the lookup's fall-through `ret` at `0x28f9`
    /// does.
    pub level: Vec<i32>,
}

/// What `AdjustLevel` reads off the knight before it decides anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Level {
    /// `+0x2e`, the knight's strength: `SetKnightEquipment` writes 1 and
    /// `CheckMaxAbility` will not take it past 5.
    pub strength: i32,
    /// `+0x3c`, his experience.
    pub experience: i32,
    /// What `CalcDamage` (`0x2d67`) returns for him with `[si+0x28]` set to 4,
    /// which is the swing: the swing's own `*Dam` entry, plus strength, plus
    /// what the blade in his hand adds.
    pub swing: i32,
    /// How many people are at the keyboard. `[0x91e]`, which `Adjplayers`
    /// clamps to 1..4; four of them is worth one more creature.
    pub players: i32,
}

/// The state of one fight's reinforcements: the original's four words, plus
/// what an arrival is fielded with.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Wave {
    /// `TotalMonsters`, DS:`0x96a`. How many more are owed, not how many there
    /// are; see the module note.
    pub total: i32,
    /// `MaxMonsters`, DS:`0x96c`.
    pub max: i32,
    /// `NumberInCombat`, DS:`0x96e`.
    pub in_combat: i32,
    /// `SIDE`, DS:`0x970`. `SetUpDKL` (`0x292e`) zeroes it when a fight is set
    /// up, so it belongs to the bout and not to the session.
    pub side: i32,
    /// What an arrival is fielded with, once the moon and this engine's own
    /// scale have had their say on what `INITANIM` writes. The bout cannot work
    /// either out for itself, so the caller that sets the fight up leaves them
    /// here. Zero means take the definition's own.
    pub health: i32,
    pub damage: i32,
    /// Whether `INITMO` is a real routine, copied off [`WaveDef::reinforced`]
    /// so the bout needs no definition to answer [`Wave::owed`].
    pub reinforced: bool,
    /// `RegenerateFLAG`, DS:`0xa4a`, which `AdjustLevel` is the only writer of:
    /// 0 under thirty experience, then 1, 2 and 3 at thirty, sixty and ninety.
    /// Carried because the routine writes it; nothing in this engine reads it
    /// yet.
    pub regenerate: i32,
}

impl Wave {
    /// `InitKnightvs*` and then `AdjustLevel`, in that order, which is the order
    /// every one of the thirteen calls them in.
    ///
    /// `lair` is the head count a lair hands over: `AdjustLevel` at `0x286f`
    /// tests the place kind and, for a lair, replaces `TotalMonsters` with
    /// `[si+4]` of the lair record, which is `ForestLairs`'s own second word.
    pub fn open(def: &WaveDef, lair: Option<i32>, k: &Level) -> Wave {
        // ---- The `InitKnightvs*` numbers. `InitKnightvsTroggAxe`, 0x20b9.
        let mut max = def.max;
        let mut total = def.heads;
        // 020cb  cmp word [0x91e], 4 / jne +3 / inc word [TotalMonsters]
        if k.players == 4 {
            total += 1;
        }

        // ---- AdjustLevel, 0x2824.
        // 02824  mov word [RegenerateFLAG], 0
        let mut regenerate = 0;
        // 02831  cmp byte [si+0x2e], 3 / jle +5 / add word [MaxMonsters], 1
        if k.strength > 3 {
            max += 1;
        }
        // 0283c  cmp word [si+0x3c], 0x1e / jl +11
        if k.experience >= 30 {
            total += 1;
            regenerate = 1;
        }
        // 0284d  cmp word [si+0x3c], 0x3c / jl +11
        if k.experience >= 60 {
            max += 1;
            regenerate = 2;
        }
        // 0285e  cmp word [si+0x3c], 0x5a / jl +11
        if k.experience >= 90 {
            regenerate = 3;
            total += 1;
        }
        // 0286f  cmp word [0x898d], 2 / jne +12: in a lair the record's own
        // head count replaces everything decided above.
        if let Some(n) = lair {
            total = n;
        }
        // 02882, 02890 and 0289e: the three ceilings. Balok's and the mudmen's
        // are `mov [MaxMonsters], 1` outright and the troll's is a compare
        // first, which is the same thing while nothing above lowers it.
        if def.cap > 0 && max > def.cap {
            max = def.cap;
        }
        let mut wave = Wave {
            total,
            max,
            in_combat: 0,
            side: 0,
            health: 0,
            damage: 0,
            reinforced: def.reinforced,
            regenerate,
        };
        // 028b3  cmp word [TotalMonsters], 0 / jle DDON: nothing more is done
        // to a fight that owes nothing.
        if wave.total <= 0 {
            return wave;
        }
        // 028c3  CalcDamage with the swing's kind in `+0x28`, then the level.
        let mut ax = k.swing + (k.experience >> 2);
        // 028cf  shr ax, 1 / sub ax, 6 / jns: floored at nothing.
        ax >>= 1;
        ax -= 6;
        if ax < 0 {
            ax = 0;
        }
        // 028d9  cmp ax, 0x10 / jl: and held at fifteen.
        if ax >= 0x10 {
            ax = 0xf;
        }
        // 028e1  shr ax, 1: so the level is nought to seven.
        ax >>= 1;
        // 028e3  the KLTAB walk, and 0x290b the byte out of lev_adjust.
        // A creature KLTAB does not name returns at 0x28f9 with nothing done.
        if let Some(adjust) = def.level.get(ax as usize) {
            // 0290f  mov bx, [TotalMonsters] / sub bx, ax / jle DDON: a row
            // that would take the fight to nothing leaves it as it was.
            let bx = wave.total - adjust;
            if bx > 0 {
                wave.total = bx;
            }
        }
        wave
    }

    /// Whether anything is still owed to this fight, which is
    /// `cmp word [TotalMonsters], 0 / jle` in `CountDone` with `INITMO`
    /// checked first: a fight whose `INITMO` is the bare `ret` at `0x2059`
    /// owes nothing whatever the count says.
    pub fn owed(&self) -> bool {
        self.reinforced && self.total > 0
    }

    /// `SetMonsterCombat` (`0x27e4`), as a seat index per pass: `cx =
    /// [MaxMonsters]` passes of `InitNewMO`, the record pointer walking eight
    /// bytes on each time, so the opening takes records 0, 1, 2 in order.
    ///
    /// The troll's and the mudmen's fights open through `INITMO` instead
    /// ([`WaveDef::opens_with_side`]), so for those the opening is chosen by
    /// `SIDE` exactly as a later arrival is.
    pub fn opening_seat(&mut self, def: &WaveDef, pass: usize) -> usize {
        if def.opens_with_side {
            self.next_seat(def)
        } else {
            pass
        }
    }

    /// The `xor word [SIDE], 1` that opens `InitTrogg`, `InitBeast`,
    /// `InitRatmen` and `InitMudmen`, and the `je` after it: a zero result
    /// keeps the table's own base, anything else steps eight bytes on. So the
    /// seats go 1, 0, 1, 0 from a `SIDE` of nought, and a creature comes in
    /// from the other side of the screen each time.
    ///
    /// `InitBalok` has no such flip and always takes record 0.
    pub fn next_seat(&mut self, def: &WaveDef) -> usize {
        if !def.alternates {
            return 0;
        }
        self.side ^= 1;
        if self.side != 0 {
            1
        } else {
            0
        }
    }

    /// `CountTheDead` (`0x213`) from `sub word [NumberInCombat], 1` onwards:
    /// one creature's death script has reached its `TASKGOSUB`.
    ///
    /// Answers how many walk in behind it, which is how many passes
    /// `CountDone`'s loop makes. The loop's own test is
    /// `cmp ax, [NumberInCombat] / je`, written here as a `<` because
    /// `NumberInCombat` is never above `MaxMonsters` and an `!=` on a counter
    /// that only climbs would spin if it ever were.
    ///
    /// The count itself is not moved here. Each pass of the loop calls
    /// `INITMO`, and it is `InitNewMO` at `0x27ee` that does
    /// `add word [NumberInCombat], 1`; the caller that actually fields the
    /// creature is where that belongs.
    pub fn dead(&mut self) -> i32 {
        // 0021e  sub word [NumberInCombat], 1
        self.in_combat -= 1;
        // 00223  sub word [TotalMonsters], 1 / jg CountDone
        self.total -= 1;
        if self.total <= 0 {
            // 0022a  cmp word [NumberInCombat], 0 / jne CountDone, else
            // StopCombat. Either way nothing more walks in, because
            // `CountDone`'s own test on `TotalMonsters` would refuse it.
            return 0;
        }
        // 00243 CountDone.
        let mut sent = 0;
        while self.in_combat + sent < self.max && self.owed() {
            sent += 1;
        }
        sent
    }

    /// `InitNewMO`'s own first instruction, `add word [NumberInCombat], 1` at
    /// `0x27ee`, for the caller that fields one.
    pub fn arrived(&mut self) {
        self.in_combat += 1;
    }

    /// `StopCombat`'s own condition, read off the two counts: nothing owed and
    /// nothing standing. Asked by the bout rather than written as a flag,
    /// because the bout has its own account of who is still on their feet.
    pub fn done(&self) -> bool {
        !self.owed() && self.in_combat <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `InitKnightvsTroggAxe`: `MaxMonsters` 1, `TotalMonsters` 3, and row 2 of
    /// `lev_adjust`, which is `5 4 2 0 0 -1 -3 -4`.
    fn trogg() -> WaveDef {
        WaveDef {
            max: 1,
            heads: 3,
            cap: 0,
            alternates: true,
            reinforced: true,
            opens_with_side: false,
            level: vec![5, 4, 2, 0, 0, -1, -3, -4],
        }
    }

    /// `InitKnightvsRatmen`: two at a time and two owed, row 1,
    /// `5 4 3 2 0 -1 -2 -4`.
    fn ratmen() -> WaveDef {
        WaveDef {
            max: 2,
            heads: 2,
            cap: 0,
            alternates: true,
            reinforced: true,
            opens_with_side: false,
            level: vec![5, 4, 3, 2, 0, -1, -2, -4],
        }
    }

    /// A fresh knight: strength one, no experience, a four point swing plus the
    /// one his strength adds.
    fn fresh() -> Level {
        Level { strength: 1, experience: 0, swing: 5, players: 1 }
    }

    #[test]
    fn a_lair_head_count_replaces_everything_the_routine_decided() {
        // 0x287e: the lair record's own `+4` written straight over
        // `TotalMonsters`, after the experience has already added to it.
        let k = Level { strength: 1, experience: 95, swing: 5, players: 4 };
        let w = Wave::open(&trogg(), Some(14), &k);
        // The level is (5 + 95/4) / 2 - 6 = 8, held under sixteen, then halved:
        // row entry 4, which is 0, so nothing comes off the fourteen.
        assert_eq!(w.total, 14);
        // And the experience still moved `MaxMonsters`: 1 + 1 for sixty.
        assert_eq!(w.max, 2);
        assert_eq!(w.regenerate, 3);
    }

    #[test]
    fn a_fresh_knight_is_sent_fewer_and_a_veteran_more() {
        // Row 2 at level 0 is 5, and `sub bx, ax; jle` refuses a count that
        // would go to nothing, so three owed stays three.
        let w = Wave::open(&trogg(), None, &fresh());
        assert_eq!(w.total, 3, "5 off 3 is not positive, so the count stands");
        // A knight who swings for twenty with ninety experience: the level is
        // (20 + 22) / 2 - 6 = 15, held at fifteen, halved to 7, and row 2's
        // last entry is -4, so four more come.
        let k = Level { strength: 5, experience: 90, swing: 20, players: 1 };
        let w = Wave::open(&trogg(), None, &k);
        // 3 owed, +1 at thirty, +1 at ninety, then -(-4).
        assert_eq!(w.total, 3 + 1 + 1 + 4);
        // Strength above three and sixty experience each add one to the screen.
        assert_eq!(w.max, 3);
    }

    #[test]
    fn four_players_are_worth_one_more_creature() {
        let one = Wave::open(&trogg(), None, &fresh());
        let four = Wave::open(&trogg(), None, &Level { players: 4, ..fresh() });
        // `cmp [0x91e], 4`: three players is not four.
        let three = Wave::open(&trogg(), None, &Level { players: 3, ..fresh() });
        assert_eq!(one.total, three.total);
        // The fourth head is added before the row is subtracted, and row 2 at
        // level 0 is 5, which `jle` refuses against 4 as well.
        assert_eq!(four.total, 4);
    }

    #[test]
    fn the_three_ceilings_hold_max_monsters_down() {
        let strong = Level { strength: 5, experience: 60, swing: 8, players: 1 };
        // Nothing capped: 1 + 1 + 1.
        assert_eq!(Wave::open(&trogg(), None, &strong).max, 3);
        // The troll's `cmp [MaxMonsters], 2 / jle` at 0x289e.
        let troll = WaveDef { cap: 2, heads: 1, ..trogg() };
        assert_eq!(Wave::open(&troll, None, &strong).max, 2);
        // Balok's and the mudmen's `mov [MaxMonsters], 1` at 0x288a and 0x2898.
        let balok = WaveDef { cap: 1, heads: 2, alternates: false, ..trogg() };
        assert_eq!(Wave::open(&balok, None, &strong).max, 1);
    }

    /// The whole of `CountTheDead` and `CountDone`, counted out: a lair of
    /// fourteen with one on the screen at a time puts them in front of you one
    /// at a time, each on the death of the last, and the fight ends when the
    /// last of them falls.
    ///
    /// The count a lair is fought at is not its table entry: the lair's own
    /// fourteen is written over `TotalMonsters` at `0x287e` and then the
    /// `lev_adjust` row is taken off it at `0x2913`, so a knight fresh off the
    /// select screen raids a lair of fourteen troggs and meets nine of them.
    /// A knight the row gives nothing to meets all fourteen.
    #[test]
    fn a_lair_of_fourteen_is_fought_one_at_a_time_to_the_end() {
        // Fight it out, and answer how many arrived and how many died.
        let run = |mut w: Wave| {
            // `SetMonsterCombat`: `MaxMonsters` passes of `InitNewMO` to open.
            for _ in 0..w.max {
                w.arrived();
            }
            let mut arrived = w.in_combat;
            let mut killed = 0;
            while !w.done() {
                let sent = w.dead();
                for _ in 0..sent {
                    w.arrived();
                }
                arrived += sent;
                killed += 1;
                assert!(killed < 100, "the count has to run out");
            }
            assert_eq!(w.in_combat, 0);
            (arrived, killed)
        };
        // Fresh: level 0, and row 2's first entry is 5.
        let w = Wave::open(&trogg(), Some(14), &fresh());
        assert_eq!((w.max, w.total), (1, 9));
        assert_eq!(run(w), (9, 9));
        // A swing of 28 puts the level at (28 >> 1) - 6 = 8, halved to 4, and
        // row 2's fifth entry is nought, so the whole fourteen come.
        let middling = Level { swing: 28, ..fresh() };
        let w = Wave::open(&trogg(), Some(14), &middling);
        assert_eq!((w.max, w.total), (1, 14));
        assert_eq!(run(w), (14, 14), "fourteen walked in and fourteen fell");
    }

    /// `max + total - 1`, which is what the two counts really come to, shown on
    /// the ratmen's own 2 and 2.
    #[test]
    fn two_at_a_time_and_two_owed_is_three_ratmen() {
        let mut w = Wave::open(&ratmen(), None, &fresh());
        assert_eq!((w.max, w.total), (2, 2));
        for _ in 0..w.max {
            w.arrived();
        }
        let mut arrived = w.in_combat;
        let mut killed = 0;
        while !w.done() {
            let sent = w.dead();
            for _ in 0..sent {
                w.arrived();
            }
            arrived += sent;
            killed += 1;
            assert!(killed < 100);
        }
        assert_eq!((killed, arrived), (3, 3));
    }

    #[test]
    fn a_fight_with_no_init_mo_never_tops_itself_up() {
        // The demon's, the dragon's and a knight's `INITMO` is the `ret` at
        // 0x2059.
        let def = WaveDef { max: 1, heads: 1, reinforced: false, ..trogg() };
        let mut w = Wave::open(&def, Some(9), &fresh());
        assert!(!w.owed(), "nine owed and nothing to send them");
        w.arrived();
        assert_eq!(w.dead(), 0);
        assert!(w.done());
    }

    #[test]
    fn side_alternates_and_balok_does_not() {
        let t = trogg();
        let mut w = Wave::default();
        // `SetUpDKL` leaves `SIDE` at nought, so the first flip gives one.
        let seats: Vec<usize> = (0..5).map(|_| w.next_seat(&t)).collect();
        assert_eq!(seats, vec![1, 0, 1, 0, 1]);
        let balok = WaveDef { alternates: false, ..t.clone() };
        let mut w = Wave::default();
        assert_eq!((0..3).map(|_| w.next_seat(&balok)).collect::<Vec<_>>(), vec![0, 0, 0]);
        // The opening walks the table from the front instead, unless the fight
        // opens through `INITMO`.
        let mut w = Wave::default();
        assert_eq!((0..3).map(|i| w.opening_seat(&t, i)).collect::<Vec<_>>(), vec![0, 1, 2]);
        let mud = WaveDef { opens_with_side: true, ..t };
        let mut w = Wave::default();
        assert_eq!(w.opening_seat(&mud, 0), 1, "the mudmen open on the far side");
    }
}
