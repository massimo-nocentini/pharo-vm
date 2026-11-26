"What tests/wasm/websocket.spec.mjs evaluates in the VMs of the pages: the class WSP, with the
 primitives of the WebSocketPlugin (without the package WebFetch-WebSocket) and what the spec does
 with them.  The sockets have names, Symbols, and are kept in the global WSSockets, as
 {id. state semaphore. read semaphore. write semaphore}.  The methods of the second part answer a
 String between double brackets, which the spec looks for; the state of a socket is said as
 'state|the lines of its info record', such as '2|chat||ws://...' or '5|request|...'."

| wsp |
((Object << #WSP slots: {  }) package: 'WebSocketLane') install.
wsp := Smalltalk at: #WSP.
Smalltalk at: #WSSockets put: Dictionary new.
Smalltalk at: #WSGot put: nil.
#( 'availability' 'primitiveWebSocketAvailability'
   'open: s read: r write: w record: b maxMessage: m maxQueued: q timeout: t' 'primitiveWebSocketOpen'
   'state: id' 'primitiveWebSocketState'
   'info: id' 'primitiveWebSocketInfo'
   'receive: id' 'primitiveWebSocketReceive'
   'send: id bytes: b isText: t' 'primitiveWebSocketSend'
   'buffered: id' 'primitiveWebSocketBuffered'
   'close: id code: c reason: r' 'primitiveWebSocketClose'
   'release: id' 'primitiveWebSocketRelease' ) pairsDo: [ :pattern :primitive |
	wsp class compile: pattern , '
	<primitive: #' , primitive , ' module: #WebSocketPlugin error: ec>
	^ { ec }' ].
wsp class compile: 'say: aString
	^ ''[['' , aString , '']]'''.
wsp class compile: 'sock: name
	^ (Smalltalk at: #WSSockets) at: name'.

"The sockets"
wsp class compile: 'open: name url: url protocols: ps maxQueued: q timeout: t
	"Open a socket, and do not wait"
	| sems id |
	sems := { Semaphore new. Semaphore new. Semaphore new }.
	id := self
		open: (Smalltalk registerExternalObject: sems first)
		read: (Smalltalk registerExternalObject: sems second)
		write: (Smalltalk registerExternalObject: sems third)
		record: (String lf join: { url } , ps) utf8Encoded
		maxMessage: 4194304 maxQueued: q timeout: t.
	id isArray ifTrue: [
		sems do: [ :each | Smalltalk unregisterExternalObject: each ].
		^ self say: id first ].
	(Smalltalk at: #WSSockets) at: name put: { id } , sems.
	^ self say: ''opened'''.
wsp class compile: 'open: name url: url
	^ self open: name url: url protocols: #(  ) maxQueued: 16777216 timeout: 10000'.
wsp class compile: 'st: name
	| st |
	st := self state: (self sock: name) first.
	^ st isArray ifTrue: [ st first ] ifFalse: [ st bitAnd: 7 ]'.
wsp class compile: 'waitFor: name until: aBlock
	"Poll the state (and the count of messages) until aBlock is true, at most 20 s"
	| t st |
	t := Time millisecondClockValue.
	[ st := self state: (self sock: name) first.
	  st isArray or: [ (aBlock value: (st bitAnd: 7) value: st >> 3) or: [ Time millisecondClockValue - t > 20000 ] ] ]
		whileFalse: [ (Delay forMilliseconds: 10) wait ]'.
wsp class compile: 'lines: name
	"The state and the lines of the info record"
	| info |
	info := self info: (self sock: name) first.
	^ { (self st: name) printString } , (info
		ifNil: [ #(  ) ]
		ifNotNil: [ info isArray ifTrue: [ info ] ifFalse: [ (info utf8Decoded splitOn: Character lf) asArray ] ])'.
wsp class compile: 'status: name
	"Its state, once it is open or over"
	self waitFor: name until: [ :st :n | st >= 2 ].
	^ self say: (''|'' join: (self lines: name))'.
wsp class compile: 'connect: name url: url
	self open: name url: url.
	^ self status: name'.
wsp class compile: 'take: name
	"The next message: a String for text, a ByteArray for binary; nil"
	| m |
	m := self receive: (self sock: name) first.
	m isNil ifTrue: [ ^ nil ].
	m size = 1 ifTrue: [ ^ m ].
	^ m first ifTrue: [ m second utf8Decoded ] ifFalse: [ m second ]'.
wsp class compile: 'next: name
	"The next message, polling for it (at most 20 s)"
	self waitFor: name until: [ :st :n | n > 0 or: [ st >= 4 ] ].
	^ self take: name'.
wsp class compile: 'waitNext: name
	"The next message, waiting on the read semaphore with no deadline: what Stop interrupts"
	| m |
	[ (m := self take: name) isNil and: [ (self st: name) < 4 ] ] whileTrue: [ ((self sock: name) at: 3) wait ].
	^ m'.
wsp class compile: 'said: m
	^ m isString ifTrue: [ m ] ifFalse: [ m printString ]'.
wsp class compile: 'nextOf: name
	^ self say: (self said: (self next: name))'.
wsp class compile: 'waitNextOf: name
	^ self say: (self said: (self waitNext: name))'.
wsp class compile: 'send: name text: aString
	| answer |
	answer := self send: (self sock: name) first bytes: aString utf8Encoded isText: true.
	^ self say: ''sent '' , (answer isArray ifTrue: [ answer first ] ifFalse: [ ''true'' ])'.
wsp class compile: 'echo: name
	"Text, binary and 1 MiB, there and back"
	| id text bytes big got |
	id := (self sock: name) first.
	text := ''caf'' , (String with: (Character value: 233)).
	self send: id bytes: text utf8Encoded isText: true.
	got := self next: name.
	self send: id bytes: #[ 0 1 2 255 ] isText: false.
	bytes := self next: name.
	big := ByteArray new: 1048576.
	1 to: big size do: [ :i | big at: i put: i \\ 251 ].
	self send: id bytes: big isText: false.
	big := (self next: name) = big.
	^ self say: (got = text) printString , ''|'' , bytes printString , ''|'' , big printString'.
wsp class compile: 'close: name code: code
	"Close it, and its state once over"
	self close: (self sock: name) first code: code reason: nil.
	self waitFor: name until: [ :st :n | st >= 4 ].
	^ self say: (''|'' join: (self lines: name))'.
wsp class compile: 'free: name
	"Release it, and unregister its semaphores"
	| sock was |
	sock := (Smalltalk at: #WSSockets) removeKey: name ifAbsent: [ ^ self say: ''released none'' ].
	was := self release: sock first.
	sock allButFirst do: [ :each | Smalltalk unregisterExternalObject: each ].
	^ self say: ''released '' , was printString'.
wsp class compile: 'freeAll
	(Smalltalk at: #WSSockets) keys do: [ :each | self free: each ].
	^ self say: ''released all'''.
wsp class compile: 'available
	^ self say: ''availability '' , self availability printString'.

"What the spec measures"
wsp class compile: 'fill: name
	"Sends of 1 MiB to a server that does not read, until the buffer is full (at most 256 MiB):
	 how many went, the largest bufferedAmount, and the answer that refused one; then whether the
	 write semaphore is signalled once the server reads again (at most 30 s)"
	| id chunk answer sends largest |
	id := (self sock: name) first.
	chunk := ByteArray new: 1048576.
	sends := largest := 0.
	[ (answer := self send: id bytes: chunk isText: false) isInteger and: [ sends < 256 ] ]
		whileTrue: [ sends := sends + 1. largest := largest max: answer ].
	((self sock: name) at: 4) consumeAllSignals.
	^ self say: sends printString , ''|'' , largest printString , ''|'' , answer printString , ''|''
		, (((self sock: name) at: 4) waitTimeoutMilliseconds: 30000) not printString'.
wsp class compile: 'closeTimed: name
	"Close it: its state once over, and how long it took, in ms"
	| t |
	t := Time millisecondClockValue.
	self close: (self sock: name) first code: 1000 reason: nil.
	self waitFor: name until: [ :st :n | st >= 4 ].
	^ self say: (Time millisecondClockValue - t) printString , ''|'' , (''|'' join: (self lines: name))'.
wsp class compile: 'firstBytesOf: name
	"The UTF-8 bytes of the next message, the first 4"
	| m |
	m := self next: name.
	^ self say: ((m isString ifTrue: [ m utf8Encoded ] ifFalse: [ m ]) first: (4 min: m size)) printString'.
wsp class compile: 'flood: name
	"Read every message, printing each one: how many, and how it ended"
	| count m |
	count := 0.
	[ (m := self waitNext: name) notNil ] whileTrue: [
		count := count + 1.
		Stdio stdout nextPutAll: count printString; nextPut: $ ; nextPutAll: (self said: m); lf; flush ].
	^ self say: count printString , ''|'' , (''|'' join: ((self lines: name) first: 2))'.
wsp class compile: 'fork: name
	"A process that waits for the next message, which WSGot then holds"
	Smalltalk at: #WSGot put: nil.
	[ Smalltalk at: #WSGot put: (self waitNext: name) ] forkAt: Processor userSchedulingPriority.
	^ self say: ''forked'''.
wsp class compile: 'got
	^ self say: ''got '' , (Smalltalk at: #WSGot) printString'.

"The world"
wsp class compile: 'morph: name
	"A morph of the world that a reader process updates with each message, deferred to the UI process"
	| morph |
	morph := StringMorph contents: ''none''.
	morph openInWorld.
	Smalltalk at: #WSMorph put: morph.
	[ | m |
	  [ (m := self waitNext: name) notNil ] whileTrue: [ UIManager default defer: [ morph contents: (self said: m) ] ].
	  UIManager default defer: [ morph contents: ''closed '' , (self st: name) printString ] ] forkAt: Processor userSchedulingPriority.
	^ self say: ''morph'''.
wsp class compile: 'morphText
	^ self say: (Smalltalk at: #WSMorph) contents'.
wsp class compile: 'debuggers
	^ World systemWindows select: [ :each |
		(each model respondsTo: #presenter) and: [ each model presenter isKindOf: StDebugger ] ]'.
wsp class compile: 'debugger
	"How many, and whether the interrupted process of the first one waits in waitNext:"
	| windows |
	windows := self debuggers.
	^ self say: ''debuggers '' , windows size printString , (windows
		ifEmpty: [ '''' ]
		ifNotEmpty: [ '' '' , (windows first model presenter session interruptedContext stack
			anySatisfy: [ :each | each selector = #waitNext: ]) printString ])'.
wsp class compile: 'closeDebuggers
	self debuggers do: [ :each | each delete ].
	^ self debugger'.

"The sockets of the SocketPlugin (Emscripten's, to a websockify-style server)"
wsp class compile: 'socket: port
	| socket |
	^ self say: ''socket '' , ([
		socket := Socket newTCP.
		socket connectTo: #[127 0 0 1] port: port waitForConnectionFor: 5.
		Smalltalk at: #WSSocket put: socket.
		socket isConnected printString ] on: Error do: [ :error | error class name asString ])'.
wsp class compile: 'socketSend
	"Whether the socket of before takes data"
	^ self say: ''send '' , ([ (Smalltalk at: #WSSocket) sendData: ''x''. ''sent'' ] on: Error do: [ :error | error class name asString ])'.
wsp say: 'ready'
