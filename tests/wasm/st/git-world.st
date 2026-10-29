"Evaluations in the UI process of the world, for tests/wasm/git.spec.mjs, which boots the world
image with it as tests/wasm/world.spec.mjs boots it with world-probe.st:

	--headless /pharo/Pharo.image --no-default-preferences --interactive st /pharo/st/git-world.st

The spec writes Smalltalk source into git-eval.st next to the image (with the fs requests of the
worker, PharoWorld.fs); a process of this file, at userBackgroundPriority, reads it and deletes
it, and has the UI process evaluate it in its next cycle (UIManager default defer:), as a click
of the user would run it: an Iceberg push there runs in the UI process, and what that defers
(the dialog of OSWebCredentialsProvider) opens in a later cycle.  The deferred evaluations of the
UI process also run while a modal dialog is open, in the loop of the dialog: an evaluation can
answer that dialog.  Once evaluated, the UI process writes git-eval.out, next to the image:
'<n> <the printString of the value, or error <class>: <messageText>>' and a last line '<end>',
<n> counting the requests from 1.  Loaded again, as after a snapshot of an image it ran in, it
stops the process of the earlier load."

| directory |
directory := FileLocator imageDirectory asFileReference.
(Smalltalk globals at: #GitWorldEvaluator ifAbsent: [ nil ]) ifNotNil: [ :old | old terminate ].
Smalltalk globals
	at: #GitWorldEvaluator
	put: ([
		| count |
		count := 0.
		[
		(Delay forMilliseconds: 100) wait.
		(directory / 'git-eval.st') exists ifTrue: [
			| source |
			source := (directory / 'git-eval.st') contents.
			(directory / 'git-eval.st') ensureDelete.
			count := count + 1.
			[ :n :code |
			UIManager default defer: [
				| value |
				value := [ (Smalltalk compiler evaluate: code) printString ]
					on: Error
					do: [ :error | 'error ' , error class name , ': ' , error messageText asString ].
				(directory / 'git-eval.out') ensureDelete; writeStreamDo: [ :stream |
					stream nextPutAll: n printString; space; nextPutAll: value; lf; nextPutAll: '<end>'; lf ] ] ]
				value: count
				value: source ] ] repeat ] forkAt: Processor userBackgroundPriority)
