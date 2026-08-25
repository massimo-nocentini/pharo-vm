# LocalePlugin, in Rust

Replaces the Slang-generated `plugins/LocalePlugin/src/common/LocalePlugin.c`
(~420 lines) **and the Unix support layer** `src/unix/sqUnixLocale.c` (~760
lines, over half of it dead code — see below), behind the same fourteen
exports. **Windows and macOS keep their C plugin**: only the Unix support
layer is ported, and the crate refuses to compile elsewhere.

## The contract is unchanged

Same module name, same primitive names (all fourteen: `primitiveCountry`,
`primitiveLanguage`, `primitiveCurrencyNotation`, `primitiveCurrencySymbol`,
`primitiveDecimalSymbol`, `primitiveDigitGroupingSymbol`,
`primitiveMeasurementMetric`, `primitiveLongDateFormat`,
`primitiveShortDateFormat`, `primitiveTimeFormat`,
`primitiveDaylightSavings`, `primitiveTimezoneOffset`,
`primitiveVMOffsetToUTC`, plus `initialiseModule`), same accessor depth (−1
throughout, matching the `\377` bytes in the C's export table — none of these
primitives reads an argument's contents), same answers, same answer *shapes*:

* country and language are 3-byte Strings holding a 2-letter code and a
  trailing NUL — the C compiles with `CODELEN == 2`, allocates 3 and copies 2;
* decimal and digit-grouping symbols are 1-byte Strings;
* the currency symbol and the date/time formats are sized to their bytes;
* long and short date format both answer `nl_langinfo(D_FMT)` — identical on
  this platform, as in the C;
* `primitiveMeasurementMetric` is hardwired `true` and
  `primitiveVMOffsetToUTC` hardwired `0`, as in the C;
* `primitiveCurrencyNotation` answers `p_cs_precedes != 0`, which makes the
  POSIX "unavailable" marker `CHAR_MAX` read as *true* — faithfully preserved.

Every value still comes from the platform's own C library through `libc` —
`setlocale(LC_ALL, "")` at module initialisation, then `localeconv`,
`nl_langinfo(D_FMT/T_FMT)` and `localtime` (`tm_gmtoff`, `tm_isdst`) — so the
image sees what the C plugin showed it on the same system. No locale data is
reimplemented in Rust. These libc entry points are thread-unsafe exactly as
they were for the C plugin; both rely on primitives running only on the
interpreter thread, and this port additionally copies every libc string out
before returning rather than retaining pointers into libc's static buffers
(the C kept `setlocale`'s and `localeconv`'s pointers in globals).

The locale-*string* parsing — the one algorithmic part — is ported byte for
byte in [`src/parse.rs`](src/parse.rs): `LC_ALL` → `LANG` → current locale →
`"en_US.ISO8859-1"` fallback with the C's exact sanitisation, country = the
two bytes between the last `_` and the following `.`, language = the first
two bytes when they are letters and the third is `.`, `_` or the end. Quirks
included: `de_DE@euro` falls back to `US` (the segment is 7 bytes, not 2),
`en_us.utf8` answers lowercase `us`, an empty `LC_ALL` passes sanitisation,
and glibc's composite `LC_CTYPE=...;LC_NUMERIC=...` strings parse to whatever
the C would have made of them.

## What changed underneath

**A heap overflow is gone.** The C's `safestrcpy` copies
`strlen(source)` bytes wherever it is pointed. For the decimal and
digit-grouping symbols the generated code allocates a **1-byte** String, so
any locale whose separator is multi-byte (`ru_RU.UTF-8`'s NBSP thousands
separator is three bytes of UTF-8) made the C write past the end of the
object, corrupting the adjacent heap object's header. This port truncates to
the first byte instead: what the image sees in the String is unchanged (it
only ever saw byte 0 — the rest landed outside the object), and the write is
in bounds.

**No retained libc pointers.** The C stored `setlocale`'s return in a global
`const char *` and parsed it lazily; any later `setlocale` in the process
(another plugin, FFI) would mutate or invalidate it. The locale string is
snapshotted once at initialisation. Same value, no dangling.

**No NULL dereference on an uninitialised module.** Had a primitive somehow
run before `initialiseModule`, the C dereferenced NULL (`localeString`,
`localeConv`). The port initialises lazily on first use instead. Unreachable
in practice — the VM initialises a module before dispatching into it.

**Arity is checked.** The generated C pops 1 and pushes the answer without
checking `argumentCount`; installed on a non-unary method it would silently
unbalance the stack. Each primitive here fails cleanly with `BadNumArgs`
instead. With the arity the image actually uses (all these are unary) the
behaviour is identical.

**No per-call caching.** The C caches country and language in function-local
statics on first call. This port recomputes them from the snapshotted locale
string — deterministic, so the answers are identical.

**Dead code dropped, not ported.** The C carries ISO→UN 3-letter code tables
(~450 lines) for `CODELEN == 3`, which is never compiled. The module-name
string also drops the C's `" VMMaker.oscog-eem.2495 (e)"` suffix, as
jpeg-plugin's did — the VM compares the prefix, and this string carries no
version contract (unlike LargeIntegers').

## Verification

`cargo test -p locale-plugin` runs 17 tests:

* 16 known-answer tests on the parsing core, covering the shapes the C
  comments enumerate (`ll`, `ll.PP`, `ll_CC`, `ll_CC.PP`, `_CC`, `_CC.PP`),
  the degenerate inputs (`C`, `POSIX`, empty, one-letter, three-letter),
  the sanitisation rules (spaces, slashes, exact-match-only `C`/`POSIX`,
  empty-string pass-through), the priority order `LC_ALL` > `LANG` >
  current, case preservation, last-underscore selection, `@modifier`
  behaviour with and without a dot, and a glibc composite locale string;
* one serialised smoke test over the libc layer (locale capture stability,
  2-byte codes, non-empty decimal point and formats, timezone offset within
  ±24 h, DST answered).

## Not verified

* **Image-side differential testing** — no runnable VM in the porting
  environment. The proxy plumbing (`instantiateClassindexableSize` +
  `write_bytes` answering through `methodReturnValue` vs the C's
  `popthenPush(1, ...)`) and the exact oop-level answers should be compared
  against the C plugin from an image, across locales (`C`, `en_US.UTF-8`,
  `de_DE.UTF-8`, a composite `LC_*` mix) and timezones.
* The `setlocale(LC_ALL, "") == NULL` fallback path (requires an environment
  naming a locale the system lacks).
* Behaviour under a non-UTF-8 8-bit locale (`en_US.ISO8859-1`) — the byte
  copies are locale-encoding-agnostic by construction, but untested against
  a real system so configured.
* **CMake wiring.** The crate builds; switching the Unix build to link it in
  place of the C plugin is deliberately left as a separate change.
