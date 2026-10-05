"ffi.st - the FFI of the WebAssembly VM (tests/wasm/lanes/58-ffi.sh)

Callouts through uFFI to the C library (LibC's libc.so.6, the library c of
the registry of src/emscripten) and to the FFI test library (TestLibrary,
libTestLibrary.so, in the node VM only): scalars, a variadic function,
structures by value, callbacks, reentrant callbacks, results declared wider
than the function's (an int answered for a declared int64 or uint64, which
the return patch of libffi widens, signed or unsigned), declarations whose
integers are wider or narrower than the function's, which the width adapter
of the FFI (src/emscripten/ffiAdapt.c) adapts to the function (strlen
declared int, abs given an int64, bsearch given ints for its size_t, with a
callback that makes an adapted callout itself) or calls as a variadic
function (sprintf declared without fixedArgumentCount:, unless a float is
among its variadic arguments, which C cannot take: a primitive failure, and
the adapter says why on stderr), declarations that do not match the function
in another way (a primitive failure on WebAssembly, where the types of a
call are checked: a double declared for strlen's size_t, and a callback
declared void where C calls an int (*)(int)), the environment, and a bench
of 10000 strlen callouts, which must take less than WASM_FFI_BENCH_MS
milliseconds (default 10000).  Prints a line for each check, 'ok <name>' or
'FAIL <name>: ...', the line 'ffi bench: 10000 callouts in <ms> ms', then
'ffi: <n> checks, <m> failed'.

With WASM_FFI_BUSY set, it runs none of the checks, but a busy loop of 2 s,
inside a callback (callback) or not (top), then prints 'busy <where>: done',
for the lane to compare the slices of the two (PHARO_WASM_STATS)."
| out checks failed lib arr cb got env busy benchLimit benchMs |
out := Stdio stdout.
checks := 0. failed := 0.
env := OSEnvironment current.
((FFILibrary << #WasmFFILibC) package: 'WasmFFILane') install.
(Smalltalk at: #WasmFFILibC) compile: 'unixLibraryName ^ ''libc.so.6'''.
((Object << #WasmFFILane) package: 'WasmFFILane') install.
#( 'getenv: name ^ self ffiCall: #(String getenv(String name)) library: WasmFFILibC'
   'strlen: s ^ self ffiCall: #(size_t strlen(String s)) library: WasmFFILibC'
   'strlenAsDouble: s ^ self ffiCall: #(double strlen(String s)) library: WasmFFILibC'
   'abs: x ^ self ffiCall: #(int abs(int x)) library: WasmFFILibC'
   'llabs: x ^ self ffiCall: #(int64 llabs(int64 x)) library: WasmFFILibC'
   'pow: x to: y ^ self ffiCall: #(double pow(double x, double y)) library: WasmFFILibC'
   'memcpy: dst from: src size: n ^ self ffiCall: #(void* memcpy(ByteArray dst, ByteArray src, size_t n)) library: WasmFFILibC'
   'snprintf: buf size: n format: f with: i ^ self ffiCall: #(int snprintf(ByteArray buf, size_t n, String f, int i)) library: WasmFFILibC fixedArgumentCount: 3'
   'qsort: base count: n size: s with: c ^ self ffiCall: #(void qsort(ByteArray base, size_t n, size_t s, FFICallback c)) library: WasmFFILibC'
   'noSuchFunction ^ self ffiCall: #(int no_such_function_in_libc()) library: WasmFFILibC'
   'newPointX: x y: y ^ self ffiCall: #(TFPointTestStruct newPoint(int x, int y)) library: TFTestLibraryUsingSameThreadRunner'
   'assertPoint: p x: x y: y ^ self ffiCall: #(int assertCorrectPoint(TFPointTestStruct p, int x, int y)) library: TFTestLibraryUsingSameThreadRunner'
   'singleCall: c base: v ^ self ffiCall: #(int singleCallToCallback(void *c, int v)) library: TFTestLibraryUsingSameThreadRunner'
   'reentring: c base: v ^ self ffiCall: #(int reentringCallback(void *c, int v)) library: TFTestLibraryUsingSameThreadRunner'
   'callInt: c value: v ^ self ffiCall: #(void simple_callback_f_int(void *c, int v)) library: TFTestLibraryUsingSameThreadRunner'
   'idIntAsInt64: v ^ self ffiCall: #(int64 id_int(int v)) library: TFTestLibraryUsingSameThreadRunner'
   'idIntAsUint64: v ^ self ffiCall: #(uint64 id_int(int v)) library: TFTestLibraryUsingSameThreadRunner'
   'strlenAsInt: s ^ self ffiCall: #(int strlen(String s)) library: WasmFFILibC'
   'absInt64: x ^ self ffiCall: #(int abs(int64 x)) library: WasmFFILibC'
   'sprintf: buf format: f with: i ^ self ffiCall: #(int sprintf(ByteArray buf, String f, int i)) library: WasmFFILibC'
   'sprintfFixed: buf format: f with: i ^ self ffiCall: #(int sprintf(ByteArray buf, String f, int i)) library: WasmFFILibC fixedArgumentCount: 2'
   'sprintf: buf format: f withFloat: x ^ self ffiCall: #(int sprintf(ByteArray buf, String f, float x)) library: WasmFFILibC'
   'bsearch: key in: base count: n size: s with: c ^ self ffiCall: #(void* bsearch(void* key, void* base, int n, int s, FFICallback c)) library: WasmFFILibC'
) do: [:source | (Smalltalk at: #WasmFFILane) compile: source].
lib := (Smalltalk at: #WasmFFILane) new.
"(a loop of 2 s, the same in a callback and outside one)"
busy := [| t0 | t0 := Time millisecondClockValue. [Time millisecondClockValue - t0 < 2000] whileTrue. 0].
(env at: 'WASM_FFI_BUSY' ifAbsent: [nil]) ifNotNil: [:where |
	where = 'callback'
		ifTrue: [lib singleCall: (FFICallback signature: #(int (int a)) block: [:a | busy value + a]) base: 0]
		ifFalse: [busy value].
	out nextPutAll: 'busy '; nextPutAll: where; nextPutAll: ': done'; lf; flush.
	Smalltalk snapshot: false andQuit: true].
benchLimit := (env at: 'WASM_FFI_BENCH_MS' ifAbsent: ['10000']) asNumber.
[:name :expected :block | | value |
	checks := checks + 1.
	value := [block value] on: Error do: [:e | e class name].
	value = expected
		ifTrue: [out nextPutAll: 'ok '; nextPutAll: name; lf]
		ifFalse: [failed := failed + 1.
			out nextPutAll: 'FAIL '; nextPutAll: name; nextPutAll: ': expected '; nextPutAll: expected printString; nextPutAll: ', got '; nextPutAll: value printString; lf].
	out flush] in: [:check |
	check value: 'backend' value: #TFFIBackend value: [FFIBackend current class name].
	check value: 'getenv' value: 'set' value: [lib getenv: 'WASM_FFI_LANE'].
	check value: 'strlen' value: 10 value: [lib strlen: 'hello wasm'].
	check value: 'abs' value: 42 value: [lib abs: -42].
	check value: 'llabs' value: (2 raisedTo: 40) value: [lib llabs: (2 raisedTo: 40) negated].
	check value: 'pow' value: 1024.0 value: [lib pow: 2.0 to: 10.0].
	check value: 'memcpy' value: #[1 2 3 4 5] value: [| buffer | buffer := ByteArray new: 5. lib memcpy: buffer from: #[1 2 3 4 5] size: 5. buffer].
	check value: 'variadic snprintf' value: 'n=42' value: [| buffer n | buffer := ByteArray new: 32. n := lib snprintf: buffer size: 32 format: 'n=%d' with: 42. (buffer first: n) asString].
	check value: 'missing symbol' value: #SymbolNotFoundError value: [lib noSuchFunction].
	check value: 'struct returned by value' value: 3 @ 4 value: [| p | p := lib newPointX: 3 y: 4. p x @ p y].
	check value: 'struct passed by value' value: 1 value: [| p | p := TFPointTestStruct new. p x: 7; y: 8. lib assertPoint: p x: 7 y: 8].
	check value: 'qsort with a callback' value: #(1 2 3 5 7 9) value: [
		arr := ByteArray new: 24.
		#(5 3 9 1 7 2) doWithIndex: [:e :i | arr unsignedLongAt: i - 1 * 4 + 1 put: e bigEndian: false].
		cb := FFICallback signature: #(int (void *a, void *b)) block: [:a :b | (a signedLongAt: 1) - (b signedLongAt: 1)].
		lib qsort: arr count: 6 size: 4 with: cb.
		(1 to: 6) collect: [:i | arr unsignedLongAt: i - 1 * 4 + 1 bigEndian: false]].
	check value: 'callback' value: 43 value: [lib singleCall: (FFICallback signature: #(int (int a)) block: [:a | a + 1]) base: 41].
	check value: 'reentrant callbacks, 50 deep' value: 51 value: [
		cb := FFICallback signature: #(int (int a)) block: [:a | a = 0 ifTrue: [0] ifFalse: [(lib reentring: cb base: a - 1) + 1]].
		lib reentring: cb base: 50].
	check value: 'result declared int64, function answers int -1 (sign-extended)' value: -1 value: [lib idIntAsInt64: -1].
	check value: 'result declared uint64, function answers int -1 (zero-extended)' value: 4294967295 value: [lib idIntAsUint64: -1].
	check value: 'result declared double, function answers size_t' value: #PrimitiveFailed value: [lib strlenAsDouble: 'hello'].
	check value: 'callback declared void, C calls int (*)(int)' value: #PrimitiveFailed value: [
		lib callInt: (FFICallback signature: #(void (int a)) block: [:a | got := a]) value: 41].
	check value: 'strlen declared int (adapted)' value: 5 value: [lib strlenAsInt: 'hello'].
	check value: 'abs given an int64, of which C takes the low 32 bits (adapted)' value: 42 value: [lib absInt64: (1 bitShift: 32) - 42].
	check value: 'sprintf without fixedArgumentCount: (variadic), as with it' value: #('n=42' 'n=42') value: [
		{ #sprintf:format:with:. #sprintfFixed:format:with: } collect: [:selector | | buffer n |
			buffer := ByteArray new: 32.
			n := lib perform: selector with: buffer with: 'n=%d' with: 42.
			(buffer first: n) asString]].
	check value: 'sprintf given a float among its variadic arguments' value: #PrimitiveFailed value: [
		lib sprintf: (ByteArray new: 32) format: 'x=%f' withFloat: 1.5].
	check value: 'an adapted callout in the callback of an adapted bsearch' value: #(4 #(5)) value: [| base key inner found |
		base := ExternalAddress allocate: 24.
		key := ExternalAddress allocate: 4.
		inner := OrderedCollection new.
		[#(1 2 3 5 7 9) doWithIndex: [:e :i | base signedLongAt: i - 1 * 4 + 1 put: e].
		key signedLongAt: 1 put: 7.
		cb := FFICallback signature: #(int (void *a, void *b)) block: [:a :b |
			inner add: (lib strlenAsInt: 'hello').
			(a signedLongAt: 1) - (b signedLongAt: 1)].
		found := lib bsearch: key in: base count: 6 size: 4 with: cb.
		{found asInteger - base asInteger // 4. inner asSet asArray}]
			ensure: [base free. key free]].
	check value: 'callouts after the failures' value: 3 value: [lib strlen: 'abc'].
	check value: 'environment at:put:' value: 'x' value: [
		env at: 'WASM_FFI_LANE_SET' put: 'x'.
		lib getenv: 'WASM_FFI_LANE_SET'].
	check value: 'bench: 10000 strlen callouts' value: true value: [| sum |
		sum := 0.
		benchMs := [1 to: 10000 do: [:i | sum := sum + (lib strlen: 'hello wasm')]] timeToRun asMilliSeconds.
		out nextPutAll: 'ffi bench: 10000 callouts in '; print: benchMs; nextPutAll: ' ms'; lf.
		sum = 100000 and: [benchMs < benchLimit]]].
out nextPutAll: 'ffi: '; print: checks; nextPutAll: ' checks, '; print: failed; nextPutAll: ' failed'; lf.
