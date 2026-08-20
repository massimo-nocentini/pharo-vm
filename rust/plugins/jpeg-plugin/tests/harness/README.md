# Differential harnesses

These reproduce the C-vs-Rust comparison reported in the crate README. They are
Smalltalk scripts rather than `cargo test` cases because the thing under test is
a plugin loaded by a running VM into a real image.

## Decode (`jpegdiff.st`)

Declares each primitive on `ByteArray` with a `<primitive:module:>` pragma,
drives them directly, and dumps every decoded bitmap to `dump/`.

```sh
# 1. a corpus (see make_corpus.py)
# 2. run once per plugin, saving the dumps
cp libJPEGReadWriter2Plugin.so <vm-dir>/          # the C one
cd <vm-dir> && ./pharo --headless img.image st jpegdiff.st && mv dump dump_c
cp .../libJPEGReadWriter2Plugin.so <vm-dir>/      # the Rust one
cd <vm-dir> && ./pharo --headless img.image st jpegdiff.st && mv dump dump_rs
# 3. compare with compare.py
```

Compare **visible pixels only**. Odd-width Forms carry padding pixels past the
visible width, and that is exactly where the C plugin reads out of bounds, so a
whole-bitmap comparison reports differences that no user could ever see.

## Encode (`jpegenc.st`)

Builds known gradient Forms, encodes them through the plugin, and writes the
JPEGs to `enc/` for checking with any independent decoder.

## Note

`pharo ... st <file>` prints its output and then does not exit — true of both
plugins, and unrelated to them. Run it under `timeout` and read the dumps.
