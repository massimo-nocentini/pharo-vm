"Lane 22-old-space: old space fills its window and stops there, and an
 object at its top is still old for the write barrier.

	PHARO_WASM_OLD_SPACE_BASE=0x10000000 pharo --headless Pharo.image --no-default-preferences st --quit oldspace-window.st

 The masks of the memory map tell old objects by their address, in the
 window [base, 2 * base) of old space, so the VM must answer an OutOfMemory
 rather than grow old space past it: the write barrier would not remember
 an object out there that stores a young one, and a scavenge would leave it
 pointing at garbage.  This fills old space with 10 MB ByteArrays (each in
 a segment of its own) until the OutOfMemory, or 60 of them, which go well
 past a window of 256 MB, and prints 'OutOfMemory' or 'no OutOfMemory' and
 the size of old space in bytes (VM parameter 1).  It then frees the last
 ones, puts an Array where they were, at the top of old space, stores a
 young String in each of its slots, scavenges many times and prints how
 many slots no longer hold their String, 'bad slots: 0'."

| keep outOfMemory big n bad |
keep := OrderedCollection new.
outOfMemory := false.
[ [ keep size < 60 ] whileTrue: [ keep add: (ByteArray new: 10 * 1024 * 1024) ] ]
	on: OutOfMemory
	do: [ :e | outOfMemory := true ].
Stdio stdout
	nextPutAll: (outOfMemory ifTrue: [ 'OutOfMemory' ] ifFalse: [ 'no OutOfMemory' ]);
	nextPut: Character space;
	print: (Smalltalk vm parameterAt: 1);
	lf;
	flush.
keep removeLast: (keep size min: 4).
Smalltalk garbageCollect.
n := 1000000.
big := Array new: n.
1 to: n do: [ :i | big at: i put: i printString ].
1 to: 2000000 do: [ :i | Array new: 4 ].
bad := 0.
1 to: n do: [ :i | (big at: i) = i printString ifFalse: [ bad := bad + 1 ] ].
Stdio stdout
	nextPutAll: 'bad slots: ';
	print: bad;
	lf;
	flush.
