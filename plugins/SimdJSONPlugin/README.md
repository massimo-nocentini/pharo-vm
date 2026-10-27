# SimdJSONPlugin

JSON reading for Pharo 15 with simdjson 5.0.5: the plugin (`src/`, `include/`) and its reader `SimdJSONReader`, with the API of NeoJSON (`st/SimdJSON.st`); see [docs/SimdJSON.md](../../docs/SimdJSON.md) for the build options, the API, the differences from NeoJSON, the tests and the benchmark (`bench/`).
File in `st/SimdJSON.st`, which needs no NeoJSON, then `st/SimdJSON-Tests.st`; `st/SimdJSON-NeoTests.st` compares the reader with NeoJSON, in an image where NeoJSON is loaded (`st/load-neojson.st`).
simdjson is under the Apache License 2.0 or the MIT licence, installed next to the plugin as `LICENSE.simdjson` and `LICENSE.simdjson-MIT`; the mapper of `st/SimdJSON.st` is a port of NeoJSON, under its MIT licence (Sven Van Caekenberghe).
