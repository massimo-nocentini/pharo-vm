"Lane 84-websocket: the primitives of the WebSocketPlugin, without the package WebFetch-WebSocket.

	pharo --headless Pharo.image --no-default-preferences st --quit websocket-lane.st

 The file websocket.lane of the working directory holds the URL of a running
 tests/wasm/lib/fetch-http.mjs, the port of its second listener (which never answers), and the
 probe to run, with blanks between them:

	codes        what each primitive answers, and the code it fails with on each bad argument, 32
	             sockets open and one more, the signals of the three semaphores, the messages of
	             the endpoints of lib/ws-server.mjs, what fails a socket, back-pressure, the closes,
	             and 150 times the indexes of semaphores that are reused
	lingering    32 sockets to /ws/deaf, each closed and released: the 33rd open fails
	callback     a socket opened, looked at, used, closed and released inside an FFI callback,
	             where primitiveMayWait answers false
	memory       a message of 60 MB, and the old space that it costs
	unavailable  a JavaScript without WebSocket (before.cjs of the lane)
	off          PHARO_WASM_NETWORK=0
	missing      a VM without the plugin (a native one)
	weboff       in the web VM: the harness turns the network off while a socket is open (it does
	             what the lines 'harness: network off' and 'harness: network on' say), then on
	exit         three sockets open at Smalltalk exit: 3
	trap         three sockets open at a trap of the VM

 It prints 'ok <check>' or 'not ok <check>: got <value>, want <value>' for each check, and at the end

	websocket-lane <probe>: <n> passed, <n> failed

 (nothing but the lines of a check for exit and trap, which end the VM).  The class WSP, which it
 compiles, has the primitives: a failed one answers {its error code}.  A socket of the probe is
 {id. state semaphore. read semaphore. write semaphore}."

| base ws hangPort probe out passed failed check wsp sock other indexes ids index record tag cafe |
probe := 'websocket.lane' asFileReference contents trimBoth substrings.
base := probe first.
hangPort := probe second.
probe := probe third.
ws := 'ws' , (base allButFirst: 4).
"(no other characters than ASCII in this file)"
cafe := 'caf' , (String with: (Character value: 233)).
passed := failed := 0.
out := [ :text | Stdio stdout nextPutAll: text; lf; flush ].
check := [ :label :block :want |
	| got |
	got := [ block value ] on: Error do: [ :error | error class name , ' ' , error messageText asString ].
	got = want
		ifTrue: [
			passed := passed + 1.
			out value: 'ok ' , label ]
		ifFalse: [
			failed := failed + 1.
			out value: 'not ok ' , label , ': got ' , (got printString truncateTo: 300) , ', want ' , want printString ] ].

"The primitives, and what the probes do with them"
((Object << #WSP slots: {  }) package: 'WebSocketLane') install.
wsp := Smalltalk at: #WSP.
#( 'availability' 'primitiveWebSocketAvailability'
   'open: s read: r write: w record: b maxMessage: m maxQueued: q timeout: t' 'primitiveWebSocketOpen'
   'state: id' 'primitiveWebSocketState'
   'info: id' 'primitiveWebSocketInfo'
   'receive: id' 'primitiveWebSocketReceive'
   'drop: id' 'primitiveWebSocketDrop'
   'send: id bytes: b isText: t' 'primitiveWebSocketSend'
   'buffered: id' 'primitiveWebSocketBuffered'
   'close: id code: c reason: r' 'primitiveWebSocketClose'
   'release: id' 'primitiveWebSocketRelease' ) pairsDo: [ :pattern :primitive |
	wsp class compile: pattern , '
	<primitive: #' , primitive , ' module: #WebSocketPlugin error: ec>
	^ { ec }' ].
wsp class compile: 'mayWait
	<primitive: #primitiveMayWait module: #WebHostPlugin error: ec>
	^ { ec }'.
wsp class compile: 'fetchAvailability
	<primitive: #primitiveFetchAvailability module: #WebFetchPlugin error: ec>
	^ { ec }'.
"fetch, for the log of the server"
#( 'fetchStart: s head: h body: b window: w timeout: t flags: f' 'primitiveFetchStart'
   'fetchState: id' 'primitiveFetchState'
   'fetchRead: id into: b at: s count: n' 'primitiveFetchRead'
   'fetchClose: id' 'primitiveFetchClose' ) pairsDo: [ :pattern :primitive |
	wsp class compile: pattern , '
	<primitive: #' , primitive , ' module: #WebFetchPlugin error: ec>
	^ { ec }' ].
wsp class compile: 'get: url
	"The body of a GET, as a String"
	| s id bytes |
	s := Semaphore new.
	id := self fetchStart: (Smalltalk registerExternalObject: s) head: (''GET'' , String lf , url) utf8Encoded
		body: nil window: 100000000 timeout: 10000 flags: 2.
	[ ((self fetchState: id) bitAnd: 7) >= 3 ] whileFalse: [ s waitTimeoutMilliseconds: 50 ].
	bytes := ByteArray new: (self fetchState: id) >> 3.
	self fetchRead: id into: bytes at: 1 count: bytes size.
	self fetchClose: id.
	Smalltalk unregisterExternalObject: s.
	^ bytes utf8Decoded'.
wsp class compile: 'log: base tag: tag
	"The connections that the server logged whose URL has tag"
	^ (STONJSON fromString: (self get: base , ''/_stats?tag='' , tag)) at: ''ws'''.
wsp class compile: 'ended: base tag: tag
	"The first connection whose URL has tag, once it ended (at most 6 s)"
	| t entry |
	t := Time millisecondClockValue.
	[ entry := (self log: base tag: tag) detect: [ :each | each at: ''ended'' ] ifNone: [ nil ].
	  entry isNil and: [ Time millisecondClockValue - t < 6000 ] ] whileTrue: [ (Delay forMilliseconds: 50) wait ].
	^ entry'.
wsp class compile: 'got: base tag: tag
	"The code and the reason of the close frame that the server got"
	| entry |
	entry := self ended: base tag: tag.
	^ entry ifNil: [ #none ] ifNotNil: [ { entry at: ''code''. entry at: ''reason'' } ]'.
wsp class compile: 'open: url protocols: ps max: m queued: q timeout: t
	"A socket: {id. state. read. write}; or {error code}"
	| sems id |
	sems := { Semaphore new. Semaphore new. Semaphore new }.
	id := self
		open: (Smalltalk registerExternalObject: sems first)
		read: (Smalltalk registerExternalObject: sems second)
		write: (Smalltalk registerExternalObject: sems third)
		record: (String lf join: { url } , ps) utf8Encoded
		maxMessage: m maxQueued: q timeout: t.
	id isArray ifTrue: [
		sems do: [ :each | Smalltalk unregisterExternalObject: each ].
		^ id ].
	^ { id } , sems'.
wsp class compile: 'start: url
	^ self open: url protocols: #(  ) max: 4194304 queued: 16777216 timeout: 10000'.
wsp class compile: 'wait: sock until: aBlock
	"The state + 8 * the count once aBlock (of the state and the count) is true, at most 10 s.
	 It polls, and leaves the signals of the semaphores to be counted"
	| t st |
	t := Time millisecondClockValue.
	[ st := self state: sock first.
	  st isArray or: [ (aBlock value: (st bitAnd: 7) value: st >> 3) or: [ Time millisecondClockValue - t > 10000 ] ] ]
		whileFalse: [ (Delay forMilliseconds: 10) wait ].
	^ st'.
wsp class compile: 'st: sock
	| st |
	st := self state: sock first.
	^ st isArray ifTrue: [ st ] ifFalse: [ st bitAnd: 7 ]'.
wsp class compile: 'connect: url
	"A socket once open (or over)"
	| sock |
	sock := self start: url.
	sock size = 1 ifTrue: [ ^ sock ].
	self wait: sock until: [ :st :n | st >= 2 ].
	^ sock'.
wsp class compile: 'fields: sock
	"The parts of the info record"
	| info |
	info := self info: sock first.
	^ info isArray ifTrue: [ info ] ifFalse: [ (info utf8Decoded splitOn: Character lf) asArray ]'.
wsp class compile: 'take: sock
	"The next message: a String for text, a ByteArray for binary; nil"
	| m |
	m := self receive: sock first.
	m isNil ifTrue: [ ^ nil ].
	m size = 1 ifTrue: [ ^ m ].
	^ m first ifTrue: [ m second utf8Decoded ] ifFalse: [ m second ]'.
wsp class compile: 'next: sock
	"The next message, waiting for it (at most 10 s)"
	self wait: sock until: [ :st :n | n > 0 or: [ st >= 4 ] ].
	^ self take: sock'.
wsp class compile: 'all: sock
	| all m |
	all := OrderedCollection new.
	[ (m := self take: sock) notNil ] whileTrue: [ all add: m ].
	^ all asArray'.
wsp class compile: 'text: sock with: aString
	^ self send: sock first bytes: aString utf8Encoded isText: true'.
wsp class compile: 'free: sock
	"Release it, and unregister its semaphores"
	| was |
	was := self release: sock first.
	sock allButFirst do: [ :each | Smalltalk unregisterExternalObject: each ].
	^ was'.
wsp class compile: 'signals: sock
	"The excess signals of its semaphores, consumed"
	^ sock allButFirst collect: [ :each | | n | n := each instVarNamed: ''excessSignals''. each consumeAllSignals. n ]'.

index := Smalltalk registerExternalObject: Semaphore new.
record := (ws , '/ws/echo') utf8Encoded.

probe = 'codes' ifTrue: [
	check value: 'availability' value: [ wsp availability ] value: 1.
	check value: 'mayWait' value: [ wsp mayWait ] value: true.
	"open: every bad argument"
	other := { Semaphore new. Semaphore new }.
	indexes := other collect: [ :each | Smalltalk registerExternalObject: each ].
	{ 'nil' -> nil. '0' -> 0. '-1' -> -1. 'past the table' -> (Smalltalk vm maxExternalSemaphores + 1).
	  '+ 2^32' -> ((1 << 32) + index). '2^64' -> (1 << 64) } do: [ :each |
		#( 'state' 'read' 'write' ) doWithIndex: [ :which :i |
			| args |
			args := { index. indexes first. indexes second }.
			args at: i put: each value.
			check value: 'open, the ' , which , ' index ' , each key
				value: [ wsp open: args first read: args second write: args third record: record maxMessage: 10 maxQueued: 10 timeout: 0 ]
				value: { #'bad argument' } ] ].
	{ 'state and read' -> { index. index. indexes first }. 'state and write' -> { index. indexes first. index }.
	  'read and write' -> { index. indexes first. indexes first } } do: [ :each |
		check value: 'open, two indexes alike: ' , each key
			value: [ wsp open: each value first read: each value second write: each value third record: record maxMessage: 10 maxQueued: 10 timeout: 0 ]
			value: { #'bad argument' } ].
	{ 'a String' -> (ws , '/ws/echo'). 'nil' -> nil. 'empty' -> #[  ]. 'of 65537 bytes' -> (ByteArray new: 65537).
	  'a CompiledMethod' -> (Object >> #yourself) } do: [ :each |
		check value: 'open, the record ' , each key
			value: [ wsp open: index read: indexes first write: indexes second record: each value maxMessage: 10 maxQueued: 10 timeout: 0 ]
			value: { #'bad argument' } ].
	{ 'maxMessage 0' -> #( 0 10 0 ). 'maxMessage 2^28 + 1' -> { (1 << 28) + 1. 10. 0 }. 'maxMessage nil' -> #( nil 10 0 ).
	  'maxQueued 0' -> #( 10 0 0 ). 'maxQueued 2^28 + 1' -> { 10. (1 << 28) + 1. 0 }.
	  'timeout -1' -> #( 10 10 -1 ). 'timeout 2^31' -> { 10. 10. 1 << 31 }. 'timeout 1.0' -> #( 10 10 1.0 ) } do: [ :each |
		check value: 'open, ' , each key
			value: [ wsp open: index read: indexes first write: indexes second record: record
				maxMessage: each value first maxQueued: each value second timeout: each value third ]
			value: { #'bad argument' } ].
	check value: 'open, the largest values' value: [
		| id |
		id := wsp open: index read: indexes first write: indexes second record: record
			maxMessage: 1 << 28 maxQueued: 1 << 28 timeout: (1 << 31) - 1.
		{ id isInteger and: [ id between: 1 and: 16r7FFFFFFF ]. wsp release: id. wsp release: id } ] value: #( true true false ).
	other do: [ :each | Smalltalk unregisterExternalObject: each ].
	"the ids that are none"
	#( 'state' 'info' 'receive' 'drop' 'buffered' ) do: [ :name |
		check value: name , ' of nil' value: [ wsp perform: (name , ':') asSymbol with: nil ] value: { #'bad argument' }.
		check value: name , ' of a String' value: [ wsp perform: (name , ':') asSymbol with: '1' ] value: { #'bad argument' }.
		{ '0' -> 0. '-1' -> -1. '2^31' -> (1 << 31). 'an unknown id' -> 16r7FFFFFFE } do: [ :each |
			check value: name , ' of ' , each key value: [ wsp perform: (name , ':') asSymbol with: each value ] value: { #'not found' } ] ].
	check value: 'release of nil' value: [ wsp release: nil ] value: { #'bad argument' }.
	check value: 'release of 0, -1, 2^31' value: [ { wsp release: 0. wsp release: -1. wsp release: 1 << 31 } ] value: #( false false false ).
	check value: 'send to nil' value: [ wsp send: nil bytes: #[ 1 ] isText: false ] value: { #'bad argument' }.
	check value: 'send to an unknown id' value: [ wsp send: 16r7FFFFFFE bytes: #[ 1 ] isText: false ] value: { #'not found' }.
	check value: 'close of nil' value: [ wsp close: nil code: 1000 reason: nil ] value: { #'bad argument' }.
	check value: 'close of an unknown id' value: [ wsp close: 16r7FFFFFFE code: 1000 reason: nil ] value: { #'not found' }.
	"32 open and one more"
	ids := (1 to: 32) collect: [ :i | wsp start: ws , '/ws/echo?t=many' ].
	check value: '32 ids, all different' value: [
		(ids allSatisfy: [ :each | each size = 4 and: [ each first between: 1 and: 16r7FFFFFFF ] ]) -> (ids collect: [ :each | each first ]) asSet size ]
		value: true -> 32.
	check value: 'the 33rd' value: [ wsp start: ws , '/ws/echo?t=many' ] value: { #'resource limit exceeded' }.
	check value: 'all open' value: [ (ids collect: [ :each | (wsp wait: each until: [ :st :n | st >= 2 ]) bitAnd: 7 ]) asSet asArray ] value: #( 2 ).
	check value: 'all released' value: [ (ids collect: [ :each | wsp free: each ]) asSet asArray ] value: #( true ).
	check value: 'they linger until the server closes them' value: [
		| t s |
		t := Time millisecondClockValue.
		[ (s := wsp start: ws , '/ws/echo?t=many') size = 1 and: [ Time millisecondClockValue - t < 5000 ] ]
			whileTrue: [ (Delay forMilliseconds: 20) wait ].
		s size = 4 ifTrue: [ wsp free: s ].
		s size ] value: 4.
	(Delay forMilliseconds: 200) wait.
	"the open, the signals, the info record"
	sock := wsp start: ws , '/ws/echo?t=open'.
	check value: 'state 1 at first, no info' value: [ { wsp state: sock first. wsp info: sock first } ] value: #( 1 nil ).
	check value: 'send before the open' value: [ wsp text: sock with: 'early' ] value: { #'inappropriate operation' }.
	wsp wait: sock until: [ :st :n | st >= 2 ].
	check value: 'the open signals each semaphore once' value: [ wsp signals: sock ] value: #( 1 1 1 ).
	check value: 'the info record: no protocol, no extension, the URL' value: [ wsp fields: sock ] value: { ''. ''. ws , '/ws/echo?t=open' }.
	check value: 'echo of text: cafe' value: [ wsp text: sock with: cafe. wsp next: sock ] value: cafe.
	check value: 'Receive answers {isText. bytes}' value: [ wsp text: sock with: cafe. wsp wait: sock until: [ :st :n | n > 0 ]. wsp receive: sock first ]
		value: { true. cafe utf8Encoded }.
	check value: 'echo of binary' value: [ wsp send: sock first bytes: #[ 0 1 2 255 ] isText: false.
		wsp wait: sock until: [ :st :n | n > 0 ]. wsp receive: sock first ] value: { false. #[ 0 1 2 255 ] }.
	check value: 'echo of an empty text and an empty binary' value: [
		wsp text: sock with: ''.
		wsp send: sock first bytes: #[  ] isText: false.
		wsp wait: sock until: [ :st :n | n >= 2 ].
		{ wsp receive: sock first. wsp receive: sock first. wsp receive: sock first } ] value: { { true. #[  ] }. { false. #[  ] }. nil }.
	check value: 'echo of 1 MiB' value: [
		| bytes got |
		bytes := ByteArray new: 1048576.
		1 to: bytes size do: [ :i | bytes at: i put: i \\ 251 ].
		wsp send: sock first bytes: bytes isText: false.
		got := wsp next: sock.
		{ got size. got = bytes } ] value: { 1048576. true }.
	check value: 'buffered' value: [ wsp buffered: sock first ] value: 0.
	check value: 'drop of a queued message: its size, then tooLarge' value: [
		wsp text: sock with: 'droppable'.
		wsp wait: sock until: [ :st :n | n > 0 ].
		"(the write signal of the drain of the send first)"
		(Delay forMilliseconds: 100) wait.
		wsp signals: sock.
		{ wsp drop: sock first. wsp st: sock. (wsp fields: sock) first: 2. wsp drop: sock first } ]
		value: { 9. 5. { 'tooLarge'. 'a message of 9 bytes is larger than the image can hold' }. nil }.
	check value: 'the server got 4009' value: [ wsp got: base tag: 't=open' ] value: #( 4009 'message too large' ).
	check value: 'a failure signals each semaphore once' value: [ wsp signals: sock ] value: #( 1 1 1 ).
	wsp free: sock.
	"one message signals read, and only read"
	sock := wsp connect: ws , '/ws/push?n=2&interval=300&t=signals'.
	check value: 'a message signals read only' value: [
		wsp next: sock.
		wsp signals: sock.
		wsp next: sock.
		wsp signals: sock ] value: #( 0 1 0 ).
	wsp free: sock.
	"subprotocols, extensions, fragments, pushes, broadcast"
	sock := wsp open: ws , '/ws/echo?t=protocol' protocols: #( 'x' 'chat' ) max: 1000 queued: 10000 timeout: 10000.
	wsp wait: sock until: [ :st :n | st >= 2 ].
	check value: 'the subprotocol chosen' value: [ wsp fields: sock ] value: { 'chat'. ''. ws , '/ws/echo?t=protocol' }.
	wsp free: sock.
	sock := wsp connect: ws , '/ws/deflate?t=deflate'.
	check value: 'the extensions of /ws/deflate' value: [ ((wsp fields: sock) second beginsWith: 'permessage-deflate') -> (wsp text: sock with: 'zip zip zip zip'; next: sock) ]
		value: true -> 'zip zip zip zip'.
	wsp free: sock.
	sock := wsp connect: ws , '/ws/fragmented?parts=4'.
	check value: 'fragments joined' value: [ wsp next: sock ]
		value: 'fragmented:' , ((1 to: 4) inject: '' into: [ :all :i | all , ' ' , cafe , ' ' , (String with: (Character value: 16r2713)) ]).
	wsp free: sock.
	sock := wsp connect: ws , '/ws/fragmented?parts=3&type=binary'.
	check value: 'binary fragments joined' value: [ wsp next: sock ] value: ((0 to: 47) asByteArray).
	wsp free: sock.
	sock := wsp connect: ws , '/ws/push?n=100&close=1000&t=push'.
	check value: 'push of 100, in order, then a clean close' value: [
		wsp wait: sock until: [ :st :n | st >= 4 ].
		{ (wsp all: sock) = ((1 to: 100) collect: [ :i | i printString ]). (wsp fields: sock) allButFirst: 3 } ]
		value: { true. #( '1000' '' '1' ) }.
	wsp free: sock.
	sock := wsp connect: ws , '/ws/broadcast?room=lane'.
	other := wsp connect: ws , '/ws/broadcast?room=lane'.
	check value: 'broadcast between two sockets' value: [ wsp text: other with: 'to the room'. { wsp next: sock. wsp next: other } ]
		value: #( 'to the room' 'to the room' ).
	wsp free: sock; free: other.
	"closes of the server"
	sock := wsp connect: ws , '/ws/close?code=4321&reason=bye&after=100'.
	check value: 'a close of the server: 4321 bye, clean' value: [ wsp wait: sock until: [ :st :n | st >= 4 ]. { wsp st: sock. (wsp fields: sock) allButFirst: 3 } ]
		value: { 4. #( '4321' 'bye' '1' ) }.
	wsp free: sock.
	sock := wsp connect: ws , '/ws/close?abrupt=1&after=300'.
	wsp text: sock with: 'before the end'.
	check value: 'an abrupt end: 1006, not clean, the queue still read' value: [
		wsp wait: sock until: [ :st :n | st >= 4 ].
		{ wsp st: sock. ((wsp fields: sock) allButFirst: 3) first. ((wsp fields: sock) allButFirst: 3) last. wsp take: sock } ]
		value: #( 4 '1006' '0' 'before the end' ).
	wsp free: sock.
	"what fails a socket"
	sock := wsp connect: ws , '/ws/refuse?status=403'.
	check value: 'a refused handshake: network' value: [ { wsp st: sock. (wsp fields: sock) first } ] value: #( 5 'network' ).
	wsp free: sock.
	sock := wsp connect: 'ws://127.0.0.1:1/ws/echo'.
	check value: 'a port with nothing: network' value: [ { wsp st: sock. (wsp fields: sock) first } ] value: #( 5 'network' ).
	wsp free: sock.
	sock := wsp open: 'ws://127.0.0.1:' , hangPort , '/ws/hang' protocols: #(  ) max: 10 queued: 10 timeout: 500.
	check value: '/ws/hang with a timeout of 500 ms: timeout' value: [
		wsp wait: sock until: [ :st :n | st >= 2 ].
		{ wsp st: sock. (wsp fields: sock) first: 2 } ] value: { 5. #( 'timeout' 'not open after 500 ms' ) }.
	wsp free: sock.
	#( 'http://127.0.0.1/' 'ws://u:p@127.0.0.1/' 'ws://127.0.0.1/#f' ) do: [ :each |
		sock := wsp connect: each.
		check value: 'not a request: ' , each value: [ { wsp st: sock. (wsp fields: sock) first } ] value: #( 5 'request' ).
		wsp free: sock ].
	sock := wsp open: ws , '/ws/echo' protocols: #( 'a' 'A' ) max: 10 queued: 10 timeout: 0.
	check value: 'subprotocols alike ignoring case: request' value: [ wsp wait: sock until: [ :st :n | st >= 2 ]. (wsp fields: sock) first: 2 ]
		value: #( 'request' 'the subprotocol A is offered twice' ).
	wsp free: sock.
	sock := wsp open: ws , '/ws/echo?t=big' protocols: #(  ) max: 1000 queued: 100000 timeout: 10000.
	wsp wait: sock until: [ :st :n | st >= 2 ].
	check value: 'a message over maxMessage: tooLarge, the earlier one read, the server got 4009' value: [
		wsp text: sock with: 'small'.
		wsp wait: sock until: [ :st :n | n > 0 ].
		wsp text: sock with: (String new: 1001 withAll: $x).
		wsp wait: sock until: [ :st :n | st >= 4 ].
		{ wsp st: sock. (wsp fields: sock) first. wsp take: sock. wsp take: sock. wsp got: base tag: 't=big' } ]
		value: { 5. 'tooLarge'. 'small'. nil. #( 4009 'message too large' ) }.
	wsp free: sock.
	sock := wsp open: ws , '/ws/flood?n=100&size=100&t=flood' protocols: #(  ) max: 1000 queued: 1000 timeout: 10000.
	check value: 'a flood over maxQueued: overflow, the queue read, the server got 4008' value: [
		wsp wait: sock until: [ :st :n | st >= 4 ].
		{ wsp st: sock. (wsp fields: sock) first. (wsp all: sock) size. wsp got: base tag: 't=flood' } ]
		value: { 5. 'overflow'. 10. #( 4008 'too many unread messages' ) }.
	wsp free: sock.
	"back-pressure"
	sock := wsp connect: ws , '/ws/slow-read?resume=1500'.
	check value: '/ws/slow-read: sends until the buffer is full, a write signal, then it goes' value: [
		| chunk answer sends full |
		chunk := ByteArray new: 1048576.
		sends := 0.
		[ (answer := wsp send: sock first bytes: chunk isText: false) isInteger and: [ sends < 256 ] ]
			whileTrue: [ sends := sends + 1 ].
		full := answer.
		wsp signals: sock.
		{ full. sends >= 16. (sock at: 4) waitTimeoutMilliseconds: 20000. (wsp send: sock first bytes: chunk isText: false) isInteger } ]
		value: { { #'resource limit exceeded' }. true. false. true }.
	wsp free: sock.
	"send and close: bad arguments"
	sock := wsp connect: ws , '/ws/deaf?t=huge'.
	check value: 'send of 64 MiB' value: [ (wsp send: sock first bytes: (ByteArray new: 64 * 1048576) isText: false) isInteger ] value: true.
	wsp free: sock.
	sock := wsp connect: ws , '/ws/echo?t=close'.
	{ 'not UTF-8' -> { #[ 16rC3 16r28 ]. true }. 'a String' -> { 'text'. true }. 'isText 1' -> { #[ 1 ]. 1 }.
	  'a CompiledMethod' -> { Object >> #yourself. false }. 'over 64 MiB' -> { ByteArray new: 64 * 1048576 + 1. false } } do: [ :each |
		check value: 'send of ' , each key value: [ wsp send: sock first bytes: each value first isText: each value second ] value: { #'bad argument' } ].
	{ 'code 999' -> { 999. nil }. 'code 1001' -> { 1001. nil }. 'code 2999' -> { 2999. nil }. 'code 5000' -> { 5000. nil }.
	  'code nil' -> { nil. nil }. 'a reason of 124 bytes' -> { 1000. ByteArray new: 124 withAll: 65 }.
	  'a reason C3' -> { 1000. #[ 16rC3 ] }. 'a String reason' -> { 1000. 'bye' } } do: [ :each |
		check value: 'close, ' , each key value: [ wsp close: sock first code: each value first reason: each value second ] value: { #'bad argument' } ].
	check value: 'still open after them' value: [ wsp st: sock ] value: 2.
	check value: 'close 4999 with a reason of 123 bytes: true, then false' value: [
		{ wsp close: sock first code: 4999 reason: (ByteArray new: 123 withAll: 65). wsp close: sock first code: 1000 reason: nil. wsp st: sock } ]
		value: #( true false 3 ).
	check value: 'the server got 4999' value: [ (wsp got: base tag: 't=close') first ] value: 4999.
	check value: 'the closing handshake ends: 4, clean' value: [ wsp wait: sock until: [ :st :n | st >= 4 ]. (wsp fields: sock) allButFirst: 3 ]
		value: { '4999'. String new: 123 withAll: $A. '1' }.
	wsp free: sock.
	#( 1000 3000 ) do: [ :code |
		tag := 't=code' , code printString.
		sock := wsp connect: ws , '/ws/echo?' , tag.
		check value: 'close ' , code printString , ': true, and the server got it' value: [
			{ wsp close: sock first code: code reason: ('ciao, ' , cafe) utf8Encoded. wsp got: base tag: tag } ]
			value: { true. { code. 'ciao, ' , cafe } }.
		wsp free: sock ].
	check value: 'close of a released id' value: [ wsp close: sock first code: 1000 reason: nil ] value: { #'not found' }.
	sock := wsp connect: ws , '/ws/deaf?t=deaf'.
	check value: 'close against /ws/deaf: 4 in under 6 s, 1006, not clean' value: [
		| t |
		t := Time millisecondClockValue.
		wsp close: sock first code: 1000 reason: nil.
		wsp wait: sock until: [ :st :n | st >= 4 ].
		{ wsp st: sock. Time millisecondClockValue - t < 6000. (wsp fields: sock) allButFirst: 3 } ]
		value: #( 4 true #( '1006' 'closing handshake timed out' '0' ) ).
	wsp free: sock.
	sock := wsp connect: ws , '/ws/echo?t=release'.
	check value: 'release: true, then not found, and the server got 1000 released' value: [
		{ wsp free: sock. wsp state: sock first. wsp release: sock first. wsp got: base tag: 't=release' } ]
		value: { true. { #'not found' }. false. #( 1000 'released' ) }.
	"the rule of the semaphores: release, unregister, and the indexes are another's"
	check value: '150 reuses of the indexes: 0 spurious signals' value: [
		| spurious same |
		spurious := same := 0.
		1 to: 150 do: [ :i |
			| first slots second |
			first := wsp
				open: ws , (#( '/ws/push?n=5&interval=3' '/ws/echo' '/ws/flood?n=20&size=10' '/ws/close?after=4' ) at: i \\ 4 + 1)
				protocols: #(  ) max: 1000 queued: 100000 timeout: 0.
			i \\ 4 = 1 ifTrue: [ wsp text: first with: 'echo' ].
			(Delay forMilliseconds: i \\ 11) wait.
			slots := first allButFirst collect: [ :each | Smalltalk externalObjects identityIndexOf: each ].
			wsp free: first.
			second := { Semaphore new. Semaphore new. Semaphore new }.
			(second collect: [ :each | Smalltalk registerExternalObject: each ]) = slots ifTrue: [ same := same + 1 ].
			(Delay forMilliseconds: 12) wait.
			(second anySatisfy: [ :each | (each instVarNamed: 'excessSignals') > 0 ]) ifTrue: [ spurious := spurious + 1 ].
			second do: [ :each | Smalltalk unregisterExternalObject: each ] ].
		{ spurious. same } ] value: { 0. 150 } ].

probe = 'lingering' ifTrue: [
	ids := (1 to: 32) collect: [ :i |
		| s |
		s := wsp connect: ws , '/ws/deaf?t=linger'.
		wsp close: s first code: 1000 reason: nil.
		s ].
	check value: '32 sockets to /ws/deaf, closing' value: [ (ids collect: [ :each | wsp st: each ]) asSet asArray ] value: #( 3 ).
	check value: 'released' value: [ (ids collect: [ :each | wsp free: each ]) asSet asArray ] value: #( true ).
	check value: 'the 33rd: they linger' value: [ wsp start: ws , '/ws/echo' ] value: { #'resource limit exceeded' } ].

probe = 'callback' ifTrue: [
	| got |
	((FFILibrary << #WebSocketLaneLibC) package: 'WebSocketLane') install.
	(Smalltalk at: #WebSocketLaneLibC) compile: 'unixLibraryName
	^ ''libc.so.6'''.
	wsp class compile: 'qsort: base count: n size: s with: c
	^ self ffiCall: #(void qsort(ByteArray base, size_t n, size_t s, FFICallback c)) library: WebSocketLaneLibC'.
	got := OrderedCollection new.
	sock := wsp connect: ws , '/ws/echo?t=callback'.
	wsp text: sock with: 'before'.
	wsp wait: sock until: [ :st :n | n > 0 ].
	wsp qsort: (ByteArray new: 8) count: 2 size: 4 with: (FFICallback
		signature: #( int #( void * a #, void * b ) )
		block: [ :a :b |
			| fresh |
			got add: wsp mayWait.
			fresh := wsp start: ws , '/ws/echo?t=in-callback'.
			got add: (wsp state: fresh first).
			got add: (wsp take: sock).
			got add: (wsp text: sock with: 'in the callback') isInteger.
			got add: (wsp close: fresh first code: 1000 reason: nil).
			got add: (wsp free: fresh).
			0 ]).
	check value: 'in the callback: mayWait, open and state' value: [ got asArray first: 2 ] value: #( false 1 ).
	check value: 'in the callback: receive, send, close and release' value: [ got asArray allButFirst: 2 ] value: #( 'before' true true true ).
	check value: 'after it' value: [ wsp mayWait ] value: true.
	check value: 'the socket goes on' value: [ wsp next: sock ] value: 'in the callback'.
	wsp free: sock ].

probe = 'memory' ifTrue: [
	| before message |
	sock := wsp open: ws , '/ws/big?size=60000000' protocols: #(  ) max: 64000000 queued: 64000000 timeout: 10000.
	wsp wait: sock until: [ :st :n | n > 0 or: [ st >= 4 ] ].
	Smalltalk garbageCollect.
	before := Smalltalk vm parameterAt: 1.
	"what the package does: without the memory for it, collect garbage, then grow the memory by
	 the largest message that the socket takes"
	message := wsp take: sock.
	message = { #'insufficient object memory' } ifTrue: [
		Smalltalk garbageCollect.
		message := wsp take: sock ].
	message = { #'insufficient object memory' } ifTrue: [
		Smalltalk growMemoryByAtLeast: 64000000.
		message := wsp take: sock ].
	check value: 'one copy of 60 MB at the peak' value: [ (Smalltalk vm parameterAt: 1) - before < 100000000 ] value: true.
	check value: 'all of it, in order' value: [
		| bad |
		bad := 0.
		1 to: message size by: 997 do: [ :i | (message at: i) = (i - 1 bitAnd: 255) ifFalse: [ bad := bad + 1 ] ].
		{ message class. message size. bad } ] value: { ByteArray. 60000000. 0 }.
	Smalltalk garbageCollect.
	check value: 'one copy of it after the garbage collection' value: [ (Smalltalk vm parameterAt: 1) - before < 100000000 ] value: true.
	wsp free: sock ].

probe = 'unavailable' ifTrue: [
	check value: 'availability' value: [ wsp availability ] value: 0.
	check value: 'open' value: [ wsp start: ws , '/ws/echo?t=unavailable' ] value: { #'unsupported operation' }.
	check value: 'fetch is still there' value: [ wsp fetchAvailability ] value: 1 ].

probe = 'off' ifTrue: [
	check value: 'availability' value: [ wsp availability ] value: 2.
	check value: 'open' value: [ wsp start: ws , '/ws/echo?t=off-probe' ] value: { #'unsupported operation' } ].

probe = 'missing' ifTrue: [
	check value: 'availability' value: [ wsp availability ] value: { #'not found' }.
	check value: 'open' value: [ wsp start: ws , '/ws/echo' ] value: { #'not found' }.
	check value: 'state' value: [ wsp state: 1 ] value: { #'not found' } ].

probe = 'weboff' ifTrue: [
	sock := wsp connect: ws , '/ws/echo?t=wo1'.
	check value: 'open' value: [ wsp st: sock ] value: 2.
	wsp text: sock with: 'queued'.
	wsp wait: sock until: [ :st :n | n > 0 ].
	out value: 'harness: network off'.
	check value: 'the network off: 5 off' value: [ wsp wait: sock until: [ :st :n | st >= 4 ]. { wsp st: sock. (wsp fields: sock) first: 2 } ]
		value: { 5. #( 'off' 'network access is off' ) }.
	check value: 'the queued message is still read' value: [ wsp take: sock ] value: 'queued'.
	check value: 'a new open fails' value: [ { wsp availability. wsp start: ws , '/ws/echo?t=wo2' } ] value: { 2. { #'unsupported operation' } }.
	wsp free: sock.
	out value: 'harness: network on'.
	check value: 'on again' value: [
		| t |
		t := Time millisecondClockValue.
		[ wsp availability = 1 or: [ Time millisecondClockValue - t > 10000 ] ] whileFalse: [ (Delay forMilliseconds: 20) wait ].
		sock := wsp connect: ws , '/ws/echo?t=wo3'.
		{ wsp st: sock. wsp text: sock with: 'again'; next: sock } ] value: #( 2 'again' ).
	wsp free: sock.
	check value: 'the server got 1000 network access is off' value: [ wsp got: base tag: 't=wo1' ] value: #( 1000 'network access is off' ).
	check value: 'and nothing while off' value: [ wsp log: base tag: 't=wo2' ] value: #(  ) ].

(#( 'exit' 'trap' ) includes: probe) ifTrue: [
	ids := #( '/ws/echo?t=end' '/ws/push?n=1000&interval=50&close=none&t=end' '/ws/deaf?t=end' ) collect: [ :each | wsp connect: ws , each ].
	out value: 'ending with ' , (ids allSatisfy: [ :each | (wsp st: each) = 2 ]) printString.
	probe = 'exit' ifTrue: [ Smalltalk exit: 3 ].
	(ExternalAddress fromAddress: 16r7FFFFFFF0000) unsignedLongAt: 1 put: 7.
	out value: 'not trapped' ].

out value: 'websocket-lane ' , probe , ': ' , passed printString , ' passed, ' , failed printString , ' failed'.
