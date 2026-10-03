"S14 of wasm-smoke.sh: save the image with a marker, then quit.  The saved
 image must answer the marker, both in wasm and on a native Pharo VM:

	pharo --headless Pharo.image --no-default-preferences st snapshot.st
	pharo --headless Pharo.image --no-default-preferences eval 'Smalltalk at: #WasmSnapshotMarker'

 prints 'saved by wasm64 42'."

Smalltalk
	at: #WasmSnapshotMarker
	put: 'saved by ' , Smalltalk vm architectureName , ' ' , (6 * 7) printString.
Smalltalk snapshot: true andQuit: true.
