//! What the tavern, the town healer, the temple, the mystic, the wizard and
//! the stone circle do to a run.
//!
//! Six doors that were menus answering with a line of text. All six are now the
//! original's own arithmetic, because `_TAVERN` and `_WIZARD` are two whole
//! source modules with addresses, the temple is one routine in `_STATUS`, and
//! the circle is `MOON:Henge`.
//!
//! Everything here is pure: it takes a [`Run`], changes it, and answers what it
//! did. Nothing rolls from the clock; every roll comes out of the run's own
//! seeded generator, so two machines that walk into the same tavern on the same
//! day throw the same dice.
//!
//! **How the original rolls.** One generator serves all of it, at image 0xbd89:
//! a sixteen bit word, eight rounds of rotate-and-exclusive-or, seeded from the
//! BIOS tick counter. On top of it sits a helper at 0xbda9 that masks to 0x7f
//! and folds anything from 100 up down by 27, giving a number in 0..=100 with a
//! slight lean towards the seventies. `henge` keeps the *ranges and the
//! thresholds*, which are what the game is made of, and takes the bits from the
//! run's own generator, because the original's is seeded from the wall clock
//! and this simulation may not be.

use crate::item::{Items, Virtue};
use crate::knight::Ability;
use crate::moon::{is_token, Moonstone};
use crate::run::{Run, LIFE_CEILING, WIZARD_GRUDGE};

// ---------------------------------------------------------------- the tavern

/// How many dice are thrown. `mov cx, 3` in `_TAVERN:RollDice`.
pub const DICE: usize = 3;

/// The faces, zero based. `and ax, 7` then `cmp ax, 5; jg` rolls again, so a
/// die lands on 0..=5 and the cel of `DICE.CEL` it draws is that number.
pub const FACES: u32 = 6;

/// The stakes the table takes. The tavern's six gadgets are five bets, with
/// `[si+0x10]` of 1 to 5, and an exit, and `TAV.PIV` paints them as `1 gold`
/// to `5 gold`.
pub const STAKES: [u32; 5] = [1, 2, 3, 4, 5];

/// The paying throws, and what each pays.
///
/// **Recovered.** `_TAVERN:DiceODDS` at DS:d125, eleven records six bytes
/// apart. The first four bytes are the three sorted dice and a pad, which
/// `RollDice` compares against the sorted throw two words at a time; the byte
/// at +4 is the multiplier, which `DiceWinner` loads into `bl` and multiplies
/// the stake by. The faces are written as the original stores them, zero based,
/// so `[0, 0, 0]` is three of the first face.
///
/// The order of the triples is the table's own and is not monotone in the face:
/// three of face 0 pays thirty, three of face 1 twenty, three of face 5
/// eighteen, three of face 3 sixteen, three of face 4 fourteen and three of
/// face 2 twelve. Nothing pays for a pair unless the pair is face 0.
pub const ODDS: [([u8; DICE], u32); 11] = [
    ([0, 0, 1], 4),
    ([0, 0, 2], 5),
    ([0, 0, 4], 6),
    ([0, 0, 3], 8),
    ([0, 0, 5], 10),
    ([2, 2, 2], 12),
    ([4, 4, 4], 14),
    ([3, 3, 3], 16),
    ([5, 5, 5], 18),
    ([1, 1, 1], 20),
    ([0, 0, 0], 30),
];

/// The most gold a knight can hold. `cmp word ptr [si+0x32], 0x96` then
/// `mov word ptr [si+0x32], 0x96`, in the dice winner's payout, in the wizard's
/// gift of gold and in the temple's buying. A hundred and fifty, and the purse
/// saturates there.
pub const PURSE_CEILING: u32 = 150;

/// One throw of the three dice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Throw {
    /// The faces, sorted ascending, zero based. `_TAVERN:DiceSort` is a bubble
    /// sort over the three bytes, so the odds are matched against a sorted
    /// throw and the order they landed in does not matter.
    pub dice: [u8; DICE],
    /// The stake, gone from the purse before the dice were thrown.
    pub stake: u32,
    /// What came back. Zero is a loss.
    pub won: u32,
}

impl Throw {
    pub fn winner(&self) -> bool {
        self.won > 0
    }

    /// The multiplier this throw pays, or none.
    pub fn odds(&self) -> Option<u32> {
        ODDS.iter().find(|(d, _)| *d == self.dice).map(|(_, m)| *m)
    }

    /// What the table says. `WIN` and `LOST` at DS:cd7a and cd70, with
    /// `BETWINPOT`, `BETLOSTPOT` and `GOLDTOTAL` filled in.
    pub fn describe(&self, purse: u32) -> String {
        if self.winner() {
            format!("You won {} gold pieces. You now have {} gold pieces.", self.won, purse)
        } else {
            format!("You lost {} gold pieces. You now have {} gold pieces.", self.stake, purse)
        }
    }
}

/// What came of walking up to the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wager {
    Threw(Throw),
    /// The purse will not cover the stake. `SetBET` compares the bet against
    /// the purse and refuses rather than going into debt.
    TooPoor,
    /// The tavern turns an empty purse round at the door: `cmp word ptr
    /// [si+0x32], 0; jg` then `ret`, before anything is drawn.
    Skint,
}

// ------------------------------------------------------------- the town healer

/// What a donation at the town healer bought.
///
/// `HEA.PIV`, the routine at image 0xba66, behind the `Healer` door of both
/// cities. It is not a roll: the pot is spent down in a loop, and each pass
/// buys the next thing that is worth buying.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Healing {
    /// The ratman's bite cleared. Costs nothing: the loop clears the flag
    /// before it looks at the pot, and only asks that there be ten in it.
    pub unbitten: bool,
    /// Wounds closed. Ten a time, and it writes the ceiling into the health
    /// rather than adding to it.
    pub healed: bool,
    /// Life points bought, fifteen each, to a ceiling of five.
    pub lives: u32,
    /// What was left in the pot. Donated, not returned: `OkDonation` writes the
    /// reduced purse back to the knight before the loop ever runs.
    pub unspent: i32,
    /// The donation, for choosing what the healer says about it.
    pub gave: u32,
}

impl Healing {
    pub fn anything(&self) -> bool {
        self.unbitten || self.healed || self.lives > 0
    }

    /// `ExitHealer`: nine or less buys roots and herbal tea whatever it did;
    /// otherwise `Heal5a`, or `Heal6a` if the bite came off.
    pub fn describe(&self) -> &'static str {
        if self.gave <= 9 {
            "For that amount of coin the best I can offer you is some roots and herbal tea"
        } else if self.unbitten {
            "You have been healed."
        } else {
            "You are healed of your wounds"
        }
    }
}

/// What ten in the pot buys. `cmp ax, 0xa; jl ExitHealer`, then
/// `sub word ptr [DONATION], 0xa` for the healing.
pub const HEALER_MEND: i32 = 10;

/// And what a life point costs. `sub word ptr [DONATION], 0xf`. The loop only
/// asks for ten in the pot before it buys one, so the last point can go for
/// ten and leave the pot negative; that is what the code does and it is kept.
pub const HEALER_LIFE: i32 = 15;

// ---------------------------------------------------------------- the temple

/// What the temple paid for something. `GoldSell`: the item's price shifted
/// right once, into a purse that saturates at a hundred and fifty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sale {
    Sold { paid: u32 },
    /// You are carrying none.
    HaveNone,
    /// The temple deals in magic, not in keys, moonstones, or what you wear.
    NotWanted,
    /// No such item in the packs.
    Unknown,
}

// ---------------------------------------------------------------- the mystic

/// What the cosmos decided.
///
/// `MYS.PIV`, the routine at image 0xb935, behind the `Mystic` door of
/// Waterdeep. A donation, a roll, and either a point of an ability or the loss
/// of one. The mystic is the only door in the game that can take something
/// off you for money.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// A point given.
    Granted(Ability),
    /// A point taken. `MysticAbility` refuses to go below one and says the
    /// cosmos was too weak to reach instead.
    Taken(Ability),
    /// Every ability already at five: `My6a`.
    Maxed,
    /// The bad roll landed on an ability already at its floor: `My7a`.
    Weak,
    /// Nothing was put in the pot: `Heal3a`.
    NoDonation,
    /// Not enough coin to make the donation offered.
    TooPoor,
}

impl Reading {
    /// The original's own lines, `MY3a`..`MY7b` and the granted set at
    /// DS:df18..dfd7. One quirk is not reproduced: `MysticJudge` looks a
    /// raised or lowered ability up in a table with `cx` of two, so the
    /// third entry, endurance, is never found and the original says `My
    /// powers are weak` after quietly granting it. The endurance lines exist
    /// and are used.
    pub fn describe(self) -> &'static str {
        match self {
            Reading::Granted(Ability::Strength) => "The cosmos has granted you more strength.",
            Reading::Granted(Ability::Constitution) => {
                "I see many harsh physical hardships in your future. The cosmos has granted you more constitution"
            }
            Reading::Granted(Ability::Endurance) => {
                "I see many long paths that you will need to follow. I will grant you more endurance."
            }
            Reading::Taken(Ability::Strength) => {
                "The cosmos has been hard on your soul and has stolen some of your strength."
            }
            Reading::Taken(Ability::Constitution) => {
                "Your soul has taken much punishment within the cosmos. You have lost some of your constitution."
            }
            Reading::Taken(Ability::Endurance) => {
                "The cosmos has imprisoned part of your soul thus stealing away part of your endurance."
            }
            Reading::Maxed => {
                "You have already reached skills and agilities that even I can no longer raise or improve upon.  Now go and complete your quest before the Black Knights fulfill their treachery."
            }
            Reading::Weak => {
                "My powers are weak right now and I was unable to reach the cosmos.  Perhaps later in the week I may be of help."
            }
            Reading::NoDonation => "Sorry I cannot be of help.  Come back anytime",
            Reading::TooPoor => "May Danu aid you in you quest",
        }
    }
}

/// How a donation shifts the mystic's roll.
///
/// **Recovered.** `_WIZARD:DonationTAB` at DS:e478, six records of a threshold
/// and a signed delta. `MysticUpDown` rolls 0..=100, walks the table for the
/// first threshold the donation does not exceed, adds that delta, and calls the
/// result good at fifty or under. So a bigger donation subtracts from the roll
/// and buys better odds: nine coins or fewer adds twenty and leaves about three
/// chances in ten, fifty or more subtracts thirty and leaves about eight.
pub const DONATION_TABLE: [(u32, i32); 6] =
    [(9, 20), (19, 10), (29, 0), (39, -10), (49, -20), (250, -30)];

/// The roll the mystic has to come in at or under. `cmp ax, 0x32; jg`.
pub const MYSTIC_THRESHOLD: i32 = 50;

// ---------------------------------------------------------------- the wizard

/// What Math did to you.
///
/// `_WIZARD`, image 0xb6fd. One roll, with the grudge added, decides between
/// four outcomes, and the thresholds are literals in the code: thirty,
/// seventy, ninety, and everything above that is a toad.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gift {
    /// A magic item, by id. `WIZBestowMagic`.
    Magic(String),
    /// A point of an ability. `WIZBestowAbility`.
    Ability(Ability),
    /// Coin. `WIZBestowGold`, ten to thirty one of it.
    Gold(u32),
    /// "You insulant little worm!" `[si+0x3a] = 3`, three days of it.
    Toad,
    /// The roll asked for something the run cannot take: a magic item the
    /// pack has no room for, or the pack declares none of the ten. The
    /// original rolls again until it can give; here the bell is answered
    /// with nothing rather than looped on.
    Nothing,
}

impl Gift {
    /// What Math says over the balcony, `WizardText1`..`14` and `WizardToad`,
    /// three lines to each ability in `WIZABL` order and the gold and magic
    /// lines cycled by `WIZGOLD_CNT` and `WIZMAG_CNT`; `n` is that counter.
    pub fn speech(&self, n: u32) -> &'static str {
        match self {
            Gift::Ability(Ability::Strength) => match n % 3 {
                0 => "You look weak for a knight. I will grant you the gift of strength.",
                1 => "You will require great strength to find what you seek. I will give you the great gift of strength.",
                _ => "Do you think you have the brawn to defeat your enemies? Ha! I  laugh at you! I will do you a favour and grant upon you the gift of strength.",
            },
            Gift::Ability(Ability::Endurance) => match n % 3 {
                0 => "You look like you could lose a few pounds. I will give you the gift of endurance.",
                1 => "Fleet of foot you must be to find what you seek. I will give you the great gift of endurance.",
                _ => "The path you seek is long and fraught with danger. I will bestow upon you the gift of endurance.",
            },
            Gift::Ability(Ability::Constitution) => match n % 3 {
                0 => "You look like you could get a cold in a drafty hallway. I will give you the gift of constitution!",
                1 => "Your body will have to absorb many punishing blows that would tear a normal mortal in two. I will give you the great gift of constitution!",
                _ => "Do you think that you are a mighty warrior? You are fodder for the creatures of the night! I will grant upon you the gift of constitution!",
            },
            Gift::Magic(_) => match n % 4 {
                0 => "You must be prepared for what lies ahead. This magical gift will help.",
                1 => "The magical forces of nature that bind us all together flow strongly around you. This gift is for you.",
                2 => "Use this gift to find what you seek. The future for you is uncertain.",
                _ => "You will need this enchanted object to aid you in your quest. Use it wisely, it may not be with you long.",
            },
            Gift::Gold(_) => match n % 3 {
                0 => "Gold is what fuels the desires of men.  So use this gift of gold to help you find what you seek.",
                1 => "The magical forces of nature that bind us all together flow strongly around you. This gift is for you.",
                _ => "Use this gift to find what you seek. The future for you is uncertain.",
            },
            Gift::Toad => "You insulant little worm! How dare you abuse my abundant warmth and endless favors!! One who acts as selfish as you deserves to receive all the gifts deserving of a TOAD!",
            Gift::Nothing => "The ominous figure of Math turns and slowly disappears into the depths of his tower. Somewhere off in the distance you can hear the voice of Math: 'Mortal fools all.'",
        }
    }

    /// What is at your feet when he has gone: `WizardGift`, `WizardGold`,
    /// `WizardAbil` and `WizardExit2`, with the blanks filled.
    pub fn aftermath(&self, items: &Items) -> String {
        match self {
            Gift::Magic(id) => format!(
                "As the great wizard disappears into his tower, a sack appears at your feet.  Opening it reveals a {}.",
                items.get(id).map_or(id.clone(), |d| d.name.clone())
            ),
            Gift::Gold(n) => format!(
                "As the eminant wizard Math vanishes into his tower a pouch appears at your feet filled with {n} gold."
            ),
            Gift::Ability(_) => "As the illustrious wizard disappears into his tower a great feeling of rejuvenation spreads throughout your body.".into(),
            Gift::Toad => "A strange feeling spreads throughout your body and the world starts to grow all about. When suddenly you see it is you being transformed into a TOAD!".into(),
            Gift::Nothing => String::new(),
        }
    }
}

/// `WizardIntro`, verbatim.
pub const WIZARD_INTRO: &str = "As you ring the bell at the bottom of the foreboding wizard's tower, a sense of unease rises in the air. A tall, dark figure slowly rises onto the balcony some fifty feet above your head and with a low, powerful breath, the mighty wizard Math speaks:";

/// Which magic item a roll of 0..=100 fetches.
///
/// **Recovered.** `_WIZARD:MagicRND` at DS:e365: ten records of a threshold
/// and a slot, walked for the first threshold the roll does not exceed. The
/// slots are the ones `MagicCast` dispatches on, which is what ties a name to
/// a behaviour. A healing potion is a quarter of all gifts; the three scrolls
/// at the top are one in twenty each.
pub const MAGIC_TABLE: [(u32, u8); 10] = [
    (25, 0x00),
    (35, 0x02),
    (45, 0x04),
    (55, 0x06),
    (65, 0x08),
    (75, 0x0a),
    (80, 0x0e),
    (85, 0x0c),
    (94, 0x10),
    (100, 0x12),
];

/// `MagicPrices`, as `SetUpStatus` fills it, by slot: what the temple sells
/// each for, in the words of its own `Buy ... for N GP` lines. It buys at half.
pub const MAGIC_PRICES: [(u8, u32); 12] = [
    (0x00, 20),
    (0x02, 32),
    (0x04, 100),
    (0x06, 50),
    (0x08, 52),
    (0x0a, 36),
    (0x0c, 52),
    (0x0e, 52),
    (0x10, 40),
    (0x12, 24),
    (0x14, 12),
    (0x16, 20),
];

/// The item id each magic slot names in the packs.
///
/// The slot numbers are the original's, and the names the items carry are
/// its own strings from `ma1`..`ma10`; the ids are ours, the names in snake
/// case. A pack that declares an item under another id simply never hands
/// that one out, because every bestowal checks the pack first.
pub fn magic_item(slot: u8) -> Option<&'static str> {
    Some(match slot {
        0x00 => "potion",
        0x02 => "gem_of_seeing",
        0x04 => "sword_of_sharpness",
        0x06 => "ring_of_protection",
        0x08 => "talisman_of_the_wyrm",
        0x0a => "scroll_of_haste",
        0x0c => "scroll_of_the_hawk",
        0x0e => "scroll_of_acquisition",
        0x10 => "scroll_of_the_wyrm",
        0x12 => "scroll_of_protection",
        _ => return None,
    })
}

/// The slot an item id names, if it is one of the ten.
pub fn magic_slot(id: &str) -> Option<u8> {
    MAGIC_TABLE.iter().map(|(_, s)| *s).find(|s| magic_item(*s) == Some(id))
}

/// The gold the wizard gives, and the gold a lair holds.
///
/// **Recovered.** The routine at image 0xb819 serves both: `rnd & 0x1f`, fold
/// anything over 21 down by ten, add ten. So ten to thirty one, weighted to the
/// top ten. Which purse it lands in is the `dx` it is called with: zero is the
/// knight's, anything else the lair's.
pub fn gold_from(roll: u32) -> u32 {
    let mut r = roll & 0x1f;
    if r > 21 {
        r -= 10;
    }
    r + 10
}

// ----------------------------------------------------------------- the stones

/// What the stone circle did.
///
/// `MOON:Henge`. Either you are carrying the moonstone of the night, which is
/// the end of the game, or the druids ask for an offering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rite {
    /// The quest is over. Which stone opened it, because `KnightWonGame` folds
    /// the phase into the ending it shows.
    Won(Moonstone),
    /// An offering accepted: a life point, a full mending and the ratman's
    /// bite lifted. `life` is whether there was a point to give.
    Blessed { offered: String, life: bool },
    /// Nothing to offer. The original's page simply has no gadget to press
    /// and lets you leave.
    NothingToOffer,
}

impl Rite {
    /// The between-days hint gives the words: `Offer a magic item within
    /// Stonehenge and Danu will grant you a longer life`. `HengeInstruct`,
    /// what the druids say on the night, is inside the unreadable part of
    /// DGROUP, so these lines are ours; `The druids prepare for the ritual`
    /// is the one recovered string, `_TAVERN:HengeWait`.
    pub fn describe(&self, items: &Items) -> String {
        match self {
            Rite::Won(stone) => format!("The druids prepare for the ritual. The {} blazes in your hand. The quest is done.", stone.name()),
            Rite::Blessed { offered, life } => {
                let name = items.get(offered).map_or(offered.clone(), |d| d.name.clone());
                if *life {
                    format!("The druids prepare for the ritual. Danu accepts the {name}, and grants you a longer life.")
                } else {
                    format!("The druids prepare for the ritual. Danu accepts the {name}, and your wounds close.")
                }
            }
            Rite::NothingToOffer => "The druids wait. You have nothing to offer Danu.".into(),
        }
    }
}

// ----------------------------------------------------------------- the work

impl Run {
    /// Throw three dice for a stake.
    ///
    /// `_TAVERN`: the stake leaves the purse first, the three dice are rolled
    /// and sorted, and a winning pattern multiplies the stake back in. The
    /// purse saturates at a hundred and fifty, so a big win can pay less than
    /// the arithmetic says, exactly as it does in the original.
    pub fn throw_dice(&mut self, stake: u32) -> Wager {
        if self.gold == 0 {
            return Wager::Skint;
        }
        if stake == 0 || stake > self.gold {
            return Wager::TooPoor;
        }
        self.gold -= stake;
        let mut dice = [0u8; DICE];
        for d in dice.iter_mut() {
            // `and ax, 7; cmp ax, 5; jg` rolls again: a bounded re-roll here,
            // and a modulus past it, so a run can never hang on a die.
            let mut face = FACES;
            for _ in 0..8 {
                let n = self.next_roll() & 7;
                if n < FACES {
                    face = n;
                    break;
                }
            }
            if face == FACES {
                face = self.roll(FACES);
            }
            *d = face as u8;
        }
        dice.sort_unstable();
        let won = ODDS
            .iter()
            .find(|(pattern, _)| *pattern == dice)
            .map_or(0, |(_, odds)| stake.saturating_mul(*odds));
        if won > 0 {
            self.gold = (self.gold + won).min(PURSE_CEILING);
        }
        Wager::Threw(Throw { dice, stake, won })
    }

    /// Donate to the town healer, and take what the pot buys.
    ///
    /// `HealDon`, in full: while there are ten in the pot, clear the bite for
    /// nothing, mend for ten if there is anything to mend, otherwise buy a
    /// life point for fifteen, and stop when neither is worth doing. The pot
    /// is the healer's whatever is left in it. None means the purse would
    /// not cover the donation and nothing changed hands.
    pub fn donate_to_healer(&mut self, gold: u32) -> Option<Healing> {
        if !self.alive() || !self.spend(gold) {
            return None;
        }
        let mut pot = gold as i32;
        let mut got = Healing { gave: gold, ..Healing::default() };
        while pot >= HEALER_MEND {
            if self.bitten {
                self.bitten = false;
                got.unbitten = true;
            }
            if self.health < self.max_health {
                self.health = self.max_health;
                pot -= HEALER_MEND;
                got.healed = true;
                continue;
            }
            if self.lives >= LIFE_CEILING {
                break;
            }
            self.lives += 1;
            pot -= HEALER_LIFE;
            got.lives += 1;
        }
        got.unspent = pot;
        Some(got)
    }

    /// Sell something to the temple. `SellToTemple` then `GoldSell`: one off
    /// the record, half the price into the purse, the purse capped at a
    /// hundred and fifty, and a sword sold out of the hand leaves a long
    /// sword in it (`mov word ptr [bp+0x40], 0x16`).
    pub fn sell_to_temple(&mut self, id: &str, items: &Items) -> Sale {
        let Some(def) = items.get(id) else { return Sale::Unknown };
        if is_token(id) || matches!(def.virtue, Virtue::Weapon { .. } | Virtue::Armour { .. }) && magic_slot(id).is_none() {
            return Sale::NotWanted;
        }
        if self.kit.lose(id, 1) == 0 {
            return Sale::HaveNone;
        }
        let paid = def.price / 2;
        self.gold = (self.gold + paid).min(PURSE_CEILING);
        if self.knight.weapon == id && self.kit.count(id) == 0 {
            self.knight.weapon = "long_sword".into();
        }
        self.refresh(items);
        Sale::Sold { paid }
    }

    /// Donate at the mystic, and let the cosmos decide.
    pub fn consult_the_mystic(&mut self, gold: u32, items: &Items) -> Reading {
        if gold == 0 {
            return Reading::NoDonation;
        }
        if !self.alive() || !self.spend(gold) {
            return Reading::TooPoor;
        }
        // `call 0xb7e9` picks the ability first, before the roll, and comes
        // back with nothing when all three are at five.
        let pick = self.next_roll();
        let picked = self.knight.rolled_ability(pick);
        let bonus = DONATION_TABLE
            .iter()
            .find(|(threshold, _)| gold <= *threshold)
            .map_or(-30, |(_, delta)| *delta);
        let good = self.roll(101) as i32 + bonus <= MYSTIC_THRESHOLD;
        let Some(which) = picked else {
            // Every ability maxed. The good branch says so; the bad branch
            // would walk into `MysticAbility` with a stale offset, which is a
            // slip. Saying so and changing nothing is the honest reading.
            return Reading::Maxed;
        };
        if good {
            return if self.raise_ability(which, items) {
                Reading::Granted(which)
            } else {
                Reading::Maxed
            };
        }
        // `cmp byte ptr [bx+di], 1; je ExitMystic`: the cosmos will not take
        // your last point of anything.
        if self.lower_ability(which, items) {
            Reading::Taken(which)
        } else {
            Reading::Weak
        }
    }

    /// Ring the wizard's bell.
    ///
    /// One roll, plus whatever grudge he is holding, against thirty, seventy
    /// and ninety. The grudge is a byte added to a byte, so a knight he has
    /// never met, at 0xff, rolls one lower and can never reach the toad: the
    /// routine tests the 0xff and rolls again. Leaving sets it to seventy
    /// whatever happened, so the second visit of a day is dangerous and the
    /// third is close to certain.
    pub fn visit_the_wizard(&mut self, items: &Items) -> Gift {
        let gift = loop {
            let never_met = self.grudge == crate::run::NEVER_MET;
            let roll = (self.roll(101) + self.grudge) & 0xff;
            if roll <= 30 {
                break self.wizard_magic(items);
            } else if roll <= 70 {
                match self.bestow_ability(items) {
                    Some(which) => break Gift::Ability(which),
                    // All three at five: `WIZBestowAbility` jumps back to
                    // the top and rolls again.
                    None => continue,
                }
            } else if roll <= 90 {
                let gold = gold_from(self.next_roll());
                self.gold = (self.gold + gold).min(PURSE_CEILING);
                break Gift::Gold(gold);
            } else if never_met {
                continue;
            } else {
                self.turned_to_toad();
                break Gift::Toad;
            }
        };
        self.grudge = WIZARD_GRUDGE;
        gift
    }

    /// One magic slot off `MagicRND`, with the two refusals the original
    /// makes before it will hand one over, and a third that is ours.
    ///
    /// Shared by the wizard and by the lairs, because it is literally the same
    /// routine: `WIZBestowMagic` is called with `dx` naming whose record the
    /// item is counted into, and everything above that is common.
    ///
    /// The original rolls again until it has a slot it will give, which is an
    /// unbounded loop. This one is bounded, because a simulation two machines
    /// have to keep in step must not be able to hang: sixteen tries under
    /// every refusal, then eight under the sword's alone, then the potion,
    /// which is never refused. Sixteen refusals in a row of the commonest slot
    /// is about one chance in ten billion.
    pub(crate) fn next_magic_slot(&mut self) -> u8 {
        for attempt in 0..24 {
            let roll = self.roll(101);
            let slot = MAGIC_TABLE
                .iter()
                .find(|(threshold, _)| roll <= *threshold)
                .map_or(0x12, |(_, slot)| *slot);
            // `cmp ax, 4; cmp word ptr [0x6f06], 0; jne` rolls again: one
            // Sword of Sharpness exists, and once it is out it never comes.
            if slot == 0x04 && self.sword_out {
                continue;
            }
            // `cmp ax, [MAGIC_CNT]; je` rolls again: never the same gift twice
            // running, wherever the last one went.
            if attempt < 16 && slot == self.magic_last {
                continue;
            }
            self.magic_last = slot;
            if slot == 0x04 {
                self.sword_out = true;
            }
            return slot;
        }
        self.magic_last = 0x00;
        0x00
    }

    /// A magic slot the pack has an item for, off the same roll. A slot the
    /// pack lacks is rolled past, which is ours, so a pack that declares only
    /// the potion still gets the potion; a pack that declares none of the ten
    /// gets nothing. The last magic ids that are known win a bounded search.
    pub(crate) fn known_magic_slot(&mut self, items: &Items) -> Option<u8> {
        let known = |slot: u8| magic_item(slot).is_some_and(|id| items.contains_key(id));
        if !MAGIC_TABLE.iter().any(|(_, s)| known(*s)) {
            return None;
        }
        for _ in 0..64 {
            let slot = self.next_magic_slot();
            if known(slot) {
                return Some(slot);
            }
        }
        MAGIC_TABLE.iter().map(|(_, s)| *s).find(|s| known(*s))
    }

    /// The wizard's magic gift: a slot off `MagicRND`, into your own pack.
    fn wizard_magic(&mut self, items: &Items) -> Gift {
        let Some(slot) = self.known_magic_slot(items) else { return Gift::Nothing };
        let Some(id) = magic_item(slot) else { return Gift::Nothing };
        if self.kit.take(id, 1) == 0 {
            return Gift::Nothing;
        }
        // `mov word ptr [di+0x40], 0x19`: the sword goes straight into the
        // hand rather than waiting to be equipped, and `add word ptr
        // [si+0x38], 0x14`: a ring is twenty health on the spot.
        if slot == 0x04 && matches!(items.get(id).map(|d| &d.virtue), Some(Virtue::Weapon { .. })) {
            self.knight.weapon = id.to_string();
        }
        if slot == 0x06 {
            self.health += 20;
        }
        self.refresh(items);
        Gift::Magic(id.to_string())
    }

    /// Stand in the circle.
    ///
    /// `MOON:Henge` tests the moonstone bits against the moon before it
    /// offers anything else: carry the stone of the night and the quest is
    /// over. Any other night the page asks for an offering, and for one the
    /// druids give a life point, close every wound and lift a ratman's bite.
    /// Which item is offered is the player's in the original; here it is
    /// whatever `offer` names, or the least valuable thing that can be
    /// offered when it names nothing.
    pub fn rite_at_the_stones(&mut self, offer: Option<&str>, items: &Items) -> Rite {
        let phase = self.moon.phase();
        if let Some(stone) = Moonstone::ALL
            .into_iter()
            .find(|m| m.phase() == phase && self.kit.count(m.item()) > 0)
        {
            self.won = true;
            return Rite::Won(stone);
        }
        let chosen = match offer {
            Some(id) if self.kit.count(id) > 0 && Self::offerable(id, items) => Some(id.to_string()),
            Some(_) => None,
            None => self
                .kit
                .iter()
                .filter(|(id, _)| Self::offerable(id, items))
                .min_by_key(|(id, _)| (items.get(*id).map_or(0, |d| d.price), id.to_string()))
                .map(|(id, _)| id.to_string()),
        };
        let Some(id) = chosen else { return Rite::NothingToOffer };
        self.kit.lose(&id, 1);
        let life = self.lives < LIFE_CEILING;
        if life {
            self.lives += 1;
        }
        self.refresh(items);
        self.health = self.max_health;
        self.bitten = false;
        Rite::Blessed { offered: id, life }
    }

    /// What Danu takes: the panel's `Offer` gadgets are the potion, the gem,
    /// the ring, the talisman and the five scrolls, so anything magic that is
    /// not a weapon, armour or one of the quest's tokens.
    pub fn offerable(id: &str, items: &Items) -> bool {
        if is_token(id) {
            return false;
        }
        match items.get(id).map(|d| &d.virtue) {
            None => false,
            Some(Virtue::Weapon { .. }) | Some(Virtue::Armour { .. }) => false,
            Some(_) => true,
        }
    }

    /// Whether a blow struck tonight is doubled by the moon.
    ///
    /// `MOON:CalcDamage` doubles the damage of a knight carrying the moonstone
    /// whose night it is. Nothing hands out a moonstone yet, so nothing reads
    /// this in a fight; it is here so the calendar's one combat effect has a
    /// name.
    pub fn moonstruck(&self) -> Option<Moonstone> {
        let phase = self.moon.phase();
        Moonstone::ALL.into_iter().find(|m| m.phase() == phase && self.kit.count(m.item()) > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Items};
    use crate::knight::{KnightDef, MAX_ABILITY};
    use crate::moon::Phase;
    use crate::run::NEVER_MET;

    fn goods() -> Items {
        let mut items = Items::new();
        let mut add = |id: &str, name: &str, price: u32, virtue: Virtue, consumed: bool| {
            items.insert(id.into(), ItemDef { name: name.into(), price, virtue, consumed });
        };
        add("potion", "Potion of healing", 20, Virtue::Heal { health: 40 }, true);
        add("gem_of_seeing", "Gem of seeing", 32, Virtue::Inert, false);
        add("sword_of_sharpness", "Sword of Sharpness", 100, Virtue::Weapon { damage: 5 }, false);
        add("ring_of_protection", "Ring of protection", 50, Virtue::Ward { health: 20 }, false);
        add("talisman_of_the_wyrm", "Talisman of the Wyrm", 52, Virtue::Inert, false);
        add("scroll_of_haste", "Scroll of Haste", 36, Virtue::Haste, true);
        add("scroll_of_the_hawk", "Scroll of the Hawk", 52, Virtue::Inert, true);
        add("scroll_of_acquisition", "Scroll of Aquisition", 52, Virtue::Inert, true);
        add("scroll_of_the_wyrm", "Scroll of the Wyrm", 40, Virtue::Inert, true);
        add("scroll_of_protection", "Scroll of Protection", 24, Virtue::Inert, true);
        add("long_sword", "Long sword", 0, Virtue::Weapon { damage: 0 }, false);
        add("padded_armour", "Padded armour", 0, Virtue::Armour { health: 0, stride: 0 }, false);
        for m in Moonstone::ALL {
            add(m.item(), m.name(), 20, Virtue::Inert, false);
        }
        items
    }

    fn knight() -> KnightDef {
        KnightDef {
            name: "Sir Banner".into(),
            shades: vec![0x2244cc],
            home: [16, 16],
            strength: 1,
            constitution: 1,
            endurance: 1,
            life: 5,
            daggers: 10,
            gold: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        }
    }

    fn run() -> (Run, Items) {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.kit.capacity = 40;
        (r, items)
    }

    // The tavern.

    /// The table is the original's own. Checked against it rather than a rule
    /// of thumb, because the payouts are not monotone in the face and a rule
    /// of thumb would quietly disagree.
    #[test]
    fn the_odds_table_is_the_one_in_the_executable() {
        let by = |d: [u8; 3]| ODDS.iter().find(|(p, _)| *p == d).map(|(_, m)| *m);
        assert_eq!(by([0, 0, 0]), Some(30), "three of the first face");
        assert_eq!(by([1, 1, 1]), Some(20), "three of the second beat three of the sixth");
        assert_eq!(by([5, 5, 5]), Some(18));
        assert_eq!(by([2, 2, 2]), Some(12), "and three of the third pay least of the triples");
        assert_eq!(by([0, 0, 5]), Some(10), "a pair of the first face and a sixth");
        assert_eq!(by([1, 1, 5]), None, "no other pair pays at all");
        assert_eq!(ODDS.len(), 11);
    }

    #[test]
    fn a_stake_leaves_the_purse_and_a_win_comes_back_multiplied() {
        let (mut r, _) = run();
        r.earn(100);
        let before = r.gold;
        let Wager::Threw(t) = r.throw_dice(5) else { panic!("should have thrown") };
        assert_eq!(t.stake, 5);
        assert!(t.dice.windows(2).all(|w| w[0] <= w[1]), "DiceSort leaves them ascending");
        assert!(t.dice.iter().all(|d| (*d as u32) < FACES));
        if t.winner() {
            assert_eq!(t.won, 5 * t.odds().unwrap());
            assert_eq!(r.gold, (before - 5 + t.won).min(PURSE_CEILING));
            assert!(t.describe(r.gold).starts_with("You won"));
        } else {
            assert_eq!(r.gold, before - 5);
            assert_eq!(t.describe(r.gold), format!("You lost 5 gold pieces. You now have {} gold pieces.", r.gold));
        }
    }

    #[test]
    fn the_table_will_not_take_a_stake_you_cannot_cover() {
        let (mut r, _) = run();
        assert_eq!(r.gold, 10);
        assert_eq!(r.throw_dice(25), Wager::TooPoor);
        assert_eq!(r.gold, 10, "and nothing left the purse");
        r.spend(10);
        assert_eq!(r.throw_dice(5), Wager::Skint, "the tavern turns out an empty purse");
    }

    /// The whole point of taking the roll from the run's own seed: two
    /// machines in the same tavern on the same day see the same dice, and a
    /// run restored from its own serialization carries on the same sequence.
    #[test]
    fn a_seeded_dice_game_replays_identically() {
        let (mut a, _) = run();
        a.earn(100);
        let mut b = a.clone();
        let left: Vec<Wager> = (0..40).map(|_| a.throw_dice(5)).collect();
        let right: Vec<Wager> = (0..40).map(|_| b.throw_dice(5)).collect();
        assert_eq!(left, right, "the same seed throws the same dice");
        let json = serde_json::to_string(&a).unwrap();
        let mut restored: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(a.throw_dice(5), restored.throw_dice(5));
    }

    /// Two hundred throws is enough to see every face and at least one win,
    /// which is what says the dice are dice and not a constant.
    #[test]
    fn the_dice_use_all_six_faces_and_sometimes_pay() {
        let (mut r, _) = run();
        let mut seen = [false; FACES as usize];
        let mut wins = 0;
        for _ in 0..200 {
            r.gold = r.gold.max(5);
            if let Wager::Threw(t) = r.throw_dice(1) {
                for d in t.dice {
                    seen[d as usize] = true;
                }
                wins += t.winner() as u32;
            }
        }
        assert!(seen.iter().all(|s| *s), "every face comes up: {seen:?}");
        assert!(wins > 0, "and a night at the table pays at least once");
    }

    #[test]
    fn a_purse_cannot_pass_the_ceiling_the_original_clamps_it_to() {
        let (mut r, _) = run();
        r.gold = 149;
        for _ in 0..200 {
            r.gold = r.gold.max(5);
            let _ = r.throw_dice(5);
            assert!(r.gold <= PURSE_CEILING, "a win is capped at a hundred and fifty");
        }
    }

    // The town healer.

    #[test]
    fn ten_at_the_healer_closes_your_wounds() {
        let (mut r, _) = run();
        r.earn(40);
        r.health = 5;
        let got = r.donate_to_healer(10).expect("the purse covers it");
        assert!(got.healed);
        assert_eq!(r.health, r.max_health);
        assert_eq!(r.gold, 40, "ten of the fifty went in the pot");
        assert_eq!(got.unspent, 0);
        assert_eq!(got.describe(), "You are healed of your wounds");
    }

    #[test]
    fn a_bigger_donation_buys_life_points_at_fifteen_each() {
        let (mut r, _) = run();
        r.earn(100);
        r.lives = 1;
        r.health = 1;
        let got = r.donate_to_healer(40).expect("covered");
        assert!(got.healed, "the wound is mended first, for ten");
        assert_eq!(got.lives, 2, "and thirty of the remaining pot buys two lives");
        assert_eq!(r.lives, 3);
        assert_eq!(got.unspent, 0);
    }

    /// `sub word ptr [DONATION], 0xf` after a `cmp ax, 0xa`: the last life
    /// point can go for ten and leave the pot below nothing. The code's rule,
    /// kept.
    #[test]
    fn the_healers_loop_sells_the_last_life_point_short() {
        let (mut r, _) = run();
        r.earn(100);
        r.lives = 4;
        let got = r.donate_to_healer(10).unwrap();
        assert_eq!(got.lives, 1);
        assert_eq!(got.unspent, -5);
        assert_eq!(r.lives, 5);
    }

    #[test]
    fn the_healer_lifts_a_ratmans_bite_for_nothing_and_says_so() {
        let (mut r, _) = run();
        r.earn(40);
        r.bitten = true;
        let got = r.donate_to_healer(10).unwrap();
        assert!(got.unbitten);
        assert!(!r.bitten);
        assert!(!got.healed, "an unmarked man is not mended");
        assert_eq!(got.describe(), "You have been healed.");
    }

    #[test]
    fn nine_coins_buy_roots_and_herbal_tea_and_are_kept() {
        let (mut r, _) = run();
        r.earn(40);
        r.health = 3;
        let got = r.donate_to_healer(9).unwrap();
        assert!(!got.anything(), "nine is under the ten the loop wants");
        assert_eq!(r.gold, 41, "the coin is gone all the same");
        assert_eq!(r.health, 3);
        assert!(got.describe().contains("herbal tea"));
    }

    #[test]
    fn the_healer_refuses_what_the_purse_cannot_cover() {
        let (mut r, _) = run();
        assert!(r.donate_to_healer(25).is_none());
        assert_eq!(r.gold, 10);
    }

    // The temple.

    #[test]
    fn the_temple_buys_magic_at_half_price_and_nothing_else() {
        let (mut r, items) = run();
        r.kit.take("gem_of_seeing", 1);
        r.kit.take("long_sword", 1);
        r.kit.take("moonstone.full", 1);
        assert_eq!(r.sell_to_temple("gem_of_seeing", &items), Sale::Sold { paid: 16 });
        assert_eq!(r.gold, 26);
        assert_eq!(r.kit.count("gem_of_seeing"), 0);
        assert_eq!(r.sell_to_temple("gem_of_seeing", &items), Sale::HaveNone);
        assert_eq!(r.sell_to_temple("long_sword", &items), Sale::NotWanted);
        assert_eq!(r.sell_to_temple("moonstone.full", &items), Sale::NotWanted);
        assert_eq!(r.sell_to_temple("grail", &items), Sale::Unknown);
    }

    #[test]
    fn selling_the_sword_in_your_hand_leaves_a_long_sword_there() {
        let (mut r, items) = run();
        r.kit.take("sword_of_sharpness", 1);
        r.knight.weapon = "sword_of_sharpness".into();
        assert_eq!(r.sell_to_temple("sword_of_sharpness", &items), Sale::Sold { paid: 50 });
        assert_eq!(r.knight.weapon, "long_sword");
    }

    #[test]
    fn the_temples_prices_are_the_panels_own_lines() {
        assert_eq!(MAGIC_PRICES.iter().find(|(s, _)| *s == 0x04).unwrap().1, 100, "Sword of Sharpness for 100 GP");
        assert_eq!(MAGIC_PRICES.iter().find(|(s, _)| *s == 0x00).unwrap().1, 20, "Potion of healing for 20 GP");
        assert_eq!(magic_slot("scroll_of_protection"), Some(0x12));
        assert_eq!(magic_slot("elixir"), None);
    }

    // The mystic.

    /// A big donation is meant to be worth making. Fifty coins subtracts thirty
    /// from a roll that has to come in at fifty, so about eight visits in ten
    /// should be a gift; nine coins adds twenty and about three should be.
    #[test]
    fn a_bigger_donation_buys_better_odds_from_the_cosmos() {
        let items = goods();
        let count = |donation: u32| {
            let mut good = 0;
            for seed in 0..200u32 {
                let mut r = Run::for_knight(&knight(), 0, &items);
                r.reseed(seed.wrapping_mul(2_654_435_761).wrapping_add(1));
                r.earn(400);
                r.knight.strength = 3;
                r.knight.constitution = 3;
                r.knight.endurance = 3;
                if matches!(r.consult_the_mystic(donation, &items), Reading::Granted(_)) {
                    good += 1;
                }
            }
            good
        };
        let mean = count(5);
        let generous = count(60);
        assert!(generous > mean + 40, "sixty coins ({generous}) beats five ({mean})");
        assert!(mean > 20 && mean < 120, "a small donation is still a gamble: {mean}");
    }

    #[test]
    fn the_mystic_gives_and_takes_the_same_three_abilities() {
        let items = goods();
        let (mut seen_good, mut seen_bad) = (false, false);
        for seed in 0..300u32 {
            let mut r = Run::for_knight(&knight(), 0, &items);
            r.reseed(seed.wrapping_mul(0x9e37_79b9).wrapping_add(7));
            r.earn(400);
            r.knight.strength = 3;
            r.knight.constitution = 3;
            r.knight.endurance = 3;
            let before = r.knight.strength + r.knight.constitution + r.knight.endurance;
            match r.consult_the_mystic(20, &items) {
                Reading::Granted(a) => {
                    seen_good = true;
                    assert_eq!(r.knight.ability(a), 4, "a point given");
                    assert!(!Reading::Granted(a).describe().contains("weak"));
                }
                Reading::Taken(a) => {
                    seen_bad = true;
                    assert_eq!(r.knight.ability(a), 2, "a point taken");
                }
                other => panic!("{other:?}"),
            }
            let after = r.knight.strength + r.knight.constitution + r.knight.endurance;
            assert!((after - before).abs() == 1, "exactly one point moves");
            assert_eq!(r.gold, 390, "and the donation is gone either way");
        }
        assert!(seen_good && seen_bad, "both halves of the roll are reachable");
    }

    #[test]
    fn the_cosmos_will_not_take_your_last_point() {
        let items = goods();
        let mut weak = 0;
        for seed in 0..80u32 {
            let mut r = Run::for_knight(&knight(), 0, &items);
            r.reseed(seed.wrapping_mul(48271).wrapping_add(3));
            r.earn(400);
            if r.consult_the_mystic(5, &items) == Reading::Weak {
                weak += 1;
            }
            assert!(r.knight.strength >= 1 && r.knight.constitution >= 1 && r.knight.endurance >= 1);
        }
        assert!(weak > 0, "a new knight is refused rather than hollowed out");
    }

    #[test]
    fn the_mystic_says_when_there_is_nothing_left_to_raise() {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.earn(400);
        r.knight.strength = MAX_ABILITY;
        r.knight.constitution = MAX_ABILITY;
        r.knight.endurance = MAX_ABILITY;
        assert_eq!(r.consult_the_mystic(60, &items), Reading::Maxed);
        assert_eq!(r.gold, 350, "and it still took the donation");
        assert_eq!(r.consult_the_mystic(0, &items), Reading::NoDonation);
        assert_eq!(r.consult_the_mystic(9000, &items), Reading::TooPoor);
    }

    // The wizard.

    /// Thirty, seventy, ninety. A knight he has met before, with no grudge
    /// left, can reach all four, and the shares are roughly the thresholds.
    #[test]
    fn the_wizard_gives_magic_ability_gold_or_a_toad() {
        let items = goods();
        let mut tally = [0; 4];
        for seed in 0..400u32 {
            let mut r = Run::for_knight(&knight(), 0, &items);
            r.reseed(seed.wrapping_mul(2_246_822_519).wrapping_add(11));
            r.grudge = 0;
            r.kit.capacity = 40;
            match r.visit_the_wizard(&items) {
                Gift::Magic(_) => tally[0] += 1,
                Gift::Ability(_) => tally[1] += 1,
                Gift::Gold(g) => {
                    tally[2] += 1;
                    assert!((10..=31).contains(&g), "ten to thirty one: {g}");
                }
                Gift::Toad => tally[3] += 1,
                Gift::Nothing => panic!("a pack with all ten declared never answers with nothing"),
            }
            assert_eq!(r.grudge, WIZARD_GRUDGE, "he remembers you on the way out");
        }
        assert!(tally.iter().all(|n| *n > 20), "all four outcomes come up: {tally:?}");
        assert!(tally[1] > tally[3], "an ability is likelier than a toad: {tally:?}");
    }

    /// `SetKnightEquipment` writes 0xff and the wizard rolls again on it: the
    /// first visit of a quest cannot end in a toad.
    #[test]
    fn a_knight_he_has_never_met_is_never_a_toad() {
        let items = goods();
        for seed in 0..300u32 {
            let mut r = Run::for_knight(&knight(), 0, &items);
            r.reseed(seed.wrapping_mul(0x8088_405).wrapping_add(1));
            assert_eq!(r.grudge, NEVER_MET);
            assert_ne!(r.visit_the_wizard(&items), Gift::Toad);
            assert!(!r.is_toad());
        }
    }

    /// The grudge is the interesting part: it makes the wizard a resource you
    /// can exhaust rather than a lever you can pull.
    #[test]
    fn coming_straight_back_to_the_wizard_turns_you_into_a_toad() {
        let items = goods();
        let mut toads = 0;
        for seed in 0..200u32 {
            let mut r = Run::for_knight(&knight(), 0, &items);
            r.reseed(seed.wrapping_mul(0x8088_405).wrapping_add(1));
            r.visit_the_wizard(&items);
            if r.visit_the_wizard(&items) == Gift::Toad {
                toads += 1;
                assert!(r.is_toad());
            }
        }
        assert!(toads > 130, "a second visit the same day is mostly toads: {toads}");
    }

    #[test]
    fn a_week_away_and_the_wizard_forgets() {
        let (mut r, items) = run();
        r.visit_the_wizard(&items);
        assert_eq!(r.grudge, WIZARD_GRUDGE);
        for _ in 0..6 {
            r.new_day();
        }
        assert_eq!(r.grudge, 10, "ten a day off seventy");
        r.new_day();
        assert_eq!(r.grudge, 0, "and the seventh clears it");
        let (mut fresh, _) = run();
        fresh.new_day();
        assert_eq!(fresh.grudge, NEVER_MET, "and a knight he has never met stays never met");
    }

    /// One Sword of Sharpness exists, `[0x6f06]` is a global, and the same
    /// gift never comes twice running.
    #[test]
    fn there_is_one_sword_and_no_gift_twice_running() {
        let (mut r, items) = run();
        r.kit.capacity = 400;
        let mut last: Option<String> = None;
        for _ in 0..400 {
            r.grudge = 0;
            if let Gift::Magic(id) = r.visit_the_wizard(&items) {
                assert_ne!(Some(&id), last.as_ref(), "the same gift twice running");
                last = Some(id);
            }
        }
        assert_eq!(r.kit.count("sword_of_sharpness"), 1, "exactly one sword in four hundred visits");
        assert_eq!(r.knight.weapon, "sword_of_sharpness", "and it went straight into the hand");
        assert!(r.kit.count("potion") > 20, "the potion is a quarter of all magic");
    }

    /// A pack that declares only the potion gets only the potion, and a pack
    /// that declares none of the ten gets nothing rather than an id nobody
    /// can carry.
    #[test]
    fn the_wizard_only_gives_what_the_pack_knows() {
        let mut items = goods();
        items.retain(|id, _| id == "potion" || id == "long_sword" || id == "padded_armour");
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.kit.capacity = 400;
        let mut magic = 0;
        for _ in 0..100 {
            r.grudge = 0;
            match r.visit_the_wizard(&items) {
                Gift::Magic(id) => {
                    assert_eq!(id, "potion");
                    magic += 1;
                }
                Gift::Nothing => panic!("the potion is there to give"),
                _ => {}
            }
        }
        assert!(magic > 10);
        items.retain(|id, _| id != "potion");
        for _ in 0..50 {
            r.grudge = 0;
            assert!(!matches!(r.visit_the_wizard(&items), Gift::Magic(_)));
        }
    }

    #[test]
    fn ten_to_thirty_one_gold_and_nothing_else() {
        for roll in 0..2000u32 {
            let g = gold_from(roll.wrapping_mul(2_654_435_761));
            assert!((10..=31).contains(&g), "{g}");
        }
        assert_eq!(gold_from(0), 10);
        assert_eq!(gold_from(21), 31);
        assert_eq!(gold_from(22), 22, "twenty two folds down by ten and back up by ten");
        assert_eq!(gold_from(31), 31);
    }

    #[test]
    fn the_wizard_speaks_in_his_own_words() {
        let items = goods();
        assert!(Gift::Ability(Ability::Strength).speech(0).contains("gift of strength"));
        assert!(Gift::Toad.speech(0).starts_with("You insulant"));
        assert!(Gift::Gold(17).aftermath(&items).ends_with("filled with 17 gold."));
        assert!(Gift::Magic("potion".into()).aftermath(&items).ends_with("reveals a Potion of healing."));
    }

    // The stones.

    #[test]
    fn the_stones_take_an_offering_for_a_life_point_and_a_mending() {
        let (mut r, items) = run();
        r.kit.take("potion", 1);
        r.kit.take("gem_of_seeing", 1);
        r.health = 3;
        r.lives = 2;
        r.bitten = true;
        let rite = r.rite_at_the_stones(None, &items);
        assert_eq!(rite, Rite::Blessed { offered: "potion".into(), life: true }, "the cheapest thing on you");
        assert_eq!(r.health, r.max_health);
        assert_eq!(r.lives, 3);
        assert!(!r.bitten);
        assert_eq!(r.kit.count("potion"), 0, "and the offering is gone");
        assert_eq!(r.kit.count("gem_of_seeing"), 1);
        assert!(!r.won);
        assert!(rite.describe(&items).contains("Potion of healing"));
    }

    #[test]
    fn the_stones_take_what_you_name_and_refuse_a_sword() {
        let (mut r, items) = run();
        r.kit.take("potion", 1);
        r.kit.take("gem_of_seeing", 1);
        r.kit.take("long_sword", 1);
        assert_eq!(
            r.rite_at_the_stones(Some("gem_of_seeing"), &items),
            Rite::Blessed { offered: "gem_of_seeing".into(), life: false }
        );
        assert_eq!(r.rite_at_the_stones(Some("long_sword"), &items), Rite::NothingToOffer);
        r.kit.lose("potion", 1);
        assert_eq!(r.rite_at_the_stones(None, &items), Rite::NothingToOffer);
    }

    #[test]
    fn a_moonstone_at_the_stones_on_its_own_night_ends_the_game() {
        let (mut r, items) = run();
        r.kit.take(Moonstone::Full.item(), 1);
        assert_eq!(r.moon.phase(), Phase::Full, "day one is the full moon");
        assert_eq!(r.rite_at_the_stones(None, &items), Rite::Won(Moonstone::Full));
        assert!(r.won);
        assert_eq!(r.kit.count(Moonstone::Full.item()), 1, "the stone is not consumed by the check");
    }

    #[test]
    fn the_wrong_stone_on_the_wrong_night_is_only_an_offering() {
        let (mut r, items) = run();
        r.kit.take(Moonstone::New.item(), 1);
        r.kit.take("potion", 1);
        assert!(matches!(r.rite_at_the_stones(None, &items), Rite::Blessed { .. }));
        assert!(!r.won);
        assert_eq!(r.kit.count(Moonstone::New.item()), 1, "a moonstone is never an offering");
        // Walk the moon round to the new one and try again.
        while r.moon.phase() != Phase::New {
            r.moon.new_day();
        }
        assert_eq!(r.rite_at_the_stones(None, &items), Rite::Won(Moonstone::New));
    }

    /// The one thing the moon does that you feel without going anywhere.
    #[test]
    fn a_moonstone_doubles_your_blows_on_its_own_night() {
        let (mut r, _) = run();
        assert_eq!(r.moonstruck(), None, "carrying nothing");
        r.kit.take(Moonstone::Full.item(), 1);
        assert_eq!(r.moonstruck(), Some(Moonstone::Full), "full moon, full stone");
        for _ in 0..4 {
            r.moon.new_day();
        }
        assert_eq!(r.moonstruck(), None, "and gone again when the moon moves");
    }
}
