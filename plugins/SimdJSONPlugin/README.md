# SimdJSONPlugin

JSON reading for Pharo 15 with simdjson 5.0.5: the plugin (`src/`, `include/`) and its reader `SimdJSONReader`, with the API of NeoJSON (`st/SimdJSON.st`); see [docs/SimdJSON.md](../../docs/SimdJSON.md) for the build options, the API, the differences from NeoJSON, the tests and the benchmark (`bench/`).
File in NeoJSON first, if you want it, then `st/SimdJSON.st` (neo mode, or standalone mode without NeoJSON), then `st/SimdJSON-Tests.st`, then, in neo mode, `st/SimdJSON-NeoTests.st`.
simdjson is under the Apache License 2.0 or the MIT licence, installed next to the plugin as `LICENSE.simdjson` and `LICENSE.simdjson-MIT`; the standalone mapper of `st/SimdJSON.st` is a port of NeoJSON, under its MIT licence (Sven Van Caekenberghe).
