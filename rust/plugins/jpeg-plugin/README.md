# JPEGReadWriter2Plugin, in Rust

Replaces the plugin's vendored copy of **IJG libjpeg `6b, 27-Mar-1998`** — 24,627
lines of C carried in-tree since 1998, and therefore never picking up a distro
security update — with the maintained pure-Rust
[`jpeg-decoder`](https://crates.io/crates/jpeg-decoder) and
[`jpeg-encoder`](https://crates.io/crates/jpeg-encoder) crates, behind the same
eleven primitives.

|  | before | after |
|---|---:|---:|
| Hand-maintained C | 24,627 lines | 0 |
| Rust in this crate | — | ~700 lines |
| `setjmp`/`longjmp` per call | 2 | 0 |
| Security updates | never | `cargo update` |

## The contract is unchanged

Same module name, same primitive names, same argument order, same answers. The
image needs no modification: build this crate and drop
`libJPEGReadWriter2Plugin.so` where the VM looks for plugins, in place of the C
one.

## What changed underneath

**No `setjmp`/`longjmp`.** libjpeg reports errors by `longjmp`-ing out of an
error callback, so the C plugin `malloc`ed a `jmp_buf` on every call and
installed a custom `error_exit`. A decode failure here is an ordinary `Result`.
This also removes the one construct that is genuinely dangerous to mix with
Rust — a `longjmp` across a Rust frame is undefined behaviour.

**No pointers in image memory.** The C plugin handed the image a `ByteArray`
sized to `sizeof(struct jpeg_decompress_struct)` (600 bytes) and cast it back
on each call. That struct holds pointers, including one aimed *into a second
ByteArray* (`pcinfo->err = jpeg_std_error(&pjerr->pub)`), and it stays live
across two separate primitive calls with nothing pinning either array. This
port keeps the blob plain old data — 64 bytes of header fields, no pointers,
nothing to free — and re-parses the JPEG header in `readImage`. See
[`src/state.rs`](src/state.rs).

**Rows are packed into the Form's own bitmap.** The decoder answers its
pixels in Rust memory, but the packing loop that turns them into the image's
1/2/4/8/16/32-bit rows writes each row straight into the Bitmap's words --
the C's `bits` pointer -- instead of filling a row buffer and copying it
across. The Form is held as one in-place view for the whole loop, so a row
past the Bitmap's end still fails cleanly (`PrimErrBadIndex`) with the
earlier rows written, as before.

**An out-of-bounds read is gone.** See below.

## The out-of-bounds read

The C packing loop steps `out_components * pixels_per_word` bytes at a time
until it passes `row_stride`:

```c
for (i = 0, j = 0; i < rowStride; i += (pcinfo->out_color_components * pixelsPerWord), j++) {
    ...  buffer[0][i+redOffset2] ...   /* reads up to i+5 */
}
```

libjpeg allocates that scanline buffer at exactly `rowStride` bytes. When the
width is not a multiple of `pixels_per_word`, the last iteration reads past the
end — up to three bytes at depth 16, one at depth 8.

Confirmed experimentally. Decoding the same corpus with both plugins and
comparing pixel by pixel, the differences on odd-width images are enormous (up
to a full-scale 255) and confined entirely to the trailing partial word, while
even-width images differ by at most 3. Those trailing pixels are *padding*
beyond the Form's visible width, so nothing displayed was ever affected — but
the read itself is undefined behaviour on data adjacent to the heap buffer.

This port substitutes zero past the end of the scanline, which is deterministic
and in bounds.

## Verification

Correctness for a decoder is not "it returned something". The plugin was
checked against the C one over a corpus chosen to include the awkward cases:
RGB and grayscale, baseline and progressive, quality 15 to 95, sizes 1×1 to
200×150, and deliberately odd widths (1, 7, 31, 33).

Each image was decoded at every supported Form depth (32, 16, −16, 8, −8) with
dithering on and off — **90 cases** — and the resulting bitmaps compared
channel by channel.

| | result |
|---|---|
| Width, height, component count, bitmap size | identical in all 90 cases |
| **Worst visible channel difference** | **3** (of 255; of 31 at depth 16) |
| Cases with a visible difference > 4 | **0** |
| Cases where only padding differed | 28 — all odd-width, all at depth 16 or 8 |

A worst-case difference of 3 is the expected disagreement between two
conforming JPEG decoders: libjpeg-6b and `jpeg-decoder` round the IDCT and
chroma upsampling differently. It is not drift in the packing.

The encoder was checked separately: output re-decoded to the correct
dimensions, honoured the progressive flag, and matched the source gradient
within JPEG's lossy tolerance (Δ≤4 at quality 90, Δ≤16 at quality 25).
Comparing the two encoders' output after decoding gives a maximum difference of
9/255, with the Rust files 1–25% larger at the same nominal quality — different
quantisation tuning, not a defect.

Reproduce with the harnesses in `rust/plugins/jpeg-plugin/tests/harness/`.

## Not done yet

* ~~**CMake wiring.**~~ Done since: `USE_RUST_PLUGINS=ON` builds this crate
  instead of the C plugin (`cmake/rust.cmake` / `cmake/plugins.cmake`).
* **CMYK JPEGs.** `jpeg-decoder` reports `CMYK32`; the packing paths cover 1
  and 3 components, as the C did. A CMYK source decodes with the component
  offsets the C would have used, which is to say: neither implementation
  handles it properly.
* **16-bit grayscale** (`L16`) is reported as one component and packed as 8-bit.
