"ffi-session.st - a session of the FFI of the WebAssembly VM, through the
image's own bindings (tests/wasm/lanes/59-ffi-clean.sh)

The image's own callouts into the C library: the environment (OSEnvironment,
on LibC's setenv and getenv), the process id (LibC>>currentProcessId, on
getpid), the message of an errno (OSPlatform>>getErrorMessage:, on strerror,
which UnixPlatform binds from the module #LibC), and a qsort with a
Smalltalk callback.  Run on an image without the marker WasmFFISession, it
makes the checks, then saves the image with the marker and quits; run again
on the saved image, it makes the checks again, after the reload, which must
have looked the functions up again.
Prints a line for each check, 'ok <name>' or 'FAIL <name>: ...', then
'ffi-session <phase>: <n> checks, <m> failed', phase 'first' or 'reload'.

With WASM_FFI_CONTROL set, it makes none of the checks, but one callout
whose declaration does not match its function (strlen declared double),
which must fail as a primitive, then prints 'ffi-session control:
<result>': the negative control of the lane, which must see exactly one
line of the guard of the callouts on stderr."
| out checks failed env phase |
out := Stdio stdout.
checks := 0. failed := 0.
env := OSEnvironment current.
(Smalltalk hasClassNamed: #WasmFFISessionCalls) ifFalse: [
	((Object << #WasmFFISessionCalls) package: 'WasmFFISession') install.
	#( 'qsort: base count: n size: s with: c ^ self ffiCall: #(void qsort(ByteArray base, size_t n, size_t s, FFICallback c)) library: LibC'
	   'strlenAsDouble: s ^ self ffiCall: #(double strlen(String s)) library: LibC'
	) do: [:source | (Smalltalk at: #WasmFFISessionCalls) compile: source]].
(env at: 'WASM_FFI_CONTROL' ifAbsent: [nil]) ifNotNil: [:control |
	out nextPutAll: 'ffi-session control: ';
		print: ([(Smalltalk at: #WasmFFISessionCalls) new strlenAsDouble: 'abc'] on: PrimitiveFailed do: [:e | #failed]);
		lf; flush.
	Smalltalk snapshot: false andQuit: true].
phase := (Smalltalk includesKey: #WasmFFISession) ifTrue: ['reload'] ifFalse: ['first'].
[:name :expected :block | | value |
	checks := checks + 1.
	value := [block value] on: Error do: [:e | e class name].
	value = expected
		ifTrue: [out nextPutAll: 'ok '; nextPutAll: name; lf]
		ifFalse: [failed := failed + 1.
			out nextPutAll: 'FAIL '; nextPutAll: name; nextPutAll: ': expected '; nextPutAll: expected printString; nextPutAll: ', got '; nextPutAll: value printString; lf].
	out flush] in: [:check |
	check value: 'backend' value: #TFFIBackend value: [FFIBackend current class name].
	check value: 'environment' value: phase value: [
		env at: 'WASM_FFI_SESSION' put: phase.
		env at: 'WASM_FFI_SESSION'].
	check value: 'environment, removed' value: nil value: [
		env removeKey: 'WASM_FFI_SESSION'.
		env at: 'WASM_FFI_SESSION' ifAbsent: [nil]].
	check value: 'getpid' value: true value: [LibC uniqueInstance currentProcessId > 0].
	"(ENOENT is 44 in Emscripten's C library, which numbers errno as WASI does)"
	check value: 'strerror' value: 'No such file or directory' value: [OSPlatform current getErrorMessage: 44].
	check value: 'qsort with a callback' value: #(1 2 3 5 7 9) value: [| arr cb |
		arr := ByteArray new: 24.
		#(5 3 9 1 7 2) doWithIndex: [:e :i | arr unsignedLongAt: i - 1 * 4 + 1 put: e bigEndian: false].
		cb := FFICallback signature: #(int (void *a, void *b)) block: [:a :b | (a signedLongAt: 1) - (b signedLongAt: 1)].
		(Smalltalk at: #WasmFFISessionCalls) new qsort: arr count: 6 size: 4 with: cb.
		(1 to: 6) collect: [:i | arr unsignedLongAt: i - 1 * 4 + 1 bigEndian: false]].
	phase = 'reload' ifTrue: [
		check value: 'the marker of the saved image' value: 'saved with the FFI' value: [Smalltalk at: #WasmFFISession]]].
out nextPutAll: 'ffi-session '; nextPutAll: phase; nextPutAll: ': '; print: checks; nextPutAll: ' checks, '; print: failed; nextPutAll: ' failed'; lf; flush.
phase = 'first' ifTrue: [
	Smalltalk at: #WasmFFISession put: 'saved with the FFI'.
	Smalltalk snapshot: true andQuit: true].
