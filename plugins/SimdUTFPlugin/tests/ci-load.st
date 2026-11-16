"Load the SimdUTFPlugin of the VM, as the CI jobs do with the VMMaker image, which has no
SimdUTF package: this file compiles its own primitive methods.  Asks primitiveInfo, encodes
a string of 4 characters (1, 2, 3 and 4 UTF-8 bytes) into UTF-8, decodes it back, and
validates an overlong sequence, which must answer the status word of ERROR OVERLONG at byte
1.  Prints

	SimdUTFPlugin <platform> <kernel> <simdutf version> forced <forced or -> ok

and exits 0; else prints 'SimdUTFPlugin FAILED ...' or 'SimdUTFPlugin did not load' (which
the jobs' grep of 'SimdUTFPlugin native' does not match) and exits 1.  <forced> is slot 7
of primitiveInfo: the SIMDUTF_FORCE_IMPLEMENTATION that the plugin honoured, or 'refused
<value>' (docs/SimdUTF.md).

Then calls libSimdUTFFFI (the package SimdUTF-FFI) through the FFI, found as SimdUTFLibrary
finds it: prints 'SimdUTFFFI <protocol> <kernel> forced <forced or ->', or 'SimdUTFFFI not
loaded: <error>'; that line does not change the exit status (the Linux job greps it)"

| info source utf8 encoded decoded validated overlong ok forced ffi |
Object compile: 'suCiInfo <primitive: ''primitiveInfo'' module: ''SimdUTFPlugin''> ^ nil' classified: 'probe'.
Object compile: 'suCiEncode: s from: a to: b flags: f into: d at: i
	<primitive: ''primitiveEncode'' module: ''SimdUTFPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'suCiDecode: s from: a to: b flags: f wideStringClass: w into: d at: i
	<primitive: ''primitiveDecode'' module: ''SimdUTFPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'suCiValidate: s from: a to: b flags: f
	<primitive: ''primitiveValidate'' module: ''SimdUTFPlugin'' error: ec> ^ ec' classified: 'probe'.
info := nil suCiInfo.
source := WideString withAll: (#( 97 16rE9 16r20AC 16r1F600 ) collect: [ :each | Character value: each ]).
utf8 := #[ 97 195 169 226 130 172 240 159 152 128 ].
overlong := (2 + (4 * 4) + 4096) negated.	"status 2 ERROR, code 4 OVERLONG, at byte 1"
ok := false.
info ifNotNil: [
	encoded := nil suCiEncode: source from: 1 to: source size flags: 0 into: nil at: 1.
	decoded := nil suCiDecode: utf8 from: 1 to: utf8 size flags: 0 wideStringClass: WideString into: nil at: 1.
	validated := nil suCiValidate: #[ 16rF0 16r8F 16rBF 16rBF ] from: 1 to: 4 flags: 0.
	ok := info first = 1
		and: [ (info at: 3) = '9.4.0'
		and: [ (info at: 4) ~= 'unsupported'
		and: [ encoded = utf8
		and: [ decoded = source
		and: [ validated = overlong ] ] ] ] ] ].
forced := [ :value | (value isNil or: [ value isEmpty ]) ifTrue: [ '-' ] ifFalse: [ value ] ].
Stdio stdout
	nextPutAll: (info
		ifNil: [ 'SimdUTFPlugin did not load' ]
		ifNotNil: [
			'SimdUTFPlugin ', (ok ifTrue: [ '' ] ifFalse: [ 'FAILED ' ]), (info at: 5), ' ', (info at: 4), ' ',
				(info at: 3), ' forced ', (forced value: (info at: 7)),
				(ok
					ifTrue: [ ' ok' ]
					ifFalse: [ ' encode ', encoded printString, ' decode ', decoded printString,
						' validate ', validated printString ]) ]);
	nextPutAll: OSPlatform current lineEnding;
	flush.
ffi := [ | platform path |
	platform := OSPlatform current.
	path := platform isWindows
		ifTrue: [ FFIWindowsLibraryFinder findAnyLibrary: #( 'SimdUTFFFI.dll' 'libSimdUTFFFI.dll' ) ]
		ifFalse: [
			platform isMacOS
				ifTrue: [ FFIMacLibraryFinder findLibrary: 'libSimdUTFFFI.dylib' ]
				ifFalse: [ FFIUnix64LibraryFinder findLibrary: 'libSimdUTFFFI.so' ] ].
	Object compile: 'suCiFFIProtocol ^ self ffiCall: #( int64 su_ffi_protocol ( ) ) module: ' , path printString classified: 'probe'.
	Object compile: 'suCiFFIImplementation ^ self ffiCall: #( String su_ffi_implementation ( ) ) module: ' , path printString classified: 'probe'.
	Object compile: 'suCiFFIForced ^ self ffiCall: #( String su_ffi_forced ( ) ) module: ' , path printString classified: 'probe'.
	'SimdUTFFFI ' , nil suCiFFIProtocol printString , ' ' , nil suCiFFIImplementation , ' forced ' , (forced value: nil suCiFFIForced) ]
		on: Error
		do: [ :error | error return: 'SimdUTFFFI not loaded: ' , error description ].
Stdio stdout nextPutAll: ffi; nextPutAll: OSPlatform current lineEnding; flush.
Smalltalk exit: (ok ifTrue: [ 0 ] ifFalse: [ 1 ])
