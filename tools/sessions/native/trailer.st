"trailer.st - the end of the wrapper of run.sh, after the script: closes its block and handles its errors"
 ] on: Error, UnhandledError do: [ :e | Stdio stdout nextPutAll: 'ERROR: ', e description; lf; nextPutAll: (String streamContents: [:s | e signalerContext ifNotNil: [:c | (c stack first: (c stack size min: 25)) do: [:x | s print: x; lf ] ] ]); flush.
	Smalltalk exitFailure ].
Smalltalk snapshot: false andQuit: true.
