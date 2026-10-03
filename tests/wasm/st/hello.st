"S13 of wasm-smoke.sh: the st command line handler runs a script file, whose
 output is exactly what it writes to Stdio stdout.

	pharo --headless Pharo.image --no-default-preferences st --quit hello.st

 prints two lines, the second one being 7."

Stdio stdout
	nextPutAll: 'Hello from Pharo on ';
	nextPutAll: Smalltalk vm architectureName;
	lf;
	print: 3 + 4;
	lf;
	flush.
