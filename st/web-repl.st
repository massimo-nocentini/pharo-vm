"The read-eval-print loop of the Console page.  The page boots the image with

	--headless /pharo/Pharo.image --no-default-preferences st --no-source /pharo/st/web-repl.st

 (PharoVMDriver.vmArgs('console') of packaging/emscripten/web/vm-driver.js), so this
 script is filed in at every boot, also of an image that was saved from the page, and
 its source is not logged to the .changes each time.  It defines the class WebRepl and
 starts it.

 WebRepl reads stdin, which the page feeds with what the user types.  It evaluates
 every line, up to its LF, prints the value on stdout, or the error it raised on
 stderr, and prompts again with 'st> '.  A CR does not end a line, so the page sends
 the line breaks of multi-line input as CRs, which the compiler reads as white space.
 At the end of the input the image quits.

 WebRepl is also the error handler of every other process (ErrorHandler
 defaultErrorHandler, or ErrorHandler default in Pharo 15): an error that nothing
 handles in a process that an evaluation forked, or in a block that a Delay runs, is
 reported on stderr as well, and only that process is terminated, where the headless
 image would quit.  A Warning that nothing handles is reported, and its process goes on,
 as in the headless image.

 The page's Stop calls vm_interrupt(), which signals the semaphore that WebRepl
 registered with the WebHostPlugin: the evaluation that runs is terminated.

 The page offers to open the world when the image has OSWindow-Web: its worker names
 a file in PHARO_WEB_WORLD_FILE, where WebRepl writes the version of the package in the
 image (0 without it) when it starts and before the image is saved.

 The page's stdin is a terminal that never blocks a read.  The script runs natively too,
 e.g. pharo --headless <image> st web-repl.st < input, but there is no Stop there.

 It files in with Pharo 12 and Pharo 15.  Every method has a methodsFor: section of its
 own: Pharo 15 reads only one method in a section, takes the chunk after it for the
 metadata of that method, and the next one for code to evaluate."

"Define WebRepl, unless an image saved from the page has it already: Pharo 15 logs a class
 definition to the .changes, an unchanged one too, which --no-source does not prevent"
| shared |
shared := { #Loop. #Watcher. #EvalProcess. #EvalDone. #InterruptSemaphore. #Generation. #Session. #AtPrompt. #Fallback }.
((Smalltalk hasClassNamed: #WebRepl) and: [ (Smalltalk at: #WebRepl) classVarNames asSet = shared asSet ])
	ifFalse: [
		(Object << #WebRepl
			sharedVariables: shared;
			package: 'WebRepl') install ]!

!WebRepl class methodsFor: 'running'!
start
	"Start the loop, or start it again.  An image saved by an evaluation runs this script again
	 when it boots, in the process of that evaluation, and the old loop waits for it: so never
	 terminate the active process.  The generation number silences an old loop and its evaluation"

	| generation semaphore |
	generation := Generation := (Generation ifNil: [ 0 ]) + 1.
	Session := Smalltalk session.
	{ Loop. Watcher } do: [ :process |
		(process isNil or: [ process == Processor activeProcess ]) ifFalse: [ process terminate ] ].
	EvalProcess := EvalDone := nil.
	AtPrompt := false.
	self handleErrors.
	"Before every save: see noteWorld.  A registration replaces the one of the same name"
	SessionManager default registerUserClassNamed: self name.
	self noteWorld.
	InterruptSemaphore ifNotNil: [ :old | Smalltalk unregisterExternalObject: old ].
	semaphore := InterruptSemaphore := Semaphore new.
	self primSetInterruptSemaphore: (Smalltalk registerExternalObject: semaphore).
	Watcher := [ self watch: semaphore ]
		forkAt: Processor userInterruptPriority
		named: 'WebRepl watcher'.
	Loop := [ self loop: generation ]
		forkAt: Processor userSchedulingPriority
		named: 'WebRepl'! !

!WebRepl class methodsFor: 'error handling'!
handleError: anError
	"An error, or a Halt, that nothing handled in another process than the evaluation, which
	 catches its own: report it, and terminate that process.  The REPL's own processes cannot
	 go on without it: the handler that WebRepl replaced quits the image then, as it always
	 would"

	| process |
	process := Processor activeProcess.
	(process == Loop or: [ process == Watcher ])
		ifTrue: [ ^ Fallback handleError: anError ].
	self report: anError.
	process terminate! !

!WebRepl class methodsFor: 'error handling'!
handleErrors
	"Be the error handler of the processes that have none of their own, and keep the one it
	 replaces in Fallback.  Every start of the session, also after a snapshot, installs another
	 one (ErrorHandlerSessionHandler): so the loop calls this before every prompt.  Pharo 12
	 keeps it in ErrorHandler defaultErrorHandler, and Pharo 15 in ErrorHandler default"

	| handler |
	(ErrorHandler respondsTo: #defaultErrorHandler:)
		ifTrue: [
			handler := ErrorHandler defaultErrorHandler.
			handler == self ifFalse: [ ErrorHandler defaultErrorHandler: self ] ]
		ifFalse: [
			handler := ErrorHandler default.
			handler == self ifFalse: [ ErrorHandler default: self ] ].
	handler == self ifFalse: [ Fallback := handler ]! !

!WebRepl class methodsFor: 'error handling'!
handleNotification: aNotification

	^ Fallback handleNotification: aNotification! !

!WebRepl class methodsFor: 'error handling'!
handleWarning: aWarning
	"A Warning that nothing handled in another process than the evaluation: report it, and let
	 that process go on, as the headless image would"

	self report: aWarning.
	^ aWarning resume! !

!WebRepl class methodsFor: 'error handling'!
onPrimitiveError: aString
	"Pharo 15 sends it to the error handler"

	^ Fallback onPrimitiveError: aString! !

!WebRepl class methodsFor: 'error handling'!
report: anException
	"Write what describe: says about anException on stderr, for a process of its own.  When the
	 loop waits at its prompt, which the report pushed up, prompt again"

	self write: (self describe: anException) , String lf on: Stdio stderr.
	AtPrompt == true ifTrue: [ self prompt ]! !

!WebRepl class methodsFor: 'system startup'!
shutDown: quitting
	"The image is about to be saved, or to quit"

	self noteWorld! !

!WebRepl class methodsFor: 'private'!
codeErrorClass
	"The class of the errors of the compiler: Pharo 15 signals an OCCodeError, which its
	 CodeError subclasses, and Pharo 12 has only CodeError"

	^ Smalltalk globals at: #OCCodeError ifAbsent: [ Smalltalk globals at: #CodeError ]! !

!WebRepl class methodsFor: 'private'!
decode: aByteArray
	"The page sends UTF-8; a native stdin may send anything"

	^ [ aByteArray utf8Decoded ] on: Error do: [ aByteArray asString ]! !

!WebRepl class methodsFor: 'private'!
describe: anException
	"Error: <class> <message text>, then the frames that raised it, up to the evaluated code and at
	 most 5.  A Halt comes as itself, any other exception in the UnhandledError of its default action.
	 The frames of a syntax error are the compiler's: say where it is in the source instead"

	| error context |
	error := (anException isKindOf: UnhandledError)
		ifTrue: [ anException exception ]
		ifFalse: [ anException ].
	^ String streamContents: [ :stream |
		stream nextPutAll: 'Error: '; nextPutAll: error class name.
		(error isKindOf: self codeErrorClass)
			ifTrue: [ self describeSource: error on: stream ]
			ifFalse: [
				error messageText asString ifNotEmpty: [ :text | stream space; nextPutAll: text ].
				context := error signalerContext ].
		5 timesRepeat: [
			context ifNotNil: [
				stream lf; nextPutAll: '  '; nextPutAll: (context isBlockContext
					ifTrue: [ '[] in ' , context home printString ]
					ifFalse: [ context printString ]).
				context := (context isBlockContext not and: [ context selector == #DoIt ])
					ifTrue: [ nil ]
					ifFalse: [ context sender ] ] ] ]! !

!WebRepl class methodsFor: 'private'!
describeSource: aCodeError on: aStream
	"What the compiler says, and where the syntax error is: ' Undeclared variable x (line 1,
	 column 1)'.  Pharo 15 says 'Undeclared variable named x in DoIt' of the line evaluated: say
	 it as Pharo 12 does.  The page sends the line breaks of multi-line input as CRs.  Say nothing
	 that cannot be found out"

	| undeclared |
	undeclared := [ aCodeError notice isUndeclaredNotice ifTrue: [ aCodeError notice node name ] ]
		on: Error
		do: [ :error | error return: nil ].
	undeclared
		ifNil: [ aCodeError messageText asString ifNotEmpty: [ :text | aStream space; nextPutAll: text ] ]
		ifNotNil: [ aStream nextPutAll: ' Undeclared variable '; nextPutAll: undeclared ].
	[	| source position line column |
		source := aCodeError sourceCode asString.
		position := aCodeError position.
		line := column := 1.
		1 to: (position - 1 min: source size) do: [ :index |
			| char |
			char := source at: index.
			(char = Character lf and: [ index > 1 and: [ (source at: index - 1) = Character cr ] ])
				ifFalse: [
					(char = Character cr or: [ char = Character lf ])
						ifTrue: [
							line := line + 1.
							column := 1 ]
						ifFalse: [ column := column + 1 ] ] ].
		aStream
			nextPutAll: ' (line '; print: line;
			nextPutAll: ', column '; print: column;
			nextPut: $) ] on: Error do: [ :error | error return ]! !

!WebRepl class methodsFor: 'private'!
evaluate: aString generation: generation
	"Evaluate aString in a process of its own, which Stop may terminate, and wait for it.  EvalDone
	 is the semaphore that its end signals, for the watcher"

	| done |
	aString isAllSeparators ifTrue: [ ^ self ].
	AtPrompt := false.
	done := EvalDone := Semaphore new.
	EvalProcess := [ [ self run: aString generation: generation ] ensure: [ done signal ] ]
		forkAt: Processor userSchedulingPriority
		named: 'WebRepl evaluation'.
	done wait.
	(self isCurrent: generation) ifTrue: [ EvalProcess := EvalDone := nil ]! !

!WebRepl class methodsFor: 'private'!
isCurrent: generation
	"Whether the loop of generation is the one started last, in this session.  An image saved by
	 an evaluation resumes in a new session, with the loop that waited for that evaluation"

	^ generation = Generation and: [ Session == Smalltalk session ]! !

!WebRepl class methodsFor: 'private'!
loop: generation
	"Prompt, then evaluate every line that comes on stdin, up to its LF, and prompt again.  At the
	 end of the input, evaluate what is left of it and quit"

	| pending chunk end |
	pending := ByteArray new.
	self prompt.
	[ (chunk := self readChunk) isEmpty ] whileFalse: [
		pending := pending , chunk.
		[ (end := pending indexOf: 10) > 0 ] whileTrue: [
			self evaluate: (self decode: (pending first: end - 1)) generation: generation.
			pending := pending allButFirst: end.
			(self isCurrent: generation) ifFalse: [ ^ self ].
			self prompt ] ].
	self evaluate: (self decode: pending) generation: generation.
	(self isCurrent: generation) ifFalse: [ ^ self ].
	self write: String lf on: Stdio stdout.
	Smalltalk snapshot: false andQuit: true! !

!WebRepl class methodsFor: 'private'!
noteWorld
	"Write the version of OSWindow-Web in the image to the file that the page's worker names in
	 PHARO_WEB_WORLD_FILE, if it names one: 0 without the package, 1 for a package without
	 OSWebDriver class>>#packageVersion, else that version.  The worker reads it once the REPL
	 prompted, or the image saved itself, and else looks for the version in the bytes of the image
	 (OSWebDriver class>>#packageMarker)"

	[	| file version |
		file := (OSEnvironment current at: 'PHARO_WEB_WORLD_FILE' ifAbsent: [ ^ self ])
			asFileReference.
		version := (Smalltalk hasClassNamed: #OSWebDriver)
			ifFalse: [ 0 ]
			ifTrue: [
				((Smalltalk at: #OSWebDriver) class canUnderstand: #packageVersion)
					ifTrue: [ (Smalltalk at: #OSWebDriver) packageVersion ]
					ifFalse: [ 1 ] ].
		file ensureDelete.
		file writeStreamDo: [ :stream | stream nextPutAll: version printString ] ]
		on: Error
		do: [ :error | error return ]! !

!WebRepl class methodsFor: 'private'!
prompt
	"Also take the errors of the other processes back: an evaluation may have saved the image"

	self handleErrors.
	self write: 'st> ' on: Stdio stdout.
	AtPrompt := true! !

!WebRepl class methodsFor: 'private'!
readChunk
	"The next bytes of stdin, at most 4096, or none at the end of the input.  A terminal, as the
	 page's stdin is, answers no bytes until it has some: wait for them.  Anything else, a file or
	 a pipe, blocks the read, and the VM could not wait for a file"

	(File fileDescriptorType: 0) = 1 ifTrue: [ Stdio stdin waitForData ].
	^ Stdio stdin next: 4096! !

!WebRepl class methodsFor: 'private'!
run: aString generation: generation
	"Print the value of aString on stdout, or the error it raised on stderr.  Catch any exception
	 that nothing handles, a Halt too: the default action of the headless image is to quit.  But
	 not a Warning, which the headless image only reports: its UnhandledError goes on to its default
	 action, handleWarning:, and the evaluation with it.  Once the evaluation is over, Stop has
	 nothing to terminate: no Interrupted. after its result"

	| text failed |
	failed := false.
	text := [ (Smalltalk compiler evaluate: aString) printString ]
		on: UnhandledException
		do: [ :exception |
			((exception isKindOf: UnhandledError) and: [ exception exception isKindOf: Warning ])
				ifTrue: [ exception pass ].
			failed := true.
			self describe: exception ].
	(self isCurrent: generation) ifFalse: [ ^ self ].
	EvalProcess == Processor activeProcess ifTrue: [ EvalProcess := nil ].
	self write: text , String lf on: (failed ifTrue: [ Stdio stderr ] ifFalse: [ Stdio stdout ])! !

!WebRepl class methodsFor: 'private'!
watch: aSemaphore
	"Stop signals aSemaphore: terminate the evaluation that runs.  When none runs, do nothing.  The
	 ensure: of the evaluation tells the loop that it ended, and terminate waits for it.  But an
	 evaluation that did not start yet (a process of its priority, which an earlier one forked, was
	 ahead of it) has no ensure: to unwind: tell the loop then.  A second signal of the semaphore
	 of an evaluation that ended does no harm, nobody waits on it again"

	[	aSemaphore wait; consumeAllSignals.
		EvalProcess ifNotNil: [ :process |
			| done |
			done := EvalDone.
			EvalProcess := EvalDone := nil.
			self write: 'Interrupted.' , String lf on: Stdio stderr.
			process terminate.
			done ifNotNil: [ done signal ] ] ] repeat! !

!WebRepl class methodsFor: 'private'!
write: aString on: aStream
	"Stdio streams are binary.  Flush at once: the page shows output as it comes"

	aStream nextPutAll: aString utf8Encoded; flush! !

!WebRepl class methodsFor: 'primitives'!
primSetInterruptSemaphore: semaphoreIndex
	"Natively there is no Stop"

	<primitive: #primitiveSetInterruptSemaphore module: #WebHostPlugin>
	^ nil! !

WebRepl start!
