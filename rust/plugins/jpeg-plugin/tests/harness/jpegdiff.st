"Drives the JPEGReadWriter2Plugin primitives directly and prints a signature of
 every decode, so the C and Rust plugins can be compared byte for byte."
| out dir files depths hexOf run |

ByteArray compile: 'jrwDecompressSize
	<primitive: ''primJPEGDecompressStructSize'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwErrorSize
	<primitive: ''primJPEGErrorMgr2StructSize'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwPresent
	<primitive: ''primJPEGPluginIsPresent'' module: ''JPEGReadWriter2Plugin''>
	^ #failed'.
ByteArray compile: 'jrwGray8
	<primitive: ''primSupports8BitGrayscaleJPEGs'' module: ''JPEGReadWriter2Plugin''>
	^ #failed'.
ByteArray compile: 'jrwWidth: ds
	<primitive: ''primImageWidth'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwHeight: ds
	<primitive: ''primImageHeight'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwComponents: ds
	<primitive: ''primImageNumComponents'' module: ''JPEGReadWriter2Plugin''>
	^ -1'.
ByteArray compile: 'jrwReadHeader: ds from: src err: em
	<primitive: ''primJPEGReadHeaderfromByteArrayerrorMgr'' module: ''JPEGReadWriter2Plugin''>
	^ #failed'.
ByteArray compile: 'jrwReadImage: ds from: src onForm: f dither: d err: em
	<primitive: ''primJPEGReadImagefromByteArrayonFormdoDitheringerrorMgr'' module: ''JPEGReadWriter2Plugin''>
	^ #failed'.

(FileSystem disk workingDirectory / 'dump') ensureCreateDirectory.
out := WriteStream on: String new.
hexOf := [:int | int printStringBase: 16 length: 8 padded: true].

out nextPutAll: 'present: ', #[1] jrwPresent printString; cr.
out nextPutAll: 'gray8: ', #[1] jrwGray8 printString; cr.
out nextPutAll: 'dsSize: ', #[1] jrwDecompressSize printString; cr.
out nextPutAll: 'emSize: ', #[1] jrwErrorSize printString; cr.

dir := FileSystem disk workingDirectory / 'jpegs'.
files := (dir children select: [:f | f basename endsWith: '.jpg'])
	asSortedCollection: [:a :b | a basename <= b basename].

depths := #(32 16 -16 8 -8).

run := [:path :depth :dither |
	| bytes ds em w h nc form sig |
	bytes := path binaryReadStream contents.
	ds := ByteArray new: #[1] jrwDecompressSize.
	em := ByteArray new: #[1] jrwErrorSize.
	#[1] jrwReadHeader: ds from: bytes err: em.
	w := #[1] jrwWidth: ds.
	h := #[1] jrwHeight: ds.
	nc := #[1] jrwComponents: ds.
	out nextPutAll: path basename, ' d=', depth printString, ' dith=', dither printString,
		' -> ', w printString, 'x', h printString, ' nc=', nc printString.
	(w > 0 and: [h > 0])
		ifFalse: [ out nextPutAll: ' HEADER-FAIL'; cr ]
		ifTrue: [
			form := Form extent: w @ h depth: depth.
			#[1] jrwReadImage: ds from: bytes onForm: form dither: dither err: em.
			"Dump the raw bitmap so the two runs can be compared numerically:
			 a different decoder cannot be bit-identical to libjpeg, so the
			 comparison has to be a tolerance, not a hash."
			sig := form bits.
			ByteArray adoptInstance: sig.
			((FileSystem disk workingDirectory / 'dump')
				/ (path basename, '_d', depth printString, '_', dither printString, '.bin'))
					binaryWriteStreamDo: [:st | st nextPutAll: sig].
			out nextPutAll: ' bits=', sig size printString, ' bytes-dumped'; cr ] ].

files do: [:f |
	depths do: [:d |
		#(true false) do: [:dither | run value: f value: d value: dither ] ] ].

Transcript show: out contents; cr; flush.
