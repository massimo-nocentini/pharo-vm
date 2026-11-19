"Load the UtilsPlugin of the VM, as the CI jobs do with the VMMaker image, which has no
VM-Plugins-Primitives package: this file compiles its own primitive methods.  Asks
primitive_info, and checks one known answer of each primitive:
- primitive_fma: (1 + 2^-52)(1 - 2^-53) - 1 rounded once, 2^-53 - 2^-105 (0 when the product is
  rounded first);
- primitive_strtok_r: the lines of 'a<lf><lf>b<lf>', #('a' '' 'b'), and its words;
- primitive_decasteljau: the middle of the segment 0@0 10@20, 5.0@10.0;
- primitive_randomknuth_*: the first fraction of the seed 310952, and Knuth's own check of his
  rng-double.c: ran_u[0] is 0.36410514377569680455 after 2009 cycles of 1009 fractions;
- primitive_splitmix64_*: the first output of the seed 1234567 of Vigna's splitmix64.c,
  16r599ED017FB08FC85, and its state advanced by 16r9E3779B97F4A7C15; the first fraction of the seed
  0; the 1000th output of 1234567 (16r8B33CC4945DD1C5F), from a fill of 1000.
Prints

	UtilsPlugin <platform> <protocol> ok

and exits 0; else prints 'UtilsPlugin FAILED ...' or 'UtilsPlugin did not load' (which the
jobs' grep of 'UtilsPlugin native' does not match) and exits 1."

| lineEnd info fail lf lines state generator fraction bitsOf point outputs |
lineEnd := OSPlatform current lineEnding.
Object compile: 'upCiInfo <primitive: ''primitive_info'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Float compile: 'upCiTimes: y plus: z <primitive: ''primitive_fma'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
String compile: 'upCiStrtok: d inclusive: b <primitive: ''primitive_strtok_r'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiDeCasteljau: c into: p at: t <primitive: ''primitive_decasteljau'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiKnuthState <primitive: ''primitive_randomknuth_init'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiKnuthStart <primitive: ''primitive_randomknuth_start'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiKnuthNext: s <primitive: ''primitive_randomknuth_next'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiSplitMix64Next: s <primitive: ''primitive_splitmix64_next'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiSplitMix64NextFloat: s <primitive: ''primitive_splitmix64_nextfloat'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.
Object compile: 'upCiSplitMix64Fill: s into: a <primitive: ''primitive_splitmix64_fill'' module: ''UtilsPlugin''> ^ nil' classified: 'probe'.

fail := [ :message |
	Stdio stdout nextPutAll: 'UtilsPlugin FAILED '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
"the bits of a little-endian double of a ByteArray"
bitsOf := [ :bytes | (1 to: 8) inject: 0 into: [ :sum :index | sum + ((bytes at: index) bitShift: index - 1 * 8) ] ].

info := nil upCiInfo.
info ifNil: [
	Stdio stdout nextPutAll: 'UtilsPlugin did not load'; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
(info isArray and: [ info size = 3 and: [ info first = 1 and: [ info second = 'UtilsPlugin 1.1' ] ] ])
	ifFalse: [ fail value: 'primitive_info answers ' , info printString ].

((1.0 + (1 / (2 raisedTo: 52)) asFloat) upCiTimes: 1.0 - (1 / (2 raisedTo: 53)) asFloat plus: -1.0)
	= ((1 / (2 raisedTo: 53)) - (1 / (2 raisedTo: 105)))
		ifFalse: [ fail value: 'primitive_fma does not round once' ].

lf := String with: Character lf.
lines := 'a' , lf , lf , 'b' , lf upCiStrtok: lf inclusive: true.
lines = #( 'a' '' 'b' ) ifFalse: [ fail value: 'primitive_strtok_r answers the lines ' , lines printString ].
lines := ' a  b ' upCiStrtok: ' ' inclusive: false.
lines = #( 'a' 'b' ) ifFalse: [ fail value: 'primitive_strtok_r answers the words ' , lines printString ].

point := Array new: 1.
"it answers its receiver"
(0 upCiDeCasteljau: { 0 @ 0. 10 @ 20 } into: point at: #( 0.5 )) = 0 ifFalse: [ fail value: 'primitive_decasteljau failed' ].
point first = (5.0 @ 10.0) ifFalse: [ fail value: 'primitive_decasteljau answers ' , point printString ].

state := nil upCiKnuthState.
state ifNil: [ fail value: 'primitive_randomknuth_init failed' ].
"the receiver of primitive_randomknuth_start: the state, then the seed (KnuthRandom's slots)"
generator := Array with: state with: 310952.
generator upCiKnuthStart == generator ifFalse: [ fail value: 'primitive_randomknuth_start failed' ].
fraction := nil upCiKnuthNext: state.
fraction = (Float fromIEEE64Bit: 16r3FC4F4EEA4AAF860)
	ifFalse: [ fail value: 'the first fraction of 310952 is ' , fraction printString ].
"the 200801st fraction starts the 2009th cycle"
200800 timesRepeat: [ nil upCiKnuthNext: state ].
(bitsOf value: state first) = 16r3FD74D7FA9349B94
	ifFalse: [ fail value: 'Knuth''s check: ran_u[0] is not 0.36410514377569680455' ].

"the state of SplitMix64: a DoubleWordArray of 1, Vigna's x"
state := DoubleWordArray with: 1234567.
(nil upCiSplitMix64Next: state) = 16r599ED017FB08FC85
	ifFalse: [ fail value: 'primitive_splitmix64_next: the first output of 1234567 is not 16r599ED017FB08FC85' ].
state first = (1234567 + 16r9E3779B97F4A7C15)
	ifFalse: [ fail value: 'primitive_splitmix64_next: the state of 1234567 is ' , state first printString ].
fraction := nil upCiSplitMix64NextFloat: (DoubleWordArray with: 0).
fraction = (Float fromIEEE64Bit: 16r3FEC4415072F63B9)
	ifFalse: [ fail value: 'primitive_splitmix64_nextfloat: the first fraction of 0 is ' , fraction printString ].
outputs := DoubleWordArray new: 1000.
(nil upCiSplitMix64Fill: (DoubleWordArray with: 1234567) into: outputs) == outputs
	ifFalse: [ fail value: 'primitive_splitmix64_fill failed' ].
(outputs first = 16r599ED017FB08FC85 and: [ outputs last = 16r8B33CC4945DD1C5F ])
	ifFalse: [ fail value: 'primitive_splitmix64_fill: the outputs 1 and 1000 of 1234567 are ' , outputs first printString , ' and ' , outputs last printString ].

Stdio stdout
	nextPutAll: 'UtilsPlugin '; nextPutAll: info third;
	nextPutAll: ' '; nextPutAll: info first printString;
	nextPutAll: ' ok'; nextPutAll: lineEnd; flush.
Smalltalk snapshot: false andQuit: true.
