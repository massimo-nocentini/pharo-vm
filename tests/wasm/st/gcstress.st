"S9, S20 and S21 of wasm-smoke.sh: three million live Arrays make old space
 grow past 200 MB, and a full GC keeps them all.

	pharo --headless Pharo.image --no-default-preferences st --quit gcstress.st

 prints the number of Arrays, 3000000, and the size of old space in bytes (VM
 parameter 1), which is more than 200000000.  That size depends on when the
 scavenges and the growths of old space happened, so it differs a little from
 run to run (221511680 or 238288896, whatever the slice length)."

| arrays |
arrays := OrderedCollection new.
1 to: 3000000 do: [ :i | arrays add: (Array new: 3) ].
Smalltalk garbageCollect.
Stdio stdout
	print: arrays size;
	nextPut: Character space;
	print: (Smalltalk vm parameterAt: 1);
	lf;
	flush.
