"Encodes known Forms with the plugin and writes the JPEGs out for checking."
| out sizes |
ByteArray compile: 'jrwCompressSize
	<primitive: ''primJPEGCompressStructSize'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwErrorSize
	<primitive: ''primJPEGErrorMgr2StructSize'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwWrite: cs to: dest form: f quality: q prog: p err: em
	<primitive: ''primJPEGWriteImageonByteArrayformqualityprogressiveJPEGerrorMgr'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.

(FileSystem disk workingDirectory / 'enc') ensureCreateDirectory.
out := WriteStream on: String new.
sizes := OrderedCollection new.

#( #(64 48 32 90 false) #(33 17 32 90 false) #(32 32 8 90 false)
   #(40 40 32 80 true) #(50 37 32 25 false) ) do: [:spec |
	| w h depth quality prog form cs em dest n name |
	w := spec at: 1. h := spec at: 2. depth := spec at: 3.
	quality := spec at: 4. prog := spec at: 5.
	form := Form extent: w @ h depth: depth.
	"Deterministic gradient, so the decoded result can be checked."
	0 to: h - 1 do: [:y |
		0 to: w - 1 do: [:x |
			| r g b |
			r := (x * 255) // ((w - 1) max: 1).
			g := (y * 255) // ((h - 1) max: 1).
			b := ((x + y) * 255) // ((w + h - 2) max: 1).
			form colorAt: x @ y put: (Color r: r g: g b: b range: 255) ] ].
	cs := ByteArray new: #[1] jrwCompressSize.
	em := ByteArray new: #[1] jrwErrorSize.
	dest := ByteArray new: (w * h * 4 + 65536).
	n := #[1] jrwWrite: cs to: dest form: form quality: quality prog: prog err: em.
	name := 'enc_', w printString, 'x', h printString, '_d', depth printString,
		'_q', quality printString, (prog ifTrue: ['_prog'] ifFalse: ['']), '.jpg'.
	out nextPutAll: name, ' -> ', n printString, ' bytes'; cr.
	(n isInteger and: [n > 0]) ifTrue: [
		((FileSystem disk workingDirectory / 'enc') / name)
			binaryWriteStreamDo: [:st | st nextPutAll: (dest first: n) ] ] ].

Transcript show: out contents; cr; flush.
