# Contributing

## What this project will and will not accept

**Never commit anything from the original game.** No sprites, backgrounds, sounds,
music, disk images, executables, or anything baked out of them. The `.gitignore`
refuses the obvious cases, but it cannot catch everything, so check before you push.

This project distributes an **engine**. Anyone running it supplies their own copy of
the game. That is the line that keeps the project alive, and it is not negotiable.

Documenting a file format is fine: a format is a fact, not a creative work. Shipping
the bytes it describes is not.

## Do not copy from other reimplementations

OpenMoonstone is AGPL-3.0. Reading it to learn a format is fine; formats are facts.
Copying its code is not, and would impose its licence on this project. Every decoder
here was written from scratch against the raw data, and that needs to stay true.

## Before opening a pull request

```sh
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Rules the simulation has to keep

`henge-core` is the simulation, and it is deliberately austere. Its only dependency is
serde. Breaking any of the following forecloses networked play later, and none of it
can be retrofitted cheaply. See `docs/ROADMAP.md` for why.

- **Integers only.** No floating point anywhere in `henge-core`.
- **No `HashMap` iteration** in simulation code. Use `BTreeMap`, so order is defined.
- **Fixed ticks, never delta-time.**
- **No I/O, no rendering, no wall-clock time.**
- **Randomness is seeded and explicit**, carried in the state, never taken from the
  system.

## Tests

Test behaviour, not implementation. A test should read as a claim about the game:
`a_swing_connects_once_however_many_frames_carry_the_line` is a rule, `test_hit_fn`
is not. When a test fails, decide whether the code or the test is wrong before you
change either.

## Licence

**Not yet chosen.** Until it is, the repository is private and no contributions can be
accepted, because there is nothing for them to be licensed under.
