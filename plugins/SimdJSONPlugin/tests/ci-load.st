"Load the SimdJSONPlugin of the VM, as the CI jobs do with the VMMaker image, and write a
small value with primitiveWrite: prints 'SimdJSONPlugin <platform> <implementation> wrote
<json>' and exits 0, else prints 'SimdJSONPlugin FAILED ...' (which the jobs' grep of
'SimdJSONPlugin native' does not match) and exits 1.

Then call libSimdJSONFFI (the package SimdJSON-FFI) through the FFI, found as
SimdJSONLibrary finds it: prints 'SimdJSONFFI <protocol> <implementation>', or
'SimdJSONFFI not loaded: <error>'; that line does not change the exit status (the Linux
job greps it)"

| info layout written ok ffi |
Object compile: 'sjCiInfo <primitive: ''primitiveInfo'' module: ''SimdJSONPlugin''> ^ nil' classified: 'probe'.
Object compile: 'sjCiWrite: v flags: f newLine: n level: e maxDepth: d maxBytes: b classes: c layout: l
	<primitive: ''primitiveWrite'' module: ''SimdJSONPlugin''> ^ nil' classified: 'probe'.
info := nil sjCiInfo.
layout := ((Dictionary allInstVarNames indexOf: #tally) - 1)
	+ (((Dictionary allInstVarNames indexOf: #array) - 1) << 4)
	+ (((Association allInstVarNames indexOf: #key) - 1) << 8)
	+ (((Association allInstVarNames indexOf: #value) - 1) << 12)
	+ (((OrderedCollection allInstVarNames indexOf: #array) - 1) << 16)
	+ (((OrderedCollection allInstVarNames indexOf: #firstIndex) - 1) << 20)
	+ (((OrderedCollection allInstVarNames indexOf: #lastIndex) - 1) << 24).
written := nil
	sjCiWrite: { 1. 'a'. 0.5. Dictionary new at: 'k' put: nil; yourself }
	flags: 0 newLine: (String with: Character lf) level: 0 maxDepth: 1024 maxBytes: -1
	classes: { ByteSymbol. WideString. WideSymbol. Dictionary. Association. OrderedCollection }
	layout: layout.
ok := info notNil and: [ written notNil and: [ (written at: 2) = '[1,"a",0.5,{"k":null}]' ] ].
Stdio stdout
	nextPutAll: (info
		ifNil: [ 'SimdJSONPlugin did not load' ]
		ifNotNil: [ 'SimdJSONPlugin ', (ok ifTrue: [ '' ] ifFalse: [ 'FAILED ' ]), (info at: 5), ' ', (info at: 4),
			' wrote ', (written ifNil: [ 'nothing' ] ifNotNil: [ (written at: 2) printString ]) ]);
	nextPutAll: OSPlatform current lineEnding;
	flush.
ffi := [ | platform path |
	platform := OSPlatform current.
	path := platform isWindows
		ifTrue: [ FFIWindowsLibraryFinder findAnyLibrary: #( 'SimdJSONFFI.dll' 'libSimdJSONFFI.dll' ) ]
		ifFalse: [
			platform isMacOS
				ifTrue: [ FFIMacLibraryFinder findLibrary: 'libSimdJSONFFI.dylib' ]
				ifFalse: [ FFIUnix64LibraryFinder findLibrary: 'libSimdJSONFFI.so' ] ].
	Object compile: 'sjCiFFIProtocol ^ self ffiCall: #( int64 sj_ffi_protocol ( ) ) module: ' , path printString classified: 'probe'.
	Object compile: 'sjCiFFIImplementation ^ self ffiCall: #( String sj_ffi_implementation ( ) ) module: ' , path printString classified: 'probe'.
	'SimdJSONFFI ' , nil sjCiFFIProtocol printString , ' ' , nil sjCiFFIImplementation ]
		on: Error
		do: [ :error | error return: 'SimdJSONFFI not loaded: ' , error description ].
Stdio stdout nextPutAll: ffi; nextPutAll: OSPlatform current lineEnding; flush.
Smalltalk exit: (ok ifTrue: [ 0 ] ifFalse: [ 1 ])
