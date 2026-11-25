"What tests/wasm/webfetch.spec.mjs evaluates in the VMs of the pages: the class WFP, with the
 primitives of the WebFetchPlugin (without the package WebFetch) and what the spec does with them.
 The methods of the second part answer a String between double brackets, which the spec looks for."

| wfp |
((Object << #WFP slots: {  }) package: 'WebFetchLane') install.
wfp := Smalltalk at: #WFP.
#( 'availability' 'primitiveFetchAvailability'
   'start: s head: h body: b window: w timeout: t flags: f' 'primitiveFetchStart'
   'state: id' 'primitiveFetchState'
   'head: id' 'primitiveFetchHead'
   'read: id into: b at: s count: n' 'primitiveFetchRead'
   'close: id' 'primitiveFetchClose' ) pairsDo: [ :pattern :primitive |
	wfp class compile: pattern , '
	<primitive: #' , primitive , ' module: #WebFetchPlugin error: ec>
	^ { ec }' ].
wfp class compile: 'start: aString on: sem flags: f
	"The request is METHOD, URL and headers, separated by blanks and | between the headers"
	| parts |
	parts := aString findTokens: ''|''.
	^ self
		start: (Smalltalk registerExternalObject: sem)
		head: (String lf join: (parts first substrings , parts allButFirst)) utf8Encoded
		body: ((parts first beginsWith: ''P'') ifTrue: [ #[ 104 105 ] ])
		window: 1000000 timeout: 0 flags: f'.
wfp class compile: 'say: aString
	^ ''[['' , aString , '']]'''.

wfp class compile: 'available
	^ self say: ''availability '' , self availability printString'.
wfp class compile: 'fetch: aString
	^ self fetch: aString flags: 0'.
wfp class compile: 'fetch: aString flags: f
	"The state, the lines of the record, and the size and the begin of the body"
	| sem id bytes lines |
	sem := Semaphore new.
	id := self start: aString on: sem flags: f.
	id isArray ifTrue: [ ^ self say: id first asString ].
	^ [
		[ ((self state: id) bitAnd: 7) >= 3 ] whileFalse: [ sem wait ].
		bytes := ByteArray new: (self state: id) >> 3.
		self read: id into: bytes at: 1 count: bytes size.
		lines := (self head: id) utf8Decoded lines.
		self say: (String streamContents: [ :s |
			s print: ((self state: id) bitAnd: 7).
			(lines copyWithout: '''') do: [ :each | s nextPut: $|; nextPutAll: each ].
			s nextPut: $|; print: bytes size; nextPut: $|; nextPutAll: (bytes utf8Decoded truncateTo: 200) ]) ]
		ensure: [ self close: id ]'.
wfp class compile: 'hang: aString
	"Open a request, and leave it open"
	| sem |
	sem := Semaphore new.
	Smalltalk at: #WFHang put: (self start: aString on: sem flags: 0) -> sem.
	^ self say: ''hang '' , (Smalltalk at: #WFHang) key isInteger printString'.
wfp class compile: 'hung
	"Its end"
	| id |
	id := (Smalltalk at: #WFHang) key.
	[ ((self state: id) bitAnd: 7) >= 3 ] whileFalse: [ (Smalltalk at: #WFHang) value wait ].
	^ [ self say: ''hung '' , ((self state: id) bitAnd: 7) printString , '' '' , (self head: id) utf8Decoded lines first ]
		ensure: [ self close: id ]'.
wfp class compile: 'socket: port
	| socket |
	^ self say: ''socket '' , ([
		socket := Socket newTCP.
		socket connectTo: #[127 0 0 1] port: port waitForConnectionFor: 5.
		Smalltalk at: #WFSocket put: socket.
		socket isConnected printString ] on: Error do: [ :error | error class name asString ])'.
wfp class compile: 'send
	"Whether the socket of before takes data"
	^ self say: ''send '' , ([ (Smalltalk at: #WFSocket) sendData: ''x''. ''sent'' ] on: Error do: [ :error | error class name asString ])'.
wfp class compile: 'git: url
	^ self say: ''git '' , ([
		IceRepositoryCreator new
			remote: (IceGitRemote url: url);
			location: FileLocator imageDirectory / (''wf'' , Time microsecondClockValue printString);
			createRepository.
		''cloned'' ] on: Error do: [ :error | error messageText asString lines first ])'.
wfp class compile: 'debuggers
	^ World systemWindows select: [ :each |
		(each model respondsTo: #presenter) and: [ each model presenter isKindOf: StDebugger ] ]'.
wfp class compile: 'debugger
	"How many, and what the interrupted process of the first one runs"
	| windows |
	windows := self debuggers.
	^ self say: ''debuggers '' , windows size printString , (windows
		ifEmpty: [ '''' ]
		ifNotEmpty: [ '' '' , (windows first model presenter session interruptedContext stack
			anySatisfy: [ :each | each selector = #fetch: or: [ each selector = #fetch ] ]) printString ])'.
wfp class compile: 'closeDebuggers
	self debuggers do: [ :each | each delete ].
	^ self debugger'.
wfp say: 'ready'
