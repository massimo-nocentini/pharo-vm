"Lane 22-old-space: old space grows again where it shrank.

	PHARO_WASM_OLD_SPACE_BASE=0x20000000 pharo --headless Pharo.image --no-default-preferences st --quit oldspace-regrow.st

 ByteArrays of 50, 100 and 150 MB, each garbage collected before the next
 one: each grows old space by a segment of 64, 128 and 256 MB at its top,
 and the full GC frees that segment again.  The next, larger segment must
 go where the freed one was (a free range that ends at the break grows with
 it), or the third one no longer fits in a window of 512 MB.  Prints
 whether each ByteArray could be allocated, #(true true true)."

| results |
results := #(50 100 150) collect: [ :megabytes | | fits |
	fits := [ (ByteArray new: megabytes * 1024 * 1024) size > 0 ]
		on: OutOfMemory
		do: [ :e | false ].
	Smalltalk garbageCollect; garbageCollect.
	fits ].
Stdio stdout
	nextPutAll: results printString;
	lf;
	flush.
