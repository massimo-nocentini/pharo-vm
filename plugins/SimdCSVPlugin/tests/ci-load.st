"Load the SimdCSVPlugin of the VM, as the CI jobs do with the VMMaker image, which has no SimdCSV
package: this file compiles its own primitive methods (docs/SimdCSV.md, Primitive contract).  Asks
primitiveInfo (protocol 1, simdcsv a9494a8, the kernel), and checks known answers, all with ',' and
'""' and the flag FINAL:
- primitiveIndex of 62 'a', a ',' and a field whose opening quote is byte 64 and whose text is in
  the next block of 64 bytes, with a ',' in it: the entries 62, 68, 70 and 72;
- primitiveMeasure of the same bytes, with LINES: 2 records, 4 fields, 1 line end;
- primitiveRows of two records of UTF-8, with the types string, integer and float: a WideString,
  a SmallInteger, a Float, an escaped quote, an odd integer (a String) and an empty field;
- primitiveIndex with STRICT of a quote in an unquoted field: status 2 (ERROR), code 1, byte 4.
Prints

	SimdCSVPlugin <platform> <kernel> a9494a8 1 forced <forced or -> ok

and exits 0; else prints 'SimdCSVPlugin FAILED ...' or 'SimdCSVPlugin did not load' (which the
jobs' grep of 'SimdCSVPlugin native' does not match) and exits 1.  <forced> is slot 7 of
primitiveInfo: the SIMDCSV_FORCE_IMPLEMENTATION that the plugin honoured, or 'refused <value>'."

| lineEnd lf fail expect info dialect final lines strict bytes index rows answer |
lineEnd := OSPlatform current lineEnding.
lf := String with: Character lf.
Object compile: 'simdCSVCiInfo <primitive: ''primitiveInfo'' module: ''SimdCSVPlugin''> ^ nil' classified: 'probe'.
Object compile: 'simdCSVCiMeasure: s from: i to: j dialect: w <primitive: ''primitiveMeasure'' module: ''SimdCSVPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'simdCSVCiIndex: s base: b from: i to: j dialect: w into: d from: k <primitive: ''primitiveIndex'' module: ''SimdCSVPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'simdCSVCiRows: s base: b to: j dialect: w index: ix from: k to: l types: t fieldCount: n into: dest from: d wideStringClass: c empty: e budget: u <primitive: ''primitiveRows'' module: ''SimdCSVPlugin'' error: ec> ^ ec' classified: 'probe'.

fail := [ :message |
	Stdio stdout nextPutAll: 'SimdCSVPlugin FAILED '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
expect := [ :what :actual :expected |
	actual = expected ifFalse: [
		fail value: what , ' answers ' , actual printString , ' and not ' , expected printString ] ].

info := nil simdCSVCiInfo.
info ifNil: [
	Stdio stdout nextPutAll: 'SimdCSVPlugin did not load'; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
(info isArray and: [ info size = 8 and: [ info first = 1 and: [ info second = 'SimdCSVPlugin 1.0'
	and: [ info third = 'a9494a8' and: [ ((info at: 6) includes: info fourth)
	and: [ (#('native' 'emscripten') includes: info fifth) and: [ info last = 4294967294 ] ] ] ] ] ] ])
		ifFalse: [ fail value: 'primitiveInfo answers ' , info printString ].

"the dialect word: the separator, the quote above its 21 bits, the flags above the quote's 8"
dialect := [ :flags | $, codePoint + ($" codePoint bitShift: 21) + (flags bitShift: 29) ].
strict := 1.
final := 32.
lines := 64.

bytes := ((String new: 62 withAll: $a) , ',"x,y"' , lf , 'b,c') asByteArray.
index := WordArray new: 100.
answer := nil simdCSVCiIndex: bytes base: 1 from: 1 to: bytes size dialect: (dialect value: final) into: index from: 1.
expect value: 'primitiveIndex' value: answer value: #(0 4 2 73 0 2 2 0 0).
expect value: 'the index' value: (index first: 4) asArray value: #(62 68 70 72).
answer := nil simdCSVCiMeasure: bytes from: 1 to: bytes size dialect: (dialect value: final + lines).
expect value: 'primitiveMeasure' value: answer value: #(0 2 4 73 1 0 0).

"'e acute, euro', 12, 1.5 and 'a""b', x, nothing"
bytes := #[ 195 169 226 130 172 ] , (',12,1.5' , lf , '"a""b",x,' , lf) asByteArray.
answer := nil simdCSVCiIndex: bytes base: 1 from: 1 to: bytes size dialect: (dialect value: final) into: index from: 1.
expect value: 'primitiveIndex of two records' value: answer value: { 0. 6. 2. bytes size + 1. 0. 3. 3. 0. 0 }.
rows := Array new: 3.
answer := nil simdCSVCiRows: bytes base: 1 to: bytes size dialect: (dialect value: final)
	index: index from: 1 to: 6 types: #[ 1 2 3 ] fieldCount: 3
	into: rows from: 1 wideStringClass: WideString empty: nil budget: -1.
expect value: 'primitiveRows' value: answer value: #(0 2 7 1 0 0 0 0 0).
(rows third isNil and: [ rows first isArray and: [ rows second isArray ] ])
	ifFalse: [ fail value: 'the rows are ' , rows printString ].
expect value: 'row 1' value: rows first value: { WideString with: (Character value: 233) with: (Character value: 8364). 12. 1.5 }.
expect value: 'row 2' value: rows second value: { 'a"b'. 'x'. nil }.
((rows first collect: [ :each | each class name asString ]) = { 'WideString'. 'SmallInteger'. 'SmallFloat64' }
	and: [ rows second first class == ByteString ])
		ifFalse: [ fail value: 'the classes of the cells of ' , rows printString ].

bytes := 'a,b"' asByteArray.
answer := nil simdCSVCiIndex: bytes base: 1 from: 1 to: bytes size dialect: (dialect value: final + strict) into: index from: 1.
expect value: 'primitiveIndex, strict,' value: answer value: #(2 0 0 1 0 0 0 1 4).

Stdio stdout
	nextPutAll: 'SimdCSVPlugin '; nextPutAll: info fifth;
	nextPutAll: ' '; nextPutAll: info fourth;
	nextPutAll: ' '; nextPutAll: info third;
	nextPutAll: ' '; nextPutAll: info first printString;
	nextPutAll: ' forced '; nextPutAll: ((info at: 7) ifNil: [ '-' ]);
	nextPutAll: ' ok'; nextPutAll: lineEnd; flush.
Smalltalk snapshot: false andQuit: true.
