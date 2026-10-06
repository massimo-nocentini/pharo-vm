"The kernel of the Notebook tab of the Console page.  The tab runs a VM of its own, in a worker of
 its own, which boots the image saved in this browser, read-only, or else the site's image, with

	--headless /pharo/Pharo.image --no-default-preferences st --no-source /pharo/st/web-notebook.st

 (PharoVMDriver.vmArgs('notebook') of packaging/emscripten/web/vm-driver.js), so this script is
 filed in at every boot, and its source is not logged to the .changes.  It defines NotebookKernel,
 Notebook (what a cell calls to show rich output), NotebookDisplay, NotebookCell and
 NotebookTranscript, and starts the kernel only when NOTEBOOK_EVENTS names the file of its events:
 the driver sets it to /dev/nbevents, a device whose writes go to the page.  Filed in from the
 Console, the script only defines the classes.  Natively, or in the node VM:

	NOTEBOOK_EVENTS=/dev/fd/3 pharo --headless <image> st web-notebook.st < requests 3> events

 The requests come on stdin, one JSON object per line, in UTF-8 (JSON escapes every LF of a cell):

	{""op"":""run"",""rid"":7,""name"":""In[7]"",""code"":""x := 3""}
	{""op"":""ping"",""rid"":8}                  answered by pong {rid}
	{""op"":""stop-background""}               answered by stopped {count}

 rid is the execution count, 1 to 2^30 - 1.  Anything else gets bad-request {text}.  The requests
 are served one at a time, in order; the page sends a run once the cell before is done.  At the
 end of the input the image quits: the page never closes stdin, only the harnesses do.

 The events go to NOTEBOOK_EVENTS, one JSON object per line, ev first.  An event with ""bytes"": N
 is followed by exactly N bytes, then an LF (an attachment: raw pixels, or a PNG).  Before every
 event the Transcript of the running cell, stdout and stderr are flushed, so what a cell writes
 there lies between its start and its done:

	hello {proto, version, major, minor, image, wordSize}        once, first
	start {rid, name}
	display {rid, mime, data, id}, or {rid, mime, data: '', size, id} above 4 MB of data,
	        or {rid, mime: image/x-pharo-bgra, width, height, premultiplied, id, bytes}; a Form
	        0 wide or high is {rid, mime: text/plain, data: 'empty image 0x0', id}
	clear {rid}
	stream {rid, name, text}    what a process that an earlier cell forked shows on the Transcript
	error {rid, error}          an error that nothing handled in a process that cell rid forked
	done {rid, ms, status, values, truncated, error}

 status is ok (values is the printString of the value, in an Array, or null), error, syntax (the
 cell does not parse: nothing ran), interrupted (Stop) or terminated (the cell's process
 terminated itself).  An error is {text, kind, location, form, line, column, chain}.

 A cell is parsed whole, then its statements are compiled and run one at a time, so that a class
 can be defined and used in one cell.  A variable that a cell assigns, and nobody declared, is a
 variable of the notebook (a WorkspaceVariable); the temporaries of a cell are its own.  A ^ ends
 the cell with its value.

 The page's Stop calls vm_interrupt(), which signals the semaphore that the kernel registered with
 the WebHostPlugin: the cell that runs is terminated, or the one that is about to start does not
 run.  A Stop that comes while no cell runs, and before the next request, does nothing.  The page's
 watchdog restarts the worker when that does not end the cell.

 The kernel is the error handler of the other processes, as WebRepl is in the Console: an error in
 a process that a cell forked is reported to that cell, and the process is terminated.

 It files in with Pharo 12 and Pharo 15.  Every method has a methodsFor: section of its own (see
 web-repl.st), and the file is ASCII: Pharo 15 mangles the other characters of a file-in.  The
 kernel is not a sandbox: a cell can call it, and write events; the page checks every event"

"Define the classes, unless the image has them as they are: Pharo 15 logs a class definition to the
 .changes, an unchanged one too, which --no-source does not prevent"
| shared |
shared := { #ActiveRid. #Bindings. #EvalDone. #EvalProcess. #Events. #Fallback. #Fell. #Flusher. #Generation. #Idler. #InterruptSemaphore. #LastError. #Loop. #OurTranscript. #Pending. #Serving. #Session. #StopNext. #Taken. #Wake. #WakeIndex. #Watcher. #WebVM. #ZeroWakes }.
((Smalltalk hasClassNamed: #NotebookKernel) and: [ (Smalltalk at: #NotebookKernel) classVarNames asSet = shared asSet ])
	ifFalse: [
		(Object << #NotebookKernel
			sharedVariables: shared;
			package: 'WebNotebook') install ].
((Smalltalk hasClassNamed: #NotebookCell) and: [ (Smalltalk at: #NotebookCell) superclass == ProcessLocalVariable ])
	ifFalse: [ (ProcessLocalVariable << #NotebookCell package: 'WebNotebook') install ].
((Smalltalk hasClassNamed: #NotebookTranscript) and: [
	(Smalltalk at: #NotebookTranscript) superclass == NonInteractiveTranscript and: [
		(Smalltalk at: #NotebookTranscript) instVarNames asArray = #( #local #others ) ] ])
	ifFalse: [
		(NonInteractiveTranscript << #NotebookTranscript
			slots: { #local. #others };
			package: 'WebNotebook') install ].
((Smalltalk hasClassNamed: #NotebookDisplay) and: [
	(Smalltalk at: #NotebookDisplay) instVarNames asArray = #( #mime #data #form #premultiplied ) ])
	ifFalse: [
		(Object << #NotebookDisplay
			slots: { #mime. #data. #form. #premultiplied };
			package: 'WebNotebook') install ].
(Smalltalk hasClassNamed: #Notebook)
	ifFalse: [ (Object << #Notebook package: 'WebNotebook') install ]!

!NotebookCell class methodsFor: 'testing'!
isInheritable
	"The rid of the cell whose process forked the active one: a process that a cell forks reports to
	 that cell.  The kernel's processes have none"

	^ true! !

!NotebookTranscript methodsFor: 'initialization'!
initialize

	super initialize.
	local := WriteStream on: (String new: 1024).
	others := Dictionary new! !

!NotebookTranscript methodsFor: 'streaming'!
nextPutAll: aCollection
	"What the running cell and the kernel show goes to stdout, what the processes of other cells show
	 goes to them, as stream events.  It is sent within 50 ms (see NotebookKernel>>#tick), before
	 every event, at 64 KB, or on flush"

	| text |
	text := aCollection asString.
	self critical: [ (self bufferFor: NotebookCell value) nextPutAll: text ].
	self flushIfFull! !

!NotebookTranscript methodsFor: 'streaming'!
nextPut: aCharacter

	self critical: [ (self bufferFor: NotebookCell value) nextPut: aCharacter ].
	self flushIfFull! !

!NotebookTranscript methodsFor: 'streaming'!
newLine
	"The page wants LFs"

	self nextPut: Character lf! !

!NotebookTranscript methodsFor: 'streaming'!
endEntry
	"show: ends with it: the kernel sends the text within 50 ms, a line at a time would cost an event
	 or a write each"! !

!NotebookTranscript methodsFor: 'streaming'!
flush
	"Send what was shown now: 'Transcript show: ''bye''; flush. Smalltalk exit: 0' says bye"

	self flushLocal.
	self flushOthers! !

!NotebookTranscript methodsFor: 'streaming'!
stream
	"The buffer of the active process"

	^ self critical: [ self bufferFor: NotebookCell value ]! !

!NotebookTranscript methodsFor: 'streaming'!
clear
	"Nothing was shown that could be taken back"! !

!NotebookTranscript methodsFor: 'streaming'!
close
	"Stay open: the kernel is the Transcript of the cells"! !

!NotebookTranscript methodsFor: 'private'!
bufferFor: rid

	(rid isNil or: [ rid = NotebookKernel activeRid ]) ifTrue: [ ^ local ].
	^ others at: rid ifAbsentPut: [ WriteStream on: (String new: 256) ]! !

!NotebookTranscript methodsFor: 'private'!
flushIfFull

	| size |
	size := self critical: [
		| total |
		total := local position.
		others do: [ :each | total := total + each position ].
		total ].
	size > 65536 ifTrue: [ self flush ]! !

!NotebookTranscript methodsFor: 'private'!
flushLocal
	"Write what the running cell and the kernel showed on stdout"

	self critical: [
		local position = 0 ifFalse: [
			| text |
			text := local contents.
			local := WriteStream on: (String new: 1024).
			NotebookKernel write: text on: Stdio stdout ] ]! !

!NotebookTranscript methodsFor: 'private'!
flushOthers
	"Send what the processes of the other cells showed, each to its cell.  The events are written
	 holding the buffers, so that the texts of a cell keep their order"

	self critical: [
		| buffers |
		buffers := others.
		others := Dictionary new.
		buffers keysAndValuesDo: [ :rid :buffer |
			buffer position = 0 ifFalse: [ NotebookKernel emitStream: buffer contents rid: rid ] ] ]! !

!NotebookTranscript methodsFor: 'private'!
flushRid: rid
	"Send what the processes of cell rid showed, before another event of that cell"

	self critical: [
		(others removeKey: rid ifAbsent: [ nil ]) ifNotNil: [ :buffer |
			buffer position = 0 ifFalse: [ NotebookKernel emitStream: buffer contents rid: rid ] ] ]! !

!NotebookDisplay class methodsFor: 'instance creation'!
mime: aString data: data
	"data is a String, or the ByteArray of an attachment"

	^ self new setMime: aString data: data form: nil premultiplied: false! !

!NotebookDisplay class methodsFor: 'instance creation'!
form: aForm premultiplied: aBoolean
	"aForm is shown as raw pixels, which the page makes a PNG of"

	^ self new setMime: 'image/x-pharo-bgra' data: nil form: aForm premultiplied: aBoolean! !

!NotebookDisplay methodsFor: 'initialization'!
setMime: aString data: anObject form: aForm premultiplied: aBoolean

	mime := aString.
	data := anObject.
	form := aForm.
	premultiplied := aBoolean! !

!NotebookDisplay methodsFor: 'accessing'!
mime

	^ mime! !

!NotebookDisplay methodsFor: 'accessing'!
data

	^ data! !

!NotebookDisplay methodsFor: 'accessing'!
form

	^ form! !

!NotebookDisplay methodsFor: 'accessing'!
premultiplied
	"Whether the pixels are premultiplied by their alpha, as cairo's and BitBlt's blends leave them"

	^ premultiplied == true! !

!NotebookDisplay methodsFor: 'testing'!
isVisual

	^ form notNil! !

!NotebookDisplay methodsFor: 'accessing'!
pixels
	"The Form of 32 bits a pixel, whose words are the pixels: their bytes are B G R A in memory.  An
	 icon's bits may be hibernated, a compressed ByteArray"

	| pixels |
	pixels := form.
	pixels unhibernate.
	pixels depth = 32 ifFalse: [ pixels := pixels asFormOfDepth: 32 ].
	pixels unhibernate.
	^ pixels! !

!NotebookDisplay methodsFor: 'printing'!
printOn: aStream
	"a NotebookDisplay(text/html, 9 chars)"

	aStream nextPutAll: 'a NotebookDisplay('; nextPutAll: mime; nextPutAll: ', '.
	form
		ifNotNil: [ aStream print: form width; nextPut: $x; print: form height ]
		ifNil: [
			aStream print: data size.
			aStream nextPutAll: (data isString ifTrue: [ ' chars' ] ifFalse: [ ' bytes' ]) ].
	aStream nextPut: $)! !

!Notebook class methodsFor: 'displaying'!
text: anObject
	"Plain text: a String as it is, anything else as its printString"

	^ NotebookDisplay
		mime: 'text/plain'
		data: (anObject isString
			ifTrue: [ anObject asString ]
			ifFalse: [ anObject printStringLimitedTo: NotebookKernel printLimit ])! !

!Notebook class methodsFor: 'displaying'!
html: aString
	"HTML, which the page sanitizes"

	^ NotebookDisplay mime: 'text/html' data: (self string: aString for: #html:)! !

!Notebook class methodsFor: 'displaying'!
svg: aString
	"SVG, which the page sanitizes.  The root svg element gets the namespace if it lacks it"

	| string start end |
	string := self string: aString for: #svg:.
	start := string findString: '<svg'.
	start > 0 ifTrue: [
		end := string indexOf: $> startingAt: start ifAbsent: [ string size ].
		((string copyFrom: start to: end) includesSubstring: 'xmlns=') ifFalse: [
			string := (string copyFrom: 1 to: start + 3) , ' xmlns="http://www.w3.org/2000/svg"'
				, (string copyFrom: start + 4 to: string size) ] ].
	^ NotebookDisplay mime: 'image/svg+xml' data: string! !

!Notebook class methodsFor: 'displaying'!
markdown: aString
	"Markdown, which the page renders"

	^ NotebookDisplay mime: 'text/markdown' data: (self string: aString for: #markdown:)! !

!Notebook class methodsFor: 'displaying'!
image: anObject
	"What anObject looks like, as the raw pixels of a Form: a Form, a Morph, a Roassal canvas or
	 shape, or an AthensCairoSurface.  Encoding a PNG here would take seconds (the VM has no
	 ZipPlugin): the page makes it.  The pixels of cairo and of a Morph's imageForm are premultiplied
	 by their alpha, those of a Form filled by BitBlt are not"

	(anObject isKindOf: NotebookDisplay) ifTrue: [
		anObject isVisual ifTrue: [ ^ anObject ].
		^ self error: 'Notebook image: takes something to see, not a ' , anObject mime , ' display' ].
	anObject isForm ifTrue: [ ^ NotebookDisplay form: anObject premultiplied: false ].
	(self is: anObject a: #AthensCairoSurface) ifTrue: [
		^ NotebookDisplay form: anObject asForm premultiplied: true ].
	(self is: anObject a: #RSCanvas) ifTrue: [
		^ NotebookDisplay form: (self formOfCanvas: anObject) premultiplied: true ].
	(self is: anObject a: #RSShape) ifTrue: [
		^ NotebookDisplay
			form: (self formOfCanvas: ((Smalltalk at: #RSCanvas) new add: anObject; yourself))
			premultiplied: true ].
	anObject isMorph ifTrue: [ ^ NotebookDisplay form: anObject imageForm premultiplied: true ].
	^ self error: 'Notebook image: takes a Form, a Morph, a Roassal canvas or shape, or an AthensCairoSurface'! !

!Notebook class methodsFor: 'displaying'!
png: aByteArray
	"The bytes of a PNG, sent as they are"

	(aByteArray isKindOf: ByteArray) ifFalse: [ ^ self error: 'Notebook png: takes a ByteArray' ].
	^ NotebookDisplay mime: 'image/png' data: aByteArray! !

!Notebook class methodsFor: 'displaying'!
table: rows
	"An HTML table of rows, a collection of collections; a row that is not one is a row of one cell"

	^ self table: rows header: nil! !

!Notebook class methodsFor: 'displaying'!
table: rows header: header
	"An HTML table with a head.  Strings, numbers and Characters are escaped, an HTML display is
	 spliced in as it is, and anything else is its printString, escaped"

	^ NotebookDisplay mime: 'text/html' data: (String streamContents: [ :stream |
		stream nextPutAll: '<table>'.
		header ifNotNil: [
			stream nextPutAll: '<thead>'.
			self row: header tag: 'th' on: stream.
			stream nextPutAll: '</thead>' ].
		stream nextPutAll: '<tbody>'.
		rows do: [ :row | self row: row tag: 'td' on: stream ].
		stream nextPutAll: '</tbody></table>' ])! !

!Notebook class methodsFor: 'displaying'!
show: anObject
	"Show anObject in the output of the cell now: a display, a String as text, something to see as an
	 image, anything else as its printString"

	^ self show: anObject id: nil! !

!Notebook class methodsFor: 'displaying'!
show: anObject id: aString
	"Show anObject now.  A display shown again with the same id in the same cell replaces the one
	 shown before, as a progress bar does.  The display goes to the cell of the calling process"

	NotebookKernel
		emitDisplay: (self displayOf: anObject)
		rid: NotebookCell value
		id: (aString ifNotNil: [ aString asString ]).
	^ nil! !

!Notebook class methodsFor: 'displaying'!
clearOutput
	"Clear the output of the cell so far"

	NotebookKernel emit: { #ev -> 'clear'. #rid -> NotebookCell value }.
	^ nil! !

!Notebook class methodsFor: 'testing'!
isDisplay: anObject

	^ anObject isKindOf: NotebookDisplay! !

!Notebook class methodsFor: 'testing'!
isVisual: anObject
	"Whether the value of a cell is shown as an image"

	^ anObject isForm
		or: [ anObject isMorph
		or: [ (self is: anObject a: #RSCanvas)
		or: [ (self is: anObject a: #RSShape)
		or: [ self is: anObject a: #AthensCairoSurface ] ] ] ]! !

!Notebook class methodsFor: 'accessing'!
lastError
	"The last error that a cell raised"

	^ NotebookKernel lastError! !

!Notebook class methodsFor: 'accessing'!
variables
	"The names of the variables of the notebook"

	^ NotebookKernel bindings keys asSortedCollection asArray! !

!Notebook class methodsFor: 'accessing'!
forgetVariables

	NotebookKernel forgetVariables.
	^ nil! !

!Notebook class methodsFor: 'private'!
displayOf: anObject

	(anObject isKindOf: NotebookDisplay) ifTrue: [ ^ anObject ].
	anObject isString ifTrue: [ ^ self text: anObject ].
	(self isVisual: anObject) ifTrue: [ ^ self image: anObject ].
	^ self text: anObject! !

!Notebook class methodsFor: 'private'!
string: anObject for: selector

	anObject isString ifFalse: [ ^ self error: 'Notebook ' , selector , ' takes a String' ].
	^ anObject asString! !

!Notebook class methodsFor: 'private'!
is: anObject a: className
	"Roassal and Athens may be missing"

	^ (Smalltalk globals at: className ifAbsent: [ nil ])
		ifNil: [ false ]
		ifNotNil: [ :class | anObject isKindOf: class ]! !

!Notebook class methodsFor: 'private'!
formOfCanvas: aCanvas
	"What its morph draws, at the extent of its shapes, zoomed to fit: zoomToFit after createMorph
	 would clip them"

	| extent morph |
	extent := aCanvas nodes isEmpty
		ifTrue: [ 300 @ 200 ]
		ifFalse: [ ((aCanvas encompassingRectangle extent + 40) max: 100 @ 60) min: 1200 @ 900 ].
	extent := extent rounded.
	aCanvas extent: extent.
	aCanvas zoomToFit.
	morph := aCanvas createMorph.
	morph extent: extent.
	^ morph imageForm! !

!Notebook class methodsFor: 'private'!
row: row tag: tag on: aStream

	| cells |
	cells := (row isCollection and: [ row isString not ])
		ifTrue: [ row asArray ]
		ifFalse: [ { row } ].
	aStream nextPutAll: '<tr>'.
	cells do: [ :each |
		aStream nextPut: $<; nextPutAll: tag; nextPut: $>.
		self cell: each on: aStream.
		aStream nextPutAll: '</'; nextPutAll: tag; nextPut: $> ].
	aStream nextPutAll: '</tr>'! !

!Notebook class methodsFor: 'private'!
cell: anObject on: aStream

	(anObject isKindOf: NotebookDisplay) ifTrue: [
		anObject mime = 'text/html' ifTrue: [ ^ aStream nextPutAll: anObject data ].
		anObject mime = 'text/plain' ifTrue: [ ^ self escape: anObject data on: aStream ] ].
	anObject isString ifTrue: [ ^ self escape: anObject asString on: aStream ].
	anObject isCharacter ifTrue: [ ^ self escape: (String with: anObject) on: aStream ].
	self escape: (anObject printStringLimitedTo: NotebookKernel printLimit) on: aStream! !

!Notebook class methodsFor: 'private'!
escape: aString on: aStream

	aString do: [ :char |
		char = $& ifTrue: [ aStream nextPutAll: '&amp;' ] ifFalse: [
		char = $< ifTrue: [ aStream nextPutAll: '&lt;' ] ifFalse: [
		char = $> ifTrue: [ aStream nextPutAll: '&gt;' ] ifFalse: [
		char = $" ifTrue: [ aStream nextPutAll: '&quot;' ] ifFalse: [
			aStream nextPut: char ] ] ] ] ]! !

!NotebookKernel class methodsFor: 'starting'!
startIfRequested
	"Start the kernel when NOTEBOOK_EVENTS names the file of the events, as the driver does.  Filed in
	 from the Console, where it is not set, only say that the classes are there.  When the file cannot
	 be opened, nobody would hear of the kernel: say why on stderr, and quit"

	| path |
	path := OSEnvironment current at: 'NOTEBOOK_EVENTS' ifAbsent: [ nil ].
	(path isNil or: [ path isEmpty ]) ifTrue: [
		^ Transcript show: 'NotebookKernel is defined: it starts when NOTEBOOK_EVENTS names the file of its events'; cr ].
	[ self startOn: path ]
		on: Error
		do: [ :error |
			self writeError: 'NotebookKernel: cannot start on ' , path , ': ' , (self messageOf: error).
			Smalltalk exit: 1 ]! !

!NotebookKernel class methodsFor: 'starting'!
start
	"Start the kernel on the file that NOTEBOOK_EVENTS names"

	^ self startOn: (OSEnvironment current at: 'NOTEBOOK_EVENTS')! !

!NotebookKernel class methodsFor: 'starting'!
startOn: path
	"Open the events, take over the error handler and the Transcript, and start the loop, the watcher
	 of Stop, the flusher of the Transcript and, in the web VM, the idler (see #idle:).  An image saved
	 by a cell has the processes of the kernel of the session that saved it: terminate them, but never
	 the active process, which may be the cell that saved the image"

	| semaphore |
	self isRunning ifTrue: [ ^ self error: 'the notebook kernel is already running' ].
	Generation := (Generation ifNil: [ 0 ]) + 1.
	Session := Smalltalk session.
	self terminateOldProcesses.
	Events := self openEvents: path.
	EvalProcess := EvalDone := Serving := ActiveRid := LastError := nil.
	StopNext := Fell := false.
	Pending := ByteArray new.
	Taken := ZeroWakes := 0.
	Bindings := Dictionary new.
	OurTranscript := NotebookTranscript new.
	self assertEnvironment.
	Wake ifNotNil: [ :old | Smalltalk unregisterExternalObject: old ].
	Wake := Semaphore new.
	WakeIndex := Smalltalk registerExternalObject: Wake.
	InterruptSemaphore ifNotNil: [ :old | Smalltalk unregisterExternalObject: old ].
	semaphore := InterruptSemaphore := Semaphore new.
	WebVM := (self primSetInterruptSemaphore: (Smalltalk registerExternalObject: semaphore)) notNil.
	Watcher := self fork: [ self watch: semaphore ] at: Processor userInterruptPriority named: 'NotebookKernel watcher'.
	Loop := self fork: [ self loop: Generation ] at: Processor userSchedulingPriority named: 'NotebookKernel loop'.
	Flusher := self fork: [ self flushEvery: 50 ] at: Processor userSchedulingPriority + 1 named: 'NotebookKernel flusher'.
	Idler := WebVM ifTrue: [ self fork: [ self idle: Generation ] at: Processor lowestPriority + 1 named: 'NotebookKernel idler' ].
	WebVM ifTrue: [ self patchCairo ].
	self emit: {
		#ev -> 'hello'.
		#proto -> 1.
		#version -> SystemVersion current dottedMajorMinor.
		#major -> SystemVersion current major.
		#minor -> SystemVersion current minor.
		#image -> SystemVersion current imageVersionString.
		#wordSize -> Smalltalk wordSize }! !

!NotebookKernel class methodsFor: 'starting'!
isRunning
	"Whether the loop of this session runs"

	^ Loop notNil and: [ Session == Smalltalk session and: [ Loop isTerminated not ] ]! !

!NotebookKernel class methodsFor: 'starting'!
terminateOldProcesses
	"The processes of the kernel, and of the Console's REPL, that an image saved after a file-in of this
	 script, or by a cell, holds from an older session; never the active process.  The REPL's loop ends
	 by itself (WebRepl>>#isCurrent:)"

	| names |
	names := #('NotebookKernel loop' 'NotebookKernel watcher' 'NotebookKernel flusher' 'NotebookKernel idler' 'WebRepl watcher' 'WebRepl evaluation').
	(Process allSubInstances select: [ :process |
		process ~~ Processor activeProcess
			and: [ process isTerminated not
			and: [ names includes: process name ] ] ])
		do: [ :process | [ process terminate ] on: Error do: [ :error | error return ] ]! !

!NotebookKernel class methodsFor: 'starting'!
openEvents: path
	"The binary stream of the events.  /dev/fd/3 may be a socket, as node's pipes are, which open()
	 refuses: take the descriptor itself then"

	| descriptor |
	descriptor := (path beginsWith: '/dev/fd/')
		ifTrue: [ (path allButFirst: 8) asInteger ]
		ifFalse: [ nil ].
	descriptor ifNotNil: [
		[ ^ StdioStream
				handle: (File connectToFileDescriptor: descriptor writable: true)
				file: (File named: path)
				forWrite: true ]
			on: Error
			do: [ :error | error return ] ].
	^ (File named: path) writeStream! !

!NotebookKernel class methodsFor: 'starting'!
fork: aBlock at: priority named: aString
	"A process of the kernel, which reports to no cell"

	^ [	NotebookCell value: nil.
		aBlock value ] forkAt: priority named: aString! !

!NotebookKernel class methodsFor: 'starting'!
patchCairo
	"The web VMs have no SurfacePlugin, without which a stock AthensCairoSurface cannot register
	 itself: compile the methods of packaging/emscripten/st/OSWindow-Web/AthensCairoSurface.extension.st,
	 which a prepared image has, into this instance, which is never saved, without logging them (the
	 .changes would grow at every boot).  Natively the stock methods work"

	| class protocol |
	class := Smalltalk globals at: #AthensCairoSurface ifAbsent: [ ^ self ].
	protocol := (class class compiledMethodAt: #registerSurface: ifAbsent: [ nil ])
		ifNil: [ '' ]
		ifNotNil: [ :method | method protocolName asString ].
	(#('*OSWindow-Web' '*WebNotebook') includes: protocol) ifTrue: [ ^ self ].
	[	self cairoSources do: [ :each |
			(each first ifTrue: [ class class ] ifFalse: [ class ])
				compile: each last
				classified: '*WebNotebook'
				withStamp: nil
				notifying: nil
				logSource: false ] ]
		on: Error
		do: [ :error | self writeError: 'NotebookKernel: AthensCairoSurface: ' , (self messageOf: error) ]! !

!NotebookKernel class methodsFor: 'starting'!
cairoSources
	"{class side. source} of the methods of AthensCairoSurface.extension.st"

	^ {
	{ true. 'registerSurface: anAthensCairoSurface
	"The web VMs have no SurfacePlugin, through which the stock method shares the pixels of a surface with BitBlt, and fails without: register nothing, and answer nil for the id slot of the surface, where #asForm keeps a copy of the pixels instead"

	self checkSession.
	^ nil' }.
	{ true. 'unregisterSurfaceWithId: anAthensCairoSurfaceId
	"Nothing was registered (see #registerSurface:)"' }.
	{ false. 'asForm
	"A Form of my extent holding a copy of my pixels as they are now, the premultiplied ARGB32 of cairo. The Form is kept in my id slot, which the SurfacePlugin left free, and reused while my extent is the same"

	| form |
	self checkSession.
	self flush.
	form := id.
	(form isForm and: [ form extent = self extent ]) ifFalse: [
		form := Form extent: self extent depth: 32.
		id := form ].
	LibC
		memCopy: self getDataPtr
		to: form bits
		size: (self stride * self height min: form bits byteSize).
	^ form' } }! !

!NotebookKernel class methodsFor: 'environment'!
assertEnvironment
	"Take back what a snapshot, or a cell, changed: the error handler, which every start of the session
	 installs, also after a snapshot; the Transcript, which Pharo 12 replaces then; the default stdio
	 streams (Stdio useNullStreams).  Done before every request, after every cell and at every tick of
	 the flusher: it compares identities only"

	[ self handleErrors ] on: Error do: [ :error | error return ].
	[	(Smalltalk globals at: #Transcript ifAbsent: [ nil ]) == OurTranscript
			ifFalse: [ Smalltalk globals at: #Transcript put: OurTranscript ] ] on: Error do: [ :error | error return ].
	[	(Stdio stdout class == StdioStream and: [ Stdio stderr class == StdioStream ])
			ifFalse: [ Stdio useDefaultStreams ] ] on: Error do: [ :error | error return ]! !

!NotebookKernel class methodsFor: 'environment'!
handleErrors
	"Be the error handler of the processes that have none of their own, and keep the first one replaced
	 in Fallback.  Pharo 12 keeps it in ErrorHandler defaultErrorHandler, and Pharo 15 in ErrorHandler
	 default"

	| handler |
	(ErrorHandler respondsTo: #defaultErrorHandler:)
		ifTrue: [
			handler := ErrorHandler defaultErrorHandler.
			handler == self ifFalse: [ ErrorHandler defaultErrorHandler: self ] ]
		ifFalse: [
			handler := ErrorHandler default.
			handler == self ifFalse: [ ErrorHandler default: self ] ].
	(handler == self or: [ Fallback notNil ]) ifFalse: [ Fallback := handler ]! !

!NotebookKernel class methodsFor: 'loop'!
loop: generation
	"Serve the requests that come on stdin, one at a time, in order.  At the end of the input quit"

	| line |
	Serving := ActiveRid := nil.
	StopNext := false.
	[	line := self nextLine.
		(self isCurrent: generation) ifFalse: [ ^ self ].
		line ifNil: [ ^ self endOfInput ].
		self assertEnvironment.
		self serve: line.
		(self isCurrent: generation) ifFalse: [ ^ self ].
		Serving := nil.
		StopNext := false.
		self assertEnvironment ] repeat! !

!NotebookKernel class methodsFor: 'loop'!
isCurrent: generation
	"Whether the loop of generation is the one started last, in this session.  An image saved by a cell
	 resumes, when it boots, with the loop that waited for that cell"

	^ generation = Generation and: [ Session == Smalltalk session ]! !

!NotebookKernel class methodsFor: 'loop'!
nextLine
	"The next line of stdin, without its LF, or nil at the end of the input"

	| line |
	[ (line := self takeLine) isNil ] whileTrue: [ self fill ifFalse: [ ^ nil ] ].
	^ line! !

!NotebookKernel class methodsFor: 'loop'!
takeLine
	"Take the first complete line out of Pending, or answer nil.  Serving is set in the same
	 unpreemptible block: from then on a Stop is for this request (see #stop)"

	^ [	| end line |
		end := self indexOfLf: Pending.
		end = 0
			ifTrue: [ nil ]
			ifFalse: [
				line := Pending copyFrom: 1 to: end - 1.
				Pending := Pending copyFrom: end + 1 to: Pending size.
				Serving := -1.
				line ] ] valueUnpreemptively! !

!NotebookKernel class methodsFor: 'loop'!
fill
	"Wait for more of stdin, into Pending, and answer false at its end.  The page's stdin is a
	 terminal, which answers no bytes until it has some: wait for them, as WebRepl does, and take what
	 it has.  The watcher may take them first (see #stop), and a wake may find nothing: only two wakes
	 in a row that find nothing, while nothing was taken, are the end.  Natively stdin may be a pipe, a
	 file or a socket (type 3 too), which the FilePlugin may read with fread, which waits until it has
	 all it was asked for: read a byte at a time, up to the end of the line.  epoll refuses a file: wait
	 for a terminal and a pipe only"

	| before type |
	before := Taken.
	type := File fileDescriptorType: 0.
	type = 3 ifFalse: [ self waitForInput ].
	type = 1
		ifTrue: [ [ self drain ] valueUnpreemptively ]
		ifFalse: [ self readLine ].
	ZeroWakes := Taken = before
		ifTrue: [ ZeroWakes + 1 ]
		ifFalse: [ 0 ].
	^ ZeroWakes < 2! !

!NotebookKernel class methodsFor: 'loop'!
waitForInput
	"Stdio stdin waitForData, on a semaphore of the kernel's: the watcher signals it as well when it
	 takes a request (see #stop), since the VM may then find no input left to signal it"

	File primitiveWaitForDataOn: (File stdioHandles at: 1) signalling: WakeIndex.
	Wake wait! !

!NotebookKernel class methodsFor: 'loop'!
readLine
	"Add to Pending the bytes of stdin up to an LF, or up to the end of the input"

	| line byte |
	line := WriteStream on: (ByteArray new: 256).
	[	byte := Stdio stdin next: 1.
		byte isEmpty or: [
			line nextPut: byte first.
			byte first = 10 ] ] whileFalse.
	line position = 0 ifFalse: [
		[	Pending := Pending , line contents.
			Taken := Taken + line position ] valueUnpreemptively ]! !

!NotebookKernel class methodsFor: 'loop'!
drain
	"Add to Pending what stdin has, without waiting: a terminal answers what it has, or nothing"

	| chunk |
	chunk := Stdio stdin next: 65536.
	chunk isEmpty ifFalse: [
		Pending := Pending , chunk.
		Taken := Taken + chunk size ]! !

!NotebookKernel class methodsFor: 'loop'!
indexOfLf: aByteArray
	"With a primitive of the MiscPrimitivePlugin: a request may have megabytes"

	| map |
	map := ByteArray new: 256.
	map at: 11 put: 1.
	^ [ ByteString findFirstInString: aByteArray inSet: map startingAt: 1 ]
		on: Error
		do: [ :error | error return: (aByteArray indexOf: 10) ]! !

!NotebookKernel class methodsFor: 'loop'!
endOfInput
	"Only the harnesses close stdin"

	[ OurTranscript flush ] on: Error do: [ :error | error return ].
	Smalltalk exit: 0! !

!NotebookKernel class methodsFor: 'loop'!
serve: aByteArray

	| request op |
	request := self parse: aByteArray.
	request ifNil: [ ^ self badRequest: aByteArray ].
	op := request at: 'op'.
	op = 'run' ifTrue: [
		^ self
			runCell: (request at: 'code')
			rid: (request at: 'rid')
			name: (request at: 'name' ifAbsent: [ 'In[' , (self decimal: (request at: 'rid')) , ']' ]) ].
	op = 'ping' ifTrue: [ ^ self emit: { #ev -> 'pong'. #rid -> (request at: 'rid') } ].
	self stopBackground! !

!NotebookKernel class methodsFor: 'loop'!
parse: aByteArray
	"The request on a line, a Dictionary, or nil when the line is not one.  STONJSON reads STON, which
	 is more than JSON (Point[1,2], #run, 'quotes', 1e400, and what follows the object): check the
	 line, and the class of every field"

	| text request op rid name |
	text := (self decode: aByteArray) trimBoth.
	(text notEmpty and: [ text first = ${ and: [ text last = $} ] ]) ifFalse: [ ^ nil ].
	(self isJson: aByteArray) ifFalse: [ ^ nil ].
	request := [	| reader object |
		reader := STONReader on: text readStream.
		object := reader next.
		reader atEnd ifTrue: [ object ] ifFalse: [ nil ] ]
			on: Error
			do: [ :error | error return: nil ].
	request isDictionary ifFalse: [ ^ nil ].
	op := request at: 'op' ifAbsent: [ nil ].
	(self isText: op) ifFalse: [ ^ nil ].
	op = 'stop-background' ifTrue: [ ^ request ].
	rid := request at: 'rid' ifAbsent: [ nil ].
	(rid class == SmallInteger and: [ rid between: 1 and: 16r3FFFFFFF ]) ifFalse: [ ^ nil ].
	op = 'ping' ifTrue: [ ^ request ].
	op = 'run' ifFalse: [ ^ nil ].
	(self isText: (request at: 'code' ifAbsent: [ nil ])) ifFalse: [ ^ nil ].
	name := request at: 'name' ifAbsent: [ ^ request ].
	^ (self isCellName: name) ifTrue: [ request ] ifFalse: [ nil ]! !

!NotebookKernel class methodsFor: 'loop'!
isJson: aByteArray
	"Whether there is only what JSON has between the strings of aByteArray: no 'quotes', #symbols, nil
	 or Point[1,2] of STON.  The strings are skipped with a primitive: a cell may have megabytes"

	| map index size |
	map := ByteArray new: 256.
	map at: 35 put: 1; at: 93 put: 1.
	index := 1.
	size := aByteArray size.
	[ index <= size ] whileTrue: [
		| byte |
		byte := aByteArray at: index.
		byte = 34
			ifTrue: [
				"up to the closing quote, past the escaped characters"
				[	index := ByteString findFirstInString: aByteArray inSet: map startingAt: index + 1.
					index = 0 ifTrue: [ ^ false ].
					(aByteArray at: index) = 92 ] whileTrue: [ index := index + 1 ] ]
			ifFalse: [
				(self isWordByte: byte)
					ifTrue: [
						| end |
						"true, false, null, or the e of an exponent"
						end := index.
						[ end < size and: [ self isWordByte: (aByteArray at: end + 1) ] ] whileTrue: [ end := end + 1 ].
						(#('true' 'false' 'null' 'e' 'E') includes: (aByteArray copyFrom: index to: end) asString)
							ifFalse: [ ^ false ].
						index := end ]
					ifFalse: [
						((byte between: 48 and: 57) or: [ #[123 125 91 93 44 58 45 43 46 32 9 10 13] includes: byte ])
							ifFalse: [ ^ false ] ] ].
		index := index + 1 ].
	^ true! !

!NotebookKernel class methodsFor: 'loop'!
isWordByte: aByte

	^ (aByte between: 97 and: 122) or: [ aByte between: 65 and: 90 ]! !

!NotebookKernel class methodsFor: 'loop'!
isText: anObject
	"A JSON string: STON reads #run as a Symbol"

	^ anObject isString and: [ anObject isSymbol not ]! !

!NotebookKernel class methodsFor: 'loop'!
isCellName: anObject
	"[A-Za-z0-9_.:\[\]-]{1,32}, as In[7]"

	^ (self isText: anObject)
		and: [ (anObject size between: 1 and: 32)
		and: [ anObject allSatisfy: [ :char |
			(char between: $a and: $z)
				or: [ (char between: $A and: $Z)
				or: [ (char between: $0 and: $9)
				or: [ '_.:[]-' includes: char ] ] ] ] ] ]! !

!NotebookKernel class methodsFor: 'loop'!
badRequest: aByteArray

	| text |
	text := self decode: aByteArray.
	self emit: { #ev -> 'bad-request'. #text -> (text first: (text size min: 200)) }! !

!NotebookKernel class methodsFor: 'loop'!
decode: aByteArray
	"The page sends UTF-8; a native stdin may send anything"

	^ [ aByteArray utf8Decoded ] on: Error do: [ :error | error return: aByteArray asString ]! !

!NotebookKernel class methodsFor: 'cells'!
runCell: code rid: rid name: name
	"Evaluate the cell in a process of its own, which Stop may terminate, and wait for it.  A Stop that
	 came since the request was taken (StopNext) keeps the cell from running: it is checked before
	 the process is made, and as the first thing the process does"

	| t0 outcome session |
	ActiveRid := Serving := rid.
	self emit: { #ev -> 'start'. #rid -> rid. #name -> name }.
	t0 := Time millisecondClockValue.
	session := Session.
	StopNext == true ifFalse: [
		| done process |
		done := Semaphore new.
		process := [
			[	NotebookCell value: rid.
				StopNext == true ifFalse: [ outcome := self evaluate: code rid: rid name: name ] ]
				ensure: [ done signal ] ] newProcess.
		process priority: Processor userSchedulingPriority.
		process name: 'NotebookKernel cell'.
		"The watcher must not preempt the loop inside Process>>resume: it would terminate a process
		 that was never resumed, which resume then resumes a second time, and the VM spins"
		[	EvalDone := done.
			EvalProcess := process.
			process resume ] valueUnpreemptively.
		done wait.
		"A cell that saved the image ends there again when the image boots, in another session, with
		 another kernel: say nothing"
		session == Smalltalk session ifFalse: [ ^ self ].
		EvalProcess := EvalDone := nil ].
	outcome ifNil: [
		outcome := { #status -> (StopNext == true ifTrue: [ 'interrupted' ] ifFalse: [ 'terminated' ]) } ].
	self assertEnvironment.
	self emit: { #ev -> 'done'. #rid -> rid. #ms -> (Time millisecondClockValue - t0) } , outcome.
	ActiveRid := nil! !

!NotebookKernel class methodsFor: 'cells'!
evaluate: code rid: rid name: name
	"Parse the whole cell, so that a syntax error runs nothing, then compile and run its statements one
	 at a time.  Answer the fields of its done event"

	| tree syntax statements scope assigned value |
	tree := Smalltalk compiler source: code; isScripting: true; parse.
	syntax := self syntaxErrorIn: tree code: code.
	syntax ifNotNil: [ ^ { #status -> 'syntax'. #error -> syntax } ].
	statements := tree body statements.
	statements isEmpty ifTrue: [ ^ { #status -> 'ok'. #values -> nil } ].
	scope := self scopeOf: tree.
	assigned := self assignedNamesIn: tree.
	value := nil.
	1 to: statements size do: [ :index |
		| result |
		result := self
			run: (statements at: index)
			index: index
			code: code
			rid: rid
			name: name
			scope: scope
			assigned: assigned.
		result key == #failed ifTrue: [ ^ result value ].
		value := result value.
		result key == #returned ifTrue: [ ^ self outcomeOf: value rid: rid ] ].
	^ self outcomeOf: value rid: rid! !

!NotebookKernel class methodsFor: 'cells'!
run: statement index: index code: code rid: rid name: name scope: scope assigned: assigned
	"Compile the statement and run it.  Answer #next -> its value, #returned -> the value of a ^ that
	 ended the cell, or #failed -> the fields of done.  A statement that is not a ^ is compiled as
	 NotebookKernel fellThrough: (statement), so that a ^ in a block of it, which returns from the
	 statement's method only, is seen: fellThrough: did not run then.  The positions of the method
	 are those of the cell plus offset"

	| return text offset method value |
	return := statement isReturn.
	text := code copyFrom: statement start to: statement stop.
	offset := statement start - 1.
	return ifFalse: [
		text := self prefix , text , ')'.
		offset := offset - self prefix size ].
	method := self
		compile: text
		scope: scope
		assigned: assigned
		ifError: [ :error | ^ #failed -> (self compileError: error index: index offset: offset code: code) ].
	method propertyAt: #notebookCell put: { rid. name. index. offset. code. statement start. statement stop }.
	Fell := false.
	value := [ nil withArgs: #() executeMethod: method ]
		on: UnhandledException
		do: [ :exception |
			"A Warning goes on to its default action, which reports it and resumes (see #handleWarning:)"
			((exception isKindOf: UnhandledError) and: [ exception exception isKindOf: Warning ])
				ifTrue: [ exception pass ].
			^ #failed -> (self runtimeError: exception index: index rid: rid) ].
	^ (return or: [ Fell ~~ true ])
		ifTrue: [ #returned -> value ]
		ifFalse: [ #next -> value ]! !

!NotebookKernel class methodsFor: 'cells'!
prefix
	"What a statement that is not a ^ is compiled in, up to the statement (see #run:index:code:rid:name:scope:assigned:)"

	^ 'NotebookKernel fellThrough: ('! !

!NotebookKernel class methodsFor: 'cells'!
fellThrough: anObject
	"The statement around anObject ended without a ^"

	Fell := true.
	^ anObject! !

!NotebookKernel class methodsFor: 'cells'!
compile: text scope: scope assigned: assigned ifError: errorBlock
	"Compile a statement.  A variable that the cell assigns somewhere, and nobody declared, becomes a
	 variable of the notebook, a WorkspaceVariable, as in a Playground; one that it only reads is an
	 error"

	| method retry |
	[	retry := false.
		method := [ Smalltalk compiler source: text; isScripting: true; bindings: scope; compile ]
			on: self codeErrorClass
			do: [ :error |
				| name |
				name := self undeclaredNameOf: error.
				(name notNil and: [ (assigned includes: name) and: [ (scope includesKey: name) not ] ])
					ifFalse: [ ^ errorBlock value: error ].
				self declare: name in: scope.
				retry := true.
				error return: nil ].
		retry ] whileTrue.
	^ method! !

!NotebookKernel class methodsFor: 'cells'!
declare: name in: scope
	"add:, not at:put:, which would wrap the variable in an Association"

	| variable |
	variable := Bindings associationAt: name ifAbsent: [ nil ].
	variable ifNil: [
		variable := WorkspaceVariable key: name.
		Bindings add: variable ].
	scope add: variable! !

!NotebookKernel class methodsFor: 'cells'!
scopeOf: tree
	"The variables of a cell: a fresh one for each of its temporaries, and those of the notebook,
	 shared, so that assigning them lasts.  Dictionary>>#add: would change the value of a variable of
	 the same name: the temporaries go first"

	| scope |
	scope := Dictionary new.
	tree body temporaries do: [ :each | scope add: (WorkspaceVariable key: each name asSymbol) ].
	Bindings associationsDo: [ :each | (scope includesKey: each key) ifFalse: [ scope add: each ] ].
	^ scope! !

!NotebookKernel class methodsFor: 'cells'!
assignedNamesIn: tree
	"The names that the cell assigns, other than its temporaries and those of its blocks"

	| names |
	names := Set new.
	tree allChildren do: [ :node |
		(node isAssignment and: [ (self isDeclared: node variable name above: node) not ])
			ifTrue: [ names add: node variable name asSymbol ] ].
	^ names! !

!NotebookKernel class methodsFor: 'cells'!
isDeclared: name above: aNode
	"Whether name is a temporary or an argument of a block around aNode, or of the cell"

	| node |
	node := aNode parent.
	[ node notNil ] whileTrue: [
		(node isSequence and: [ node temporaries anySatisfy: [ :each | each name = name ] ]) ifTrue: [ ^ true ].
		(node isBlock and: [ node arguments anySatisfy: [ :each | each name = name ] ]) ifTrue: [ ^ true ].
		node := node parent ].
	^ false! !

!NotebookKernel class methodsFor: 'cells'!
undeclaredNameOf: aCodeError

	^ [	aCodeError notice isUndeclaredNotice
			ifTrue: [ aCodeError notice node name asSymbol ]
			ifFalse: [ nil ] ]
		on: Error
		do: [ :error | error return: nil ]! !

!NotebookKernel class methodsFor: 'cells'!
outcomeOf: value rid: rid
	"The fields of done for the value of the cell: its printString, unless it shows nothing (nil, the
	 Transcript, Notebook).  A display, or something to see, is shown instead"

	| text |
	(value isNil or: [ value == OurTranscript or: [ value == Notebook ] ])
		ifTrue: [ ^ { #status -> 'ok'. #values -> nil } ].
	((value isKindOf: NotebookDisplay) or: [ Notebook isVisual: value ]) ifTrue: [
		[ Notebook show: value id: nil ]
			on: Error
			do: [ :error | ^ self printError: error rid: rid ].
		^ { #status -> 'ok'. #values -> nil } ].
	text := [ value printStringLimitedTo: self printLimit + 1 ]
		on: Error
		do: [ :error | ^ self printError: error rid: rid ].
	^ text size > self printLimit
		ifTrue: [ { #status -> 'ok'. #values -> { text first: self printLimit }. #truncated -> true } ]
		ifFalse: [ { #status -> 'ok'. #values -> { text } } ]! !

!NotebookKernel class methodsFor: 'cells'!
printLimit

	^ 65536! !

!NotebookKernel class methodsFor: 'errors'!
syntaxErrorIn: tree code: code
	"The error of a cell that does not parse, or nil.  A pragma would be dropped: it is one too"

	| errors node |
	errors := tree allChildren select: [ :each | each isParseError ].
	errors isEmpty ifFalse: [
		node := errors detectMin: [ :each | each start ].
		^ self syntaxError: node errorMessage at: node start code: code ].
	tree pragmas isEmpty ifFalse: [
		^ self syntaxError: 'pragmas are not allowed in a cell' at: tree pragmas first start code: code ].
	^ nil! !

!NotebookKernel class methodsFor: 'errors'!
syntaxError: message at: position code: code

	| place |
	place := self lineAndColumnOf: position in: code.
	^ {	#text -> ('Syntax error: ' , message , (self where: place)).
		#kind -> #('syntax').
		#location -> nil.
		#form -> nil.
		#line -> place first.
		#column -> place last.
		#chain -> #() }! !

!NotebookKernel class methodsFor: 'errors'!
compileError: aCodeError index: index offset: offset code: code
	"A statement that does not compile: the fields of done.  Pharo 15 says 'Undeclared variable named x
	 in DoIt': say it as Pharo 12 does"

	| undeclared message position place |
	LastError := aCodeError.
	undeclared := self undeclaredNameOf: aCodeError.
	message := undeclared
		ifNil: [ self messageOf: aCodeError ]
		ifNotNil: [ 'Undeclared variable ' , undeclared ].
	position := [ aCodeError position ] on: Error do: [ :error | error return: nil ].
	place := position
		ifNil: [ { nil. nil } ]
		ifNotNil: [ self lineAndColumnOf: (position + offset max: 1) in: code ].
	^ {	#status -> 'error'.
		#error -> {
			#text -> ('Error: ' , aCodeError class name , (message ifEmpty: [ '' ] ifNotEmpty: [ ' ' , message ])
				, (place first ifNil: [ '' ] ifNotNil: [ self where: place ])).
			#kind -> (self kindOf: aCodeError).
			#location -> nil.
			#form -> index.
			#line -> place first.
			#column -> place last.
			#chain -> #() } }! !

!NotebookKernel class methodsFor: 'errors'!
runtimeError: anException index: index rid: rid
	"Described while its frames are there: the handler runs on top of them"

	| error |
	error := self unwrap: anException.
	LastError := error.
	^ { #status -> 'error'. #error -> (self describe: error rid: rid form: index) }! !

!NotebookKernel class methodsFor: 'errors'!
printError: anError rid: rid

	LastError := anError.
	^ { #status -> 'error'. #error -> (self describe: anError rid: rid form: 'print') }! !

!NotebookKernel class methodsFor: 'errors'!
describe: anError rid: rid form: form
	"The error object of an event; a fallback when describing it raises another (a messageText that
	 fails, say)"

	^ [ self errorOf: anError rid: rid form: form ]
		on: Error
		do: [ :error |
			error return: {
				#text -> 'Error: an error occurred while reporting an error'.
				#kind -> #().
				#location -> nil.
				#form -> nil.
				#line -> nil.
				#column -> nil.
				#chain -> #() } ]! !

!NotebookKernel class methodsFor: 'errors'!
errorOf: anError rid: rid form: form
	"text 'Error: <class> <messageText>', as the Console says it; kind, the classes up to Exception;
	 location, the method that raised it; form, the statement; line and column in the cell, of the
	 innermost frame of the cell; chain, the frames from the statement to the one that raised it, at
	 most 20, outermost first"

	| text message frames found cell place location chain |
	text := 'Error: ' , anError class name.
	message := anError messageText asString.
	message isEmpty ifFalse: [ text := text , ' ' , message ].
	found := self framesFrom: anError signalerContext rid: rid.
	frames := found first.
	cell := found last.
	place := cell
		ifNil: [ { nil. nil } ]
		ifNotNil: [ self lineAndColumnOf: (self positionOf: cell key info: cell value) in: (cell value at: 5) ].
	location := frames isEmpty
		ifTrue: [ nil ]
		ifFalse: [ frames first key method method printString ].
	(frames anySatisfy: [ :each | each value notNil ]) ifTrue: [
		[ frames last value isNil ] whileTrue: [ frames removeLast ] ].
	chain := frames reversed asArray collect: [ :each | self frame: each key info: each value ].
	^ {	#text -> text.
		#kind -> (self kindOf: anError).
		#location -> location.
		#form -> (form ifNil: [ cell ifNotNil: [ cell value at: 3 ] ]).
		#line -> place first.
		#column -> place last.
		#chain -> chain }! !

!NotebookKernel class methodsFor: 'errors'!
framesFrom: aContext rid: rid
	"{ the frames from aContext up, innermost first, at most 20, as context -> statement info (nil out
	 of a cell). the innermost frame of cell rid }.  They end at the statement's method; in another
	 process, at its first frame.  The kernel's frames are left out, and so are those of a message not
	 understood (Object>>#doesNotUnderstand:, and above it Pharo 15's handleDoesNotUnderstand:to:): the
	 frame that sent it comes last.  The printString of the value ends at the kernel that prints it"

	| frames cell context steps |
	frames := OrderedCollection new.
	context := aContext.
	steps := 0.
	[ context notNil and: [ steps < 100000 and: [ (self isEvaluation: context) not ] ] ] whileTrue: [
		| info |
		info := self infoOf: context.
		((self isHidden: context) or: [ frames size >= 20 ]) ifFalse: [ frames add: context -> info ].
		(cell isNil and: [ info notNil and: [ (info at: 1) = rid ] ]) ifTrue: [ cell := context -> info ].
		(info notNil and: [ (info at: 1) = rid and: [ context isBlockContext not ] ])
			ifTrue: [ context := nil ]
			ifFalse: [
				context := context sender.
				steps := steps + 1 ] ].
	^ { frames. cell }! !

!NotebookKernel class methodsFor: 'errors'!
infoOf: aContext
	"{rid. name. statement. offset. code. start. stop} of the statement whose method aContext, or its
	 home, runs, or nil.  A block made by an earlier cell carries the statement of that cell"

	| method info |
	method := aContext method method.
	method selector == #DoIt ifFalse: [ ^ nil ].
	info := method propertyAt: #notebookCell ifAbsent: [ ^ nil ].
	^ (info isArray and: [ info size = 7 ]) ifTrue: [ info ] ifFalse: [ nil ]! !

!NotebookKernel class methodsFor: 'errors'!
isEvaluation: aContext
	"Whether aContext is the kernel's, below what the cell runs"

	^ aContext methodClass == self class
		and: [ #( #outcomeOf:rid: #evaluate:rid:name: #run:index:code:rid:name:scope:assigned: ) includes: aContext selector ]! !

!NotebookKernel class methodsFor: 'errors'!
isHidden: aContext

	| class |
	class := aContext methodClass.
	^ class == self class
		or: [ class == self
		or: [ aContext selector == #handleDoesNotUnderstand:to:
		or: [ aContext selector == #doesNotUnderstand: and: [ class == Object or: [ class == ProtoObject ] ] ] ] ]! !

!NotebookKernel class methodsFor: 'errors'!
positionOf: aContext info: info
	"Where in its cell the frame of a statement is: the start of the node it runs, within the
	 statement"

	| node |
	node := [ aContext sourceNodeExecuted ] on: Error do: [ :error | error return: nil ].
	node ifNil: [ ^ info at: 6 ].
	^ (node start + (info at: 4) max: (info at: 6)) min: (info at: 7)! !

!NotebookKernel class methodsFor: 'errors'!
frame: aContext info: info
	"{where. proc. form} of a frame: In[7]:2, the line of the cell, or Class>>selector; the method or
	 block, null for the statement; the source of the node it runs, at most 100 characters"

	| name proc node form start stop |
	name := aContext methodClass name , '>>' , aContext selector.
	proc := aContext isBlockContext ifTrue: [ '[] in ' , name ] ifFalse: [ name ].
	node := [ aContext sourceNodeExecuted ] on: Error do: [ :error | error return: nil ].
	info ifNil: [
		form := [ node sourceCode asString ] on: Error do: [ :error | error return: nil ].
		^ {	#where -> name.
			#proc -> proc.
			#form -> (form ifNotNil: [ form first: (form size min: 100) ]) } ].
	"The node within the statement, in the cell: without the prefix"
	start := info at: 6.
	stop := info at: 7.
	node ifNotNil: [
		start := node start + (info at: 4) max: start.
		stop := node stop + (info at: 4) min: stop ].
	form := start <= stop
		ifTrue: [ (info at: 5) copyFrom: start to: stop ]
		ifFalse: [ '' ].
	^ {	#where -> ((info at: 2) , ':' , (self decimal: (self lineAndColumnOf: (self positionOf: aContext info: info) in: (info at: 5)) first)).
		#proc -> (aContext isBlockContext ifTrue: [ proc ] ifFalse: [ nil ]).
		#form -> (form first: (form size min: 100)) }! !

!NotebookKernel class methodsFor: 'errors'!
kindOf: anException
	"The classes of anException, up to Exception"

	| kinds class |
	kinds := OrderedCollection new.
	class := anException class.
	[ class notNil ] whileTrue: [
		kinds add: class name asString.
		class := class == Exception ifTrue: [ nil ] ifFalse: [ class superclass ] ].
	^ kinds asArray! !

!NotebookKernel class methodsFor: 'errors'!
unwrap: anException
	"A Halt comes as itself, any other exception in the UnhandledError of its default action"

	^ (anException isKindOf: UnhandledError)
		ifTrue: [ anException exception ]
		ifFalse: [ anException ]! !

!NotebookKernel class methodsFor: 'errors'!
lineAndColumnOf: position in: code
	"{line. column}, from 1, of the character at position; CR, LF and CRLF are a line break each"

	| line column |
	line := column := 1.
	1 to: (position - 1 min: code size) do: [ :index |
		| char |
		char := code at: index.
		(char = Character lf and: [ index > 1 and: [ (code at: index - 1) = Character cr ] ])
			ifFalse: [
				(char = Character cr or: [ char = Character lf ])
					ifTrue: [
						line := line + 1.
						column := 1 ]
					ifFalse: [ column := column + 1 ] ] ].
	^ { line. column }! !

!NotebookKernel class methodsFor: 'errors'!
where: place

	^ ' (line ' , (self decimal: place first) , ', column ' , (self decimal: place last) , ')'! !

!NotebookKernel class methodsFor: 'errors'!
codeErrorClass
	"Pharo 15 signals an OCCodeError, which its CodeError subclasses, and Pharo 12 has only CodeError"

	^ Smalltalk globals at: #OCCodeError ifAbsent: [ Smalltalk globals at: #CodeError ]! !

!NotebookKernel class methodsFor: 'errors'!
messageOf: anException

	^ [ anException messageText asString ] on: Error do: [ :error | error return: '' ]! !

!NotebookKernel class methodsFor: 'errors'!
textOf: anException
	"Error: <class> <message text>, never failing"

	| error message |
	error := self unwrap: anException.
	message := self messageOf: error.
	^ 'Error: ' , error class name , (message isEmpty ifTrue: [ '' ] ifFalse: [ ' ' , message ])! !

!NotebookKernel class methodsFor: 'error handling'!
handleError: anError
	"An error, or a Halt, that nothing handled in another process than a cell's, which catches its own:
	 report it to the cell that forked the process, and terminate the process.  The kernel's own
	 processes are started again; the handler that the kernel replaced quits the image when that
	 fails"

	| process rid |
	process := Processor activeProcess.
	(self isKernelProcess: process) ifTrue: [ ^ self restart: process after: anError ].
	rid := [ NotebookCell value ] on: Error do: [ :error | error return: nil ].
	[	rid
			ifNil: [ self writeError: (self textOf: anError) ]
			ifNotNil: [
				| error |
				error := self unwrap: anError.
				LastError := error.
				self emit: { #ev -> 'error'. #rid -> rid. #error -> (self describe: error rid: rid form: nil) } ] ]
		on: Error
		do: [ :error | error return ].
	process terminate! !

!NotebookKernel class methodsFor: 'error handling'!
restart: process after: anError

	self writeError: 'NotebookKernel: ' , process name , ': ' , (self textOf: anError).
	[	process == Loop ifTrue: [
			Loop := self fork: [ self loop: Generation ] at: Processor userSchedulingPriority named: 'NotebookKernel loop' ].
		process == Watcher ifTrue: [
			Watcher := self fork: [ self watch: InterruptSemaphore ] at: Processor userInterruptPriority named: 'NotebookKernel watcher' ].
		process == Flusher ifTrue: [
			Flusher := self fork: [ self flushEvery: 50 ] at: Processor userSchedulingPriority + 1 named: 'NotebookKernel flusher' ].
		process == Idler ifTrue: [
			Idler := self fork: [ self idle: Generation ] at: Processor lowestPriority + 1 named: 'NotebookKernel idler' ] ]
		on: Error
		do: [ :error |
			Fallback ifNil: [ ^ Smalltalk exit: 1 ].
			^ Fallback handleError: anError ].
	process terminate! !

!NotebookKernel class methodsFor: 'error handling'!
handleWarning: aWarning
	"A Warning that nothing handled: report it on stderr, and let its process go on, as the headless
	 image does"

	[	OurTranscript ifNotNil: [ :transcript | transcript flushLocal ].
		self write: 'Warning: ' , (self messageOf: aWarning) , String lf on: Stdio stderr ]
		on: Error
		do: [ :error | error return ].
	^ aWarning resume! !

!NotebookKernel class methodsFor: 'error handling'!
handleNotification: aNotification

	^ Fallback handleNotification: aNotification! !

!NotebookKernel class methodsFor: 'error handling'!
onPrimitiveError: aString
	"Pharo 15 sends it to the error handler"

	^ Fallback onPrimitiveError: aString! !

!NotebookKernel class methodsFor: 'error handling'!
isKernelProcess: aProcess

	^ aProcess == Loop or: [ aProcess == Watcher or: [ aProcess == Flusher or: [ aProcess == Idler ] ] ]! !

!NotebookKernel class methodsFor: 'stop'!
watch: aSemaphore
	"Stop signals aSemaphore"

	[	aSemaphore wait; consumeAllSignals.
		self stop ] repeat! !

!NotebookKernel class methodsFor: 'stop'!
stop
	"Terminate the cell that runs; its ensure: blocks run, then the loop reports it interrupted.  Else,
	 when a request is being served, the cell it starts must not run (StopNext).  Else Stop may have
	 come with the request it is for, in the same turn of the page: take what stdin has, and when it
	 holds a request, that request is stopped.  Otherwise the Stop is stale, too late for a cell that
	 ended: drop it, then and there.  That the line in stdin came before the Stop holds because the
	 watcher runs in the slice that the Stop starts (see #idle:): a request posted later finds the Stop
	 gone, and the loop clears StopNext after every request.  The watcher runs above the loop, which
	 does not preempt it.  The loop may wait for the input that the watcher took: wake it"

	| process done |
	process := EvalProcess.
	process ifNotNil: [
		done := EvalDone.
		StopNext := true.
		EvalProcess := EvalDone := nil.
		process terminate.
		done ifNotNil: [ done signal ].
		^ self ].
	Serving ifNotNil: [ ^ StopNext := true ].
	[	self drain.
		(self indexOfLf: Pending) > 0 ifTrue: [
			StopNext := true.
			Wake signal ] ] valueUnpreemptively! !

!NotebookKernel class methodsFor: 'stop'!
idle: generation
	"Be the idle process of the web VM, just above the image's, while the kernel waits.  vm_interrupt()
	 signals the semaphore of Stop between slices, and the VM signals it at its next check of events,
	 which the image's idle process may never reach before the VM sleeps again: it calls the
	 relinquish primitive first, which asks the VM to sleep, and the check after it returns to the
	 host before it signals anything.  Then the watcher would run only at the next timer or input,
	 up to 50 ms later, with the next request perhaps, which it would take for the request of that
	 Stop.  Here a send that builds a frame comes before every relinquish, and its check signals the
	 semaphore in the slice that vm_interrupt() starts (unless a slice ended at that very check, which
	 only one whose time was up does).  The VM sleeps as long as with the image's"

	[ self isCurrent: generation ] whileTrue: [
		self idleTick.
		ProcessorScheduler relinquishProcessorForMicroseconds: 50000.
		Processor yield ]! !

!NotebookKernel class methodsFor: 'stop'!
idleTick
	"A send that builds a frame: not a quick method, which a primitive runs without one"

	| tick |
	tick := 1.
	^ tick! !

!NotebookKernel class methodsFor: 'stop'!
stopBackground
	"Terminate the processes that cells forked, but not those above the priority of the cells, and not
	 the system's that a cell made start, as the Morphic UI process.  Stop never does"

	| index victims |
	index := NotebookCell soleInstance index.
	victims := Process allSubInstances select: [ :process | self isBackground: process index: index ].
	victims do: [ :process | [ process terminate ] on: Error do: [ :error | error return ] ].
	self emit: { #ev -> 'stopped'. #count -> victims size }! !

!NotebookKernel class methodsFor: 'stop'!
isBackground: aProcess index: index

	^ aProcess ~~ Processor activeProcess
		and: [ aProcess isTerminated not
		and: [ (aProcess psValueAt: index) notNil
		and: [ aProcess priority <= Processor userSchedulingPriority
		and: [ (self isKernelProcess: aProcess) not
		and: [ aProcess ~~ EvalProcess
		and: [ (self isSpared: aProcess) not ] ] ] ] ] ]! !

!NotebookKernel class methodsFor: 'stop'!
isSpared: aProcess

	| name |
	name := aProcess name ifNil: [ ^ false ].
	^ self sparedNames anySatisfy: [ :each | name asString beginsWith: each ]! !

!NotebookKernel class methodsFor: 'stop'!
sparedNames
	"The beginnings of the names of the processes of the system, which a cell may start again, and
	 which inherit its rid then.  On Pharo 12 and 15 none that a library starts on its first use in a
	 cell inherits it (the FFI's callbacks, cairo, FreeType, Roassal, Morphic's world, Iceberg,
	 OSWindow): the system's processes all exist when the image starts.  OSSDL2Driver's is at
	 lowIOPriority, above the cells, as OSWebDriver's are"

	^ #('Morphic UI Process' 'SDL2 Event loop' 'Web event loop' 'Web interrupt watcher' 'Idle Process'
		'Finalization Process' 'Low Space Watcher' 'WatchDog' 'ThreadSafeTranscript worker'
		'DelaySemaphoreScheduler' 'Callback queue')! !

!NotebookKernel class methodsFor: 'events'!
emit: fields
	"Send an event, an Array of Associations"

	self emit: fields bytes: nil! !

!NotebookKernel class methodsFor: 'events'!
emit: fields bytes: attachment
	"Send an event, then attachment, a ByteArray or the Bitmap of a Form.  What the running cell's
	 Transcript, stdout and stderr have comes before it, and so does what the Transcript of the
	 event's cell has, when that cell does not run.  The JSON is made before, and written in one
	 unpreemptible block: no other event comes between its line and its attachment, and Stop cannot
	 cut it"

	| line |
	self flushBefore: fields.
	line := self jsonLine: (attachment
		ifNil: [ fields ]
		ifNotNil: [ fields , { #bytes -> attachment byteSize } ]).
	[	[	Events nextPutAll: line.
			attachment ifNotNil: [
				Events nextPutAll: attachment.
				Events nextPut: 10 ].
			Events flush ] valueUnpreemptively ]
		on: Error
		do: [ :error | self writeError: 'NotebookKernel: cannot send an event: ' , (self messageOf: error) ]! !

!NotebookKernel class methodsFor: 'events'!
flushBefore: fields

	| rid |
	rid := (fields detect: [ :each | each key == #rid ] ifNone: [ nil ]) ifNotNil: [ :field | field value ].
	[	OurTranscript ifNotNil: [ :transcript |
			transcript flushLocal.
			(fields first value ~= 'stream' and: [ rid notNil and: [ rid ~= ActiveRid ] ])
				ifTrue: [ transcript flushRid: rid ] ] ]
		on: Error
		do: [ :error | error return ].
	[ Stdio stdout flush ] on: Error do: [ :error | error return ].
	[ Stdio stderr flush ] on: Error do: [ :error | error return ]! !

!NotebookKernel class methodsFor: 'events'!
emitStream: aString rid: rid
	"What a process that cell rid forked showed on the Transcript"

	self emit: { #ev -> 'stream'. #rid -> rid. #name -> 'stdout'. #text -> aString }! !

!NotebookKernel class methodsFor: 'events'!
emitDisplay: aDisplay rid: rid id: id
	"Data over 4 MB, and images over 16 MB, go with their size only"

	| data |
	aDisplay isVisual ifTrue: [ ^ self emitImage: aDisplay rid: rid id: id ].
	data := aDisplay data.
	(data isString not and: [ data size <= 16777216 ]) ifTrue: [
		^ self
			emit: { #ev -> 'display'. #rid -> rid. #mime -> aDisplay mime. #id -> id }
			bytes: data ].
	data size > 4194304 ifTrue: [
		^ self emit: { #ev -> 'display'. #rid -> rid. #mime -> aDisplay mime. #data -> ''. #size -> data size. #id -> id } ].
	self emit: { #ev -> 'display'. #rid -> rid. #mime -> aDisplay mime. #data -> data. #id -> id }! !

!NotebookKernel class methodsFor: 'events'!
emitImage: aDisplay rid: rid id: id
	"The pixels of a Form: its 32-bit words, in the order of the memory, B G R A.  A Form without
	 pixels, 0 wide or high, is the text empty image WxH, not an image too large to send"

	| form size |
	form := aDisplay pixels.
	(form width < 1 or: [ form height < 1 ]) ifTrue: [
		^ self emit: {
			#ev -> 'display'.
			#rid -> rid.
			#mime -> 'text/plain'.
			#data -> ('empty image ' , (self decimal: form width) , 'x' , (self decimal: form height)).
			#id -> id } ].
	size := form width * form height * 4.
	((form width between: 1 and: 16384) and: [ (form height between: 1 and: 16384) and: [ size <= 16777216 ] ])
		ifFalse: [ ^ self emit: { #ev -> 'display'. #rid -> rid. #mime -> 'image/png'. #data -> ''. #size -> size. #id -> id } ].
	form bits byteSize = size ifFalse: [ ^ self error: 'the Form has ' , (self decimal: form bits byteSize) , ' bytes for its pixels' ].
	self
		emit: {
			#ev -> 'display'.
			#rid -> rid.
			#mime -> 'image/x-pharo-bgra'.
			#width -> form width.
			#height -> form height.
			#premultiplied -> aDisplay premultiplied.
			#id -> id }
		bytes: form bits! !

!NotebookKernel class methodsFor: 'events'!
jsonLine: fields
	"The UTF-8 of the JSON of fields, and an LF"

	^ self utf8Of: (String streamContents: [ :stream |
		self writeJson: fields on: stream.
		stream nextPut: Character lf ])! !

!NotebookKernel class methodsFor: 'events'!
writeJson: anObject on: aStream
	"A small JSON writer of what events hold: an Array of Associations is an object.  A cell may
	 redefine printOn:, so integers are written here"

	anObject ifNil: [ ^ aStream nextPutAll: 'null' ].
	anObject == true ifTrue: [ ^ aStream nextPutAll: 'true' ].
	anObject == false ifTrue: [ ^ aStream nextPutAll: 'false' ].
	anObject isInteger ifTrue: [ ^ aStream nextPutAll: (self decimal: anObject) ].
	anObject isString ifTrue: [ ^ self writeJsonString: anObject on: aStream ].
	(anObject isArray and: [ anObject notEmpty and: [ anObject first isKindOf: Association ] ]) ifTrue: [
		aStream nextPut: ${.
		anObject
			do: [ :each |
				self writeJsonString: each key on: aStream.
				aStream nextPut: $:.
				self writeJson: each value on: aStream ]
			separatedBy: [ aStream nextPut: $, ].
		^ aStream nextPut: $} ].
	anObject isArray ifTrue: [
		aStream nextPut: $[.
		anObject do: [ :each | self writeJson: each on: aStream ] separatedBy: [ aStream nextPut: $, ].
		^ aStream nextPut: $] ].
	aStream nextPutAll: 'null'! !

!NotebookKernel class methodsFor: 'events'!
writeJsonString: aString on: aStream
	"Escape the quote, the backslash and the control characters, and write the rest as it is (it goes
	 out as UTF-8), U+FFFD for what UTF-8 cannot hold.  The runs between the escapes of a ByteString are
	 copied whole, found with a primitive of the MiscPrimitivePlugin"

	| string |
	string := aString asString.
	aStream nextPut: $".
	string isByteString
		ifTrue: [
			| map start index |
			map := ByteArray new: 256.
			1 to: 32 do: [ :byte | map at: byte put: 1 ].
			map at: 35 put: 1; at: 93 put: 1.
			start := 1.
			[ (index := ByteString findFirstInString: string inSet: map startingAt: start) > 0 ] whileTrue: [
				start < index ifTrue: [ aStream next: index - start putAll: string startingAt: start ].
				self writeEscape: (string at: index) on: aStream.
				start := index + 1 ].
			start <= string size ifTrue: [ aStream next: string size - start + 1 putAll: string startingAt: start ] ]
		ifFalse: [
			string do: [ :char |
				(char asInteger < 32 or: [ char = $" or: [ char = $\ ] ])
					ifTrue: [ self writeEscape: char on: aStream ]
					ifFalse: [ aStream nextPut: ((self isEncodable: char) ifTrue: [ char ] ifFalse: [ self replacement ]) ] ] ].
	aStream nextPut: $"! !

!NotebookKernel class methodsFor: 'events'!
writeEscape: aCharacter on: aStream

	| code |
	code := aCharacter asInteger.
	code = 34 ifTrue: [ ^ aStream nextPutAll: '\"' ].
	code = 92 ifTrue: [ ^ aStream nextPutAll: '\\' ].
	code = 10 ifTrue: [ ^ aStream nextPutAll: '\n' ].
	code = 13 ifTrue: [ ^ aStream nextPutAll: '\r' ].
	code = 9 ifTrue: [ ^ aStream nextPutAll: '\t' ].
	aStream
		nextPutAll: '\u00';
		nextPut: ('0123456789abcdef' at: code // 16 + 1);
		nextPut: ('0123456789abcdef' at: code \\ 16 + 1)! !

!NotebookKernel class methodsFor: 'events'!
decimal: anInteger
	"The digits of anInteger, which do not depend on printOn:"

	| digits n |
	anInteger = 0 ifTrue: [ ^ '0' ].
	anInteger < 0 ifTrue: [ ^ '-' , (self decimal: 0 - anInteger) ].
	digits := WriteStream on: (String new: 12).
	n := anInteger.
	[ n > 0 ] whileTrue: [
		digits nextPut: (Character value: n \\ 10 + 48).
		n := n // 10 ].
	^ digits contents reversed! !

!NotebookKernel class methodsFor: 'events'!
isEncodable: aCharacter
	"Whether UTF-8 holds it: not a surrogate, not above U+10FFFF (utf8Encoded refuses those)"

	| code |
	code := aCharacter asInteger.
	^ code < 16rD800 or: [ code > 16rDFFF and: [ code <= 16r10FFFF ] ]! !

!NotebookKernel class methodsFor: 'events'!
replacement

	^ Character value: 16rFFFD! !

!NotebookKernel class methodsFor: 'events'!
utf8Of: aString
	"The UTF-8 of aString, U+FFFD for what UTF-8 cannot hold.  A ByteString of ASCII is its bytes"

	| string map |
	string := aString asString.
	string isByteString ifTrue: [
		map := ByteArray new: 256.
		129 to: 256 do: [ :byte | map at: byte put: 1 ].
		^ (ByteString findFirstInString: string inSet: map startingAt: 1) = 0
			ifTrue: [ string asByteArray ]
			ifFalse: [ string utf8Encoded ] ].
	(string allSatisfy: [ :char | self isEncodable: char ]) ifFalse: [
		string := string collect: [ :char | (self isEncodable: char) ifTrue: [ char ] ifFalse: [ self replacement ] ] ].
	^ string utf8Encoded! !

!NotebookKernel class methodsFor: 'events'!
write: aString on: aStream
	"Stdio streams are binary.  Flush at once: the page shows output as it comes.  An error writing
	 must not reach a process of the kernel"

	[ aStream nextPutAll: (self utf8Of: aString); flush ] on: Error do: [ :error | error return ]! !

!NotebookKernel class methodsFor: 'events'!
writeError: aString

	self write: aString , String lf on: Stdio stderr! !

!NotebookKernel class methodsFor: 'flushing'!
flushEvery: milliseconds
	"Send what the Transcript has, and take the environment back, every so often"

	[	(Delay forMilliseconds: milliseconds) wait.
		self tick ] repeat! !

!NotebookKernel class methodsFor: 'flushing'!
tick

	self assertEnvironment.
	[ OurTranscript ifNotNil: [ :transcript | transcript flush ] ] on: Error do: [ :error | error return ]! !

!NotebookKernel class methodsFor: 'accessing'!
activeRid
	"The rid of the cell that runs, or nil"

	^ ActiveRid! !

!NotebookKernel class methodsFor: 'accessing'!
bindings
	"The variables of the notebook"

	^ Bindings ifNil: [ Bindings := Dictionary new ]! !

!NotebookKernel class methodsFor: 'accessing'!
forgetVariables

	Bindings := Dictionary new! !

!NotebookKernel class methodsFor: 'accessing'!
lastError

	^ LastError! !

!NotebookKernel class methodsFor: 'primitives'!
primSetInterruptSemaphore: semaphoreIndex
	"Natively there is no Stop"

	<primitive: #primitiveSetInterruptSemaphore module: #WebHostPlugin>
	^ nil! !

NotebookKernel startIfRequested!
