# Asset packs

`original/` is ours. Everything in it is safe to distribute, and it is the pack
that grows over time.

`reference/` is not in this repository and never will be. Generate it locally
from your own copy of the 1991 game:

```sh
cargo run --release -p henge-formats --bin henge-bake -- "path/to/Moonstone" packs/reference
```

It is gitignored, marked `derived-from-original`, and the shippability check
fails while anything still resolves to it.
