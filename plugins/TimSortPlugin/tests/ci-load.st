"Load the TimSortPlugin of the VM, as the CI jobs do with the VMMaker image, which has no
TimSort package: this file compiles its own primitive methods.  Asks primitiveInfo, sorts an
Array of numbers of every kind (SmallInteger, Float, LargePositiveInteger, -0.0) and one of
the four string classes (stable, by code point), a Float64Array with a NaN (last), argsorts an
Array of keys, and, when primitiveInfo says the VM runs resumable sorts, sorts an Array by a
sortBlock through the coroutine primitives: each request is answered with the block, the
permutation is applied, and the handle is freed (a second free answers false, and no sort is
left live).  Prints

	TimSortPlugin <platform> timsort.c <commit> coroutines <yes N | no> ok

(N: the comparisons of the sort by the block) and exits 0; else prints 'TimSortPlugin FAILED
...' or 'TimSortPlugin did not load' (which the jobs' grep of 'TimSortPlugin native' does not
match) and exits 1.

Then calls libTimSortFFI (the package TimSort-FFI) through the FFI, found as TimSortLibrary
finds it: sorts 5 int64 in place, and runs the same sort by a sortBlock through ts_ffi_coro_*;
prints 'TimSortFFI <protocol> <commit> <stack switch> coroutines <yes N | no> ok', 'TimSortFFI
FAILED ...', or 'TimSortFFI not loaded: <error>'; that line does not change the exit status
(the jobs grep it).
TimSort sorts by a sortBlock with its Smalltalk port by default; the resumable C sort is what
TimSort sort:with:backend: #plugin or #ffi runs, and these probes drive it directly, so that CI
runs the stack switches of every OS."

| lineEnd info numbers strings buffer keys perm ok coroutine sortBy plugin ffi |
lineEnd := OSPlatform current lineEnding.
Object compile: 'tsCiInfo <primitive: ''primitiveInfo'' module: ''TimSortPlugin''> ^ nil' classified: 'probe'.
Object compile: 'tsCiSort: a from: s to: e descending: d classes: c
	<primitive: ''primitiveSortObjects'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiSortBuffer: b kind: k from: s to: e descending: d
	<primitive: ''primitiveSortBuffer'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiArgSort: keys kind: k into: p from: s to: e descending: d classes: c
	<primitive: ''primitiveArgSort'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiNew: n descending: d
	<primitive: ''primitiveBlockSortNew'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiNext: id answer: a
	<primitive: ''primitiveBlockSortNext'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiPermutation: id into: p
	<primitive: ''primitiveBlockSortPermutation'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'tsCiFree: id
	<primitive: ''primitiveBlockSortFree'' module: ''TimSortPlugin'' error: ec> ^ ec' classified: 'probe'.

"The sort by a sortBlock, driven by next: one request (i << 32) | j (0-based) a comparison, answered
1 when element i must come before element j, (block value: cj value: ci) not; answers the sorted
copy and the number of comparisons, or the failure"
sortBy := [ :items :block :new :next :permutation :free |
	| id request comparisons answer order result |
	id := new value: items size.
	id isInteger
		ifFalse: [ { 'new'. id } ]
		ifTrue: [
			comparisons := 0.
			answer := 0.
			[ (request := next value: id value: answer) isInteger and: [ request >= 0 ] ] whileTrue: [
				comparisons := comparisons + 1.
				answer := (block
					value: (items at: (request bitAnd: 16rFFFFFFFF) + 1)
					value: (items at: (request bitShift: -32) + 1))
						ifTrue: [ 0 ]
						ifFalse: [ 1 ] ].
			order := request = -1 ifTrue: [ permutation value: id value: items size ].
			result := { order isArray ifTrue: [ order collect: [ :k | items at: k ] ] ifFalse: [ request -> order ]. comparisons }.
			{ result first. result second. free value: id. free value: id } ] ].

info := nil tsCiInfo.
ok := false.
coroutine := 'no'.
info ifNotNil: [
	numbers := { 3. -1. 2.5. 1 bitShift: 70. -7.0e300. -0.0. 0 }.
	nil tsCiSort: numbers from: 1 to: numbers size descending: false classes: { ByteString. ByteSymbol. WideString. WideSymbol }.
	strings := { 'pear'. #apple. 'Zebra'. 'apple'. 'é' asWideString }.
	nil tsCiSort: strings from: 1 to: strings size descending: false classes: { ByteString. ByteSymbol. WideString. WideSymbol }.
	buffer := Float64Array new: 4.
	buffer at: 1 put: 3.0; at: 2 put: Float nan; at: 3 put: -0.0; at: 4 put: 1.5.
	nil tsCiSortBuffer: buffer kind: 1 from: 1 to: 4 descending: false.
	keys := #( 30 10 20 ).
	perm := Array new: 3.
	nil tsCiArgSort: keys kind: nil into: perm from: 1 to: 3 descending: false classes: { ByteString. ByteSymbol. WideString. WideSymbol }.
	(info at: 5) == true ifTrue: [
		coroutine := sortBy valueWithArguments: {
			#( 5 3 9 1 7 3 ).
			[ :a :b | a >= b ].
			[ :n | nil tsCiNew: n descending: false ].
			[ :id :answer | nil tsCiNext: id answer: answer ].
			[ :id :n | nil tsCiPermutation: id into: (Array new: n) ].
			[ :id | nil tsCiFree: id ] }.
		coroutine := (coroutine size = 4
			and: [ coroutine first = #( 9 7 5 3 3 1 )
			and: [ (coroutine at: 3) == true
			and: [ (coroutine at: 4) == false
			and: [ (nil tsCiInfo at: 6) = 0 ] ] ] ])
				ifTrue: [ 'yes ' , coroutine second printString ]
				ifFalse: [ 'FAILED ' , coroutine printString ] ].
	ok := info first = 1
		and: [ (info at: 3) = 'timsort.c fd70381'
		and: [ numbers = { -7.0e300. -1. -0.0. 0. 2.5. 3. 1 bitShift: 70 }
		and: [ (strings collect: [ :each | each printString ]) = #( '''Zebra''' '#apple' '''apple''' '''pear''' '''é''' )
		and: [ (buffer first: 3) asArray = #( 0.0 1.5 3.0 ) and: [ buffer last isNaN
		and: [ perm = #( 2 3 1 )
		and: [ (coroutine beginsWith: 'FAILED') not ] ] ] ] ] ] ] ].
plugin := info
	ifNil: [ 'TimSortPlugin did not load' ]
	ifNotNil: [
		'TimSortPlugin ', (ok ifTrue: [ '' ] ifFalse: [ 'FAILED ' ]), (info at: 4), ' ', (info at: 3),
			' coroutines ', coroutine,
			(ok
				ifTrue: [ ' ok' ]
				ifFalse: [ ' info ', info printString, ' numbers ', numbers printString, ' strings ', strings printString,
					' buffer ', buffer printString, ' perm ', perm printString ]) ].
Stdio stdout nextPutAll: plugin; nextPutAll: lineEnd; flush.

ffi := [ | platform path protocol commit switch words sorted coroutines |
	platform := OSPlatform current.
	path := platform isWindows
		ifTrue: [ FFIWindowsLibraryFinder findAnyLibrary: #( 'TimSortFFI.dll' 'libTimSortFFI.dll' ) ]
		ifFalse: [
			platform isMacOS
				ifTrue: [ FFIMacLibraryFinder findLibrary: 'libTimSortFFI.dylib' ]
				ifFalse: [ FFIUnix64LibraryFinder findLibrary: 'libTimSortFFI.so' ] ].
	#( 'tsCiFFIProtocol ^ self ffiCall: #( int64 ts_ffi_protocol ( ) ) module: '
	   'tsCiFFICommit ^ self ffiCall: #( String ts_ffi_timsort_version ( ) ) module: '
	   'tsCiFFISwitch ^ self ffiCall: #( String ts_ffi_coro_impl ( ) ) module: '
	   'tsCiFFISupported ^ self ffiCall: #( int64 ts_ffi_coro_supported ( ) ) module: '
	   'tsCiFFISort: base n: n descending: d ^ self ffiCall: #( int64 ts_ffi_sort_i64 ( void * base , int64 n , int64 d ) ) module: '
	   'tsCiFFINew: n descending: d ^ self ffiCall: #( int64 ts_ffi_coro_new ( int64 n , int64 d ) ) module: '
	   'tsCiFFINext: id answer: a ^ self ffiCall: #( int64 ts_ffi_coro_next ( int64 id , int64 a ) ) module: '
	   'tsCiFFIPermutation: id into: dst n: n ^ self ffiCall: #( int64 ts_ffi_coro_permutation ( int64 id , void * dst , int64 n ) ) module: '
	   'tsCiFFIFree: id ^ self ffiCall: #( int64 ts_ffi_coro_free ( int64 id ) ) module: '
	   'tsCiFFILive ^ self ffiCall: #( int64 ts_ffi_coro_live ( ) ) module: ' ) do: [ :source |
		Object compile: source , path printString classified: 'probe' ].
	protocol := nil tsCiFFIProtocol.
	commit := nil tsCiFFICommit.
	switch := nil tsCiFFISwitch.
	words := ByteArray new: 40.
	#( 4 -2 9 0 -7 ) doWithIndex: [ :each :k | words integerAt: k - 1 * 8 + 1 put: each size: 8 signed: true ].
	sorted := { nil tsCiFFISort: words n: 5 descending: 0.
		(1 to: 5) collect: [ :k | words integerAt: k - 1 * 8 + 1 size: 8 signed: true ] }.
	coroutines := nil tsCiFFISupported = 1
		ifFalse: [ 'no' ]
		ifTrue: [ | result |
			result := sortBy valueWithArguments: {
				#( 5 3 9 1 7 3 ).
				[ :a :b | a >= b ].
				[ :n | nil tsCiFFINew: n descending: 0 ].
				[ :id :answer | nil tsCiFFINext: id answer: answer ].
				[ :id :n | | indices |
					indices := ByteArray new: n * 8.
					(nil tsCiFFIPermutation: id into: indices n: n) = 0 ifTrue: [
						(1 to: n) collect: [ :k | (indices integerAt: k - 1 * 8 + 1 size: 8 signed: true) + 1 ] ] ].
				[ :id | nil tsCiFFIFree: id ] }.
			(result size = 4
				and: [ result first = #( 9 7 5 3 3 1 )
				and: [ (result at: 3) = 0
				and: [ (result at: 4) = -4
				and: [ nil tsCiFFILive = 0 ] ] ] ])
					ifTrue: [ 'yes ' , result second printString ]
					ifFalse: [ 'FAILED ' , result printString ] ].
	(protocol = 1 and: [ sorted = #( 0 #( -7 -2 0 4 9 ) ) and: [ (coroutines beginsWith: 'FAILED') not ] ])
		ifTrue: [ 'TimSortFFI ' , protocol printString , ' ' , commit , ' ' , switch , ' coroutines ' , coroutines , ' ok' ]
		ifFalse: [ 'TimSortFFI FAILED ' , protocol printString , ' ' , commit printString , ' ' , switch printString
			, ' sort ' , sorted printString , ' coroutines ' , coroutines ] ]
		on: Error
		do: [ :error | error return: 'TimSortFFI not loaded: ' , error description ].
Stdio stdout nextPutAll: ffi; nextPutAll: lineEnd; flush.
Smalltalk exit: (ok ifTrue: [ 0 ] ifFalse: [ 1 ])
