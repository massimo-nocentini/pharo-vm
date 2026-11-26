"bloc-cursor.st - the hand of the world after a space of a Morphic host of Bloc sets its cursor:
Bloc's cursor: moves it by the offset of the cursor (-1@-1, so 499@249 -> 500@250), the one of
PharoBloc-Web keeps it.  The probe of the failure of lane 80 on the Bloc image (c39164fef).
Run on an image with Bloc and without that fix (a Bloc image built before it), from the root of
the checkout, whose packaging/bloc/src it installs PharoBloc-Web from (BLOC_SRC another one):

	pharo --headless Pharo.image --no-default-preferences st --quit tools/sessions/probes/bloc-cursor.st"
| out probe |
out := Stdio stdout.
probe := [ :label |
	#( #BlMorphicWindowHostSpace #BlMorphicHostSpace ) do: [ :name |
		| hand |
		hand := World activeHand.
		hand position: 499 @ 249.
		(Smalltalk globals at: name) basicNew cursor: Cursor normal.
		out nextPutAll: label , ' ' , name , ': ' , hand position printString; lf.
		(Smalltalk globals at: name) basicNew cursor: nil ] ].
probe value: 'before'.
(TonelReader
	on: (OSEnvironment current at: 'BLOC_SRC' ifAbsent: [ 'packaging/bloc/src' ]) asFileReference
	fileName: 'PharoBloc-Web') snapshot install.
probe value: 'PharoBloc-Web'.
out flush.
