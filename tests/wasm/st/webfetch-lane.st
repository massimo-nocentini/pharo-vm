"Lane 81-webfetch: the primitives of the WebFetchPlugin, without the package WebFetch.

	pharo --headless Pharo.image --no-default-preferences st --quit webfetch-lane.st

 The file webfetch.lane of the working directory holds the URL of a running
 tests/wasm/lib/fetch-http.mjs, a blank, and the probe to run:

	codes        what each primitive answers, and the code it fails with on each bad argument,
	             64 requests open and one more, what makes a request or an answer fail, manual
	             redirects, the window, and 150 times the index of a semaphore that is reused
	callback     a request started, looked at and closed inside an FFI callback, where primitiveMayWait
	             answers false
	memory       60 MB in one object, the old space that it costs, and 150 MB, which grows the memory
	nomemory     a request body that JavaScript cannot copy (nomemory.cjs of the lane)
	unavailable  a JavaScript without fetch
	off          PHARO_WASM_NETWORK=0, for fetch and, when the VM has libgit2, for git
	missing      a VM without the plugin (a native one)
	exit         three requests open at Smalltalk exit: 3
	trap         three requests open at a trap of the VM

 It prints 'ok <check>' or 'not ok <check>: got <value>, want <value>' for each check, and at the end

	webfetch-lane <probe>: <n> passed, <n> failed

 (nothing but the lines of a check for exit and trap, which end the VM).  The class WFP, which it
 compiles, has the primitives: a failed one answers {its error code}."

| base probe out passed failed check wfp sem index head start open buffer id ids |
probe := 'webfetch.lane' asFileReference contents trimBoth substrings.
base := probe first.
probe := probe second.
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
			out value: 'not ok ' , label , ': got ' , (got printString truncateTo: 200) , ', want ' , want printString ] ].

"The primitives, and what the probes do with them"
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
wfp class compile: 'mayWait
	<primitive: #primitiveMayWait module: #WebHostPlugin error: ec>
	^ { ec }'.
wfp class compile: 'get: url semaphore: s window: w timeout: t
	^ self
		start: (Smalltalk registerExternalObject: s)
		head: (''GET'' , String lf , url) utf8Encoded
		body: nil window: w timeout: t flags: 0'.
wfp class compile: 'wait: id on: s until: aBlock
	"aBlock has the state and the bytes that are ready"
	[ | state | (state := self state: id) isArray or: [ aBlock value: (state bitAnd: 7) value: state >> 3 ] ]
		whileFalse: [ s wait ]'.
wfp class compile: 'bytes: id
	| bytes |
	bytes := ByteArray new: (self state: id) >> 3.
	self read: id into: bytes at: 1 count: bytes size.
	^ bytes'.
wfp class compile: 'end: id on: s
	"The kind of the error, or the first line of the answer"
	| line |
	self wait: id on: s until: [ :state :size | state >= 3 ].
	line := (self head: id) utf8Decoded lines first.
	self close: id.
	^ line'.
wfp class compile: 'whole: url window: w timeout: t
	"The bytes of the body, or the kind and the message of the error"
	| s id answer |
	s := Semaphore new.
	id := self get: url semaphore: s window: w timeout: t.
	id isArray ifTrue: [ ^ id ].
	self wait: id on: s until: [ :state :size | state >= 3 or: [ size >= w ] ].
	answer := ((self state: id) bitAnd: 7) = 4
		ifTrue: [ (self head: id) utf8Decoded lines ]
		ifFalse: [ self bytes: id ].
	self close: id.
	Smalltalk unregisterExternalObject: s.
	^ answer'.

sem := Semaphore new.
index := Smalltalk registerExternalObject: sem.
head := ('GET' , String lf , base , '/hello') utf8Encoded.
start := [ :i :h :b :w :t :f | wfp start: i head: h body: b window: w timeout: t flags: f ].

probe = 'codes' ifTrue: [
	check value: 'availability' value: [ wfp availability ] value: 1.
	check value: 'mayWait' value: [ wfp mayWait ] value: true.
	"start"
	{ 'index nil' -> { nil. head. nil. 100. 0. 0 }.
	  'index 0' -> { 0. head. nil. 100. 0. 0 }.
	  'index -1' -> { -1. head. nil. 100. 0. 0 }.
	  'index past the table' -> { Smalltalk vm maxExternalSemaphores + 1. head. nil. 100. 0. 0 }.
	  'index + 2^32' -> { (1 << 32) + index. head. nil. 100. 0. 0 }.
	  'index 2^64' -> { 1 << 64. head. nil. 100. 0. 0 }.
	  'head a String' -> { index. head asString. nil. 100. 0. 0 }.
	  'head nil' -> { index. nil. nil. 100. 0. 0 }.
	  'head a CompiledMethod' -> { index. Object >> #yourself. nil. 100. 0. 0 }.
	  'head 3' -> { index. 3. nil. 100. 0. 0 }.
	  'head empty' -> { index. #[  ]. nil. 100. 0. 0 }.
	  'head of 1 MiB + 1' -> { index. ByteArray new: 1048577. nil. 100. 0. 0 }.
	  'body a String' -> { index. head. 'x'. 100. 0. 0 }.
	  'body 3' -> { index. head. 3. 100. 0. 0 }.
	  'window 0' -> { index. head. nil. 0. 0. 0 }.
	  'window 2^31' -> { index. head. nil. 1 << 31. 0. 0 }.
	  'window 1.0' -> { index. head. nil. 1.0. 0. 0 }.
	  'timeout -1' -> { index. head. nil. 100. -1. 0 }.
	  'timeout 2^31' -> { index. head. nil. 100. 1 << 31. 0 }.
	  'flags 2' -> { index. head. nil. 100. 0. 2 }.
	  'flags nil' -> { index. head. nil. 100. 0. nil } } do: [ :each |
		check value: 'start, ' , each key value: [ start valueWithArguments: each value ] value: { #'bad argument' } ].
	check value: 'start, the largest window and timeout' value: [
		id := start valueWithArguments: { index. head. nil. (1 << 31) - 1. (1 << 31) - 1. 1 }.
		id isInteger ] value: true.
	"state"
	{ 'of a String' -> 'x'. 'of 2^64' -> (1 << 64) } do: [ :each |
		check value: 'state ' , each key value: [ wfp state: each value ] value: { #'bad argument' } ].
	{ 'of -1' -> -1. 'of 0' -> 0. 'of id + 2^32' -> (id + (1 << 32)) } do: [ :each |
		check value: 'state ' , each key value: [ wfp state: each value ] value: { #'not found' } ].
	check value: 'head of nil' value: [ wfp head: nil ] value: { #'bad argument' }.
	check value: 'head of an unknown id' value: [ wfp head: 0 ] value: { #'not found' }.
	check value: 'state 1 at first' value: [ wfp state: id ] value: 1.
	check value: 'no head in state 1' value: [ wfp head: id ] value: nil.
	buffer := ByteArray new: 10.
	check value: 'nothing to read in state 1' value: [ wfp read: id into: buffer at: 1 count: 10 ] value: 0.
	wfp wait: id on: sem until: [ :state :size | state >= 3 ].
	check value: 'state 3 and 5 bytes' value: [ wfp state: id ] value: 3 + (8 * 5).
	check value: 'the record of the answer' value: [ (wfp head: id) utf8Decoded lines first: 3 ] value: { '200'. 'OK'. base , '/hello' }.
	check value: 'not redirected' value: [ (wfp head: id) utf8Decoded lines at: 5 ] value: '0'.
	check value: 'a header in Latin-1' value: [ (wfp head: id) utf8Decoded lines includes: 'x-a: caf' , (String with: (Character value: 233)) ] value: true.
	"read"
	check value: 'read of nil' value: [ wfp read: nil into: buffer at: 1 count: 10 ] value: { #'bad argument' }.
	check value: 'read of an unknown id' value: [ wfp read: 0 into: buffer at: 1 count: 10 ] value: { #'not found' }.
	{ 'a String' -> (String new: 10). 'nil' -> nil. 'a CompiledMethod' -> (Object >> #yourself) } do: [ :each |
		check value: 'read into ' , each key value: [ wfp read: id into: each value at: 1 count: 10 ] value: { #'bad argument' } ].
	check value: 'read at nil' value: [ wfp read: id into: buffer at: nil count: 1 ] value: { #'bad argument' }.
	{ 'at 0' -> #( 0 1 ). 'count -1' -> #( 1 -1 ). 'over the end' -> #( 2 10 ).
	  'of huge counts' -> { SmallInteger maxVal. SmallInteger maxVal } } do: [ :each |
		check value: 'read ' , each key value: [ wfp read: id into: buffer at: each value first count: each value second ] value: { #'bad index' } ].
	check value: 'read into a read-only object' value: [
		wfp read: id into: ((ByteArray new: 10) beReadOnlyObject; yourself) at: 1 count: 10 ] value: { #'no modification' }.
	check value: 'read nothing, at the end' value: [ wfp read: id into: buffer at: 11 count: 0 ] value: 0.
	check value: 'read 2' value: [ (wfp read: id into: buffer at: 1 count: 2) -> (wfp state: id) ] value: 2 -> (3 + (8 * 3)).
	check value: 'read the rest' value: [ (wfp read: id into: buffer at: 3 count: 8) -> buffer asString ]
		value: 3 -> ('hello' , (String new: 5 withAll: (Character value: 0))).
	"close"
	check value: 'close' value: [ wfp close: id ] value: true.
	check value: 'close again' value: [ wfp close: id ] value: false.
	check value: 'close nil' value: [ wfp close: nil ] value: { #'bad argument' }.
	check value: 'closed is not found' value: [ wfp state: id ] value: { #'not found' }.
	check value: 'a forwarded head' value: [
		| forwarded |
		forwarded := ByteArray new: 3.
		forwarded becomeForward: head copy.
		id := wfp start: index head: forwarded body: nil window: 100 timeout: 0 flags: 0.
		id isInteger ] value: true.
	wfp close: id.
	"the number of open requests"
	ids := (1 to: 64) collect: [ :i |
		wfp start: index head: ('GET' , String lf , base , '/hang') utf8Encoded body: nil window: 10 timeout: 0 flags: 0 ].
	check value: '64 ids, all different' value: [
		(ids allSatisfy: [ :each | each isInteger and: [ each between: 1 and: 16r7FFFFFFF ] ]) -> ids asSet size ] value: true -> 64.
	check value: 'the 65th' value: [ wfp start: index head: head body: nil window: 10 timeout: 0 flags: 0 ] value: { #'resource limit exceeded' }.
	check value: 'all closed' value: [ (ids collect: [ :each | wfp close: each ]) asSet asArray ] value: { true }.
	"requests"
	check value: 'a body' value: [ (wfp whole: base , '/hello' window: 100 timeout: 0) asString ] value: 'hello'.
	check value: 'no body, 204' value: [ wfp whole: base , '/204' window: 100 timeout: 0 ] value: #[  ].
	check value: 'a body of 0 bytes' value: [ wfp whole: base , '/n?n=0' window: 100 timeout: 0 ] value: #[  ].
	check value: 'gzip' value: [ (wfp whole: base , '/gzip' window: 30000 timeout: 0) size ] value: 20000.
	check value: 'a redirect' value: [ (wfp whole: base , '/redir' window: 100 timeout: 0) asString ] value: 'hello'.
	check value: 'a window of one more than the body: it ends' value: [ (wfp whole: base , '/n?n=60000' window: 60001 timeout: 0) size ] value: 60000.
	check value: 'a window that is full' value: [
		| s |
		s := Semaphore new.
		id := wfp get: base , '/endless' semaphore: s window: 100000 timeout: 0.
		wfp wait: id on: s until: [ :state :size | size >= 100000 ].
		(Delay forMilliseconds: 200) wait.
		{ (wfp state: id) bitAnd: 7. (wfp state: id) >> 3 < 1000000. wfp close: id } ] value: { 2. true. true }.
	#( 'ftp://x/' 'data:,x' 'hello' 'http://a:b@127.0.0.1/' ) do: [ :each |
		check value: 'not a request: ' , each value: [ (wfp whole: each window: 100 timeout: 0) first ] value: 'request' ].
	check value: 'a GET with a body' value: [
		| s |
		s := Semaphore new.
		wfp end: (wfp start: (Smalltalk registerExternalObject: s) head: head body: #[  ] window: 10 timeout: 0 flags: 0) on: s ] value: 'request'.
	check value: 'nobody listens' value: [ (wfp whole: 'http://127.0.0.1:1/' window: 100 timeout: 0) first ] value: 'network'.
	check value: 'a body that breaks off' value: [ (wfp whole: base , '/trunc' window: 1000000 timeout: 0) first ] value: 'body'.
	check value: 'a timeout' value: [ (wfp whole: base , '/hang' window: 100 timeout: 300) first ] value: 'timeout'.
	check value: 'a timeout in the body' value: [ (wfp whole: base , '/endless' window: 1000000000 timeout: 200) first ] value: 'timeout'.
	check value: 'manual redirect' value: [
		| s |
		s := Semaphore new.
		wfp end: (wfp start: (Smalltalk registerExternalObject: s) head: ('GET' , String lf , base , '/redir') utf8Encoded
			body: nil window: 100 timeout: 0 flags: 1) on: s ] value: '302'.
	check value: 'a POST, its body and its headers' value: [
		| s answer |
		s := Semaphore new.
		id := wfp
			start: (Smalltalk registerExternalObject: s)
			head: (String lf join: { 'post'. base , '/echo?post'. 'X-Mine: caf' , (String with: (Character value: 233)). 'Cookie: no'. 'Sec-X: no'.
				'Referer: no'. 'User-Agent: mine'. 'Content-Type: a/b' }) utf8Encoded
			body: (ByteArray new: 1000000 withAll: 65)
			window: 10000 timeout: 0 flags: 0.
		wfp wait: id on: s until: [ :state :size | state >= 3 ].
		answer := (wfp bytes: id) asString.
		wfp close: id.
		answer ] value: 'POST|/echo?post|a/b||mine|1000000|' , (String new: 60 withAll: $A).
	check value: 'the headers that the server saw' value: [
		| headers |
		headers := ((STONJSON fromString: (wfp whole: base , '/_stats' window: 10000000 timeout: 0) utf8Decoded) at: 'requests')
			detect: [ :each | (each at: 'url') = '/echo?post' ].
		headers := headers at: 'headers'.
		{ headers at: 'x-mine'. (headers includesKey: 'cookie'). (headers includesKey: 'sec-x'). (headers includesKey: 'referer') } ]
		value: { 'caf' , (String with: (Character value: 233)). false. false. false }.
	"the rule of the semaphores: close, unregister, and the index is another's"
	check value: '150 reuses of an index: 0 spurious signals' value: [
		| spurious same |
		spurious := same := 0.
		1 to: 150 do: [ :i |
			| first slot second |
			first := Semaphore new.
			slot := Smalltalk registerExternalObject: first.
			id := wfp
				start: slot
				head: ('GET' , String lf , base , (#( '/dribble?ms=5' '/hello' '/n?n=300000' '/hang' ) at: i \\ 4 + 1)) utf8Encoded
				body: nil window: 100 timeout: (i \\ 4 = 3 ifTrue: [ i \\ 9 + 1 ] ifFalse: [ 0 ]) flags: 0.
			(Delay forMilliseconds: i \\ 11) wait.
			wfp close: id.
			Smalltalk unregisterExternalObject: first.
			second := Semaphore new.
			(Smalltalk registerExternalObject: second) = slot ifTrue: [ same := same + 1 ].
			"true: the time was over"
			(second waitTimeoutMilliseconds: 12) ifFalse: [ spurious := spurious + 1 ].
			Smalltalk unregisterExternalObject: second ].
		{ spurious. same } ] value: { 0. 150 } ].

probe = 'callback' ifTrue: [
	((FFILibrary << #WebFetchLaneLibC) package: 'WebFetchLane') install.
	(Smalltalk at: #WebFetchLaneLibC) compile: 'unixLibraryName
	^ ''libc.so.6'''.
	wfp class compile: 'qsort: base count: n size: s with: c
	^ self ffiCall: #(void qsort(ByteArray base, size_t n, size_t s, FFICallback c)) library: WebFetchLaneLibC'.
	ids := OrderedCollection new.
	"one that is done, to read in the callback"
	id := wfp get: base , '/hello' semaphore: sem window: 100 timeout: 0.
	wfp wait: id on: sem until: [ :state :size | state >= 3 ].
	wfp qsort: (ByteArray new: 8) count: 2 size: 4 with: (FFICallback
		signature: #( int #( void * a #, void * b ) )
		block: [ :a :b |
			ids add: wfp mayWait.
			ids add: (wfp get: base , '/hello' semaphore: sem window: 100 timeout: 0).
			ids add: (wfp state: ids last).
			ids add: (wfp head: id) utf8Decoded lines first.
			ids add: (wfp bytes: id) asString.
			ids add: (wfp close: id).
			0 ]).
	check value: 'in the callback: start and state' value: [ { ids first. ids second isInteger. ids third } ] value: { false. true. 1 }.
	check value: 'in the callback: head, read and close' value: [ ids asArray copyFrom: 4 to: 6 ] value: { '200'. 'hello'. true }.
	check value: 'after it' value: [ wfp mayWait ] value: true.
	check value: 'the request goes on' value: [
		wfp wait: ids second on: sem until: [ :state :size | state >= 3 ].
		(wfp bytes: ids second) asString ] value: 'hello'.
	wfp close: ids second ].

probe = 'memory' ifTrue: [
	| before body |
	Smalltalk garbageCollect.
	before := Smalltalk vm parameterAt: 1.
	body := wfp whole: base , '/n?n=60000000' window: 64000001 timeout: 0.
	check value: 'one copy of 60 MB at the peak' value: [ (Smalltalk vm parameterAt: 1) - before < 100000000 ] value: true.
	check value: 'all of it, in order' value: [
		| bad |
		bad := 0.
		1 to: body size do: [ :i | (body at: i) = (i - 1 bitAnd: 255) ifFalse: [ bad := bad + 1 ] ].
		{ body size. bad } ] value: { 60000000. 0 }.
	Smalltalk garbageCollect.
	check value: 'one copy of it after the garbage collection' value: [ (Smalltalk vm parameterAt: 1) - before < 100000000 ] value: true.
	check value: 'the memory grows: 150 MB more' value: [
		| more |
		more := wfp whole: base , '/n?n=150000000' window: 2000000000 timeout: 0.
		{ more size. (1 to: more size by: 777) allSatisfy: [ :i | (more at: i) = (i - 1 bitAnd: 255) ]. body size } ]
		value: { 150000000. true. 60000000 } ].

probe = 'nomemory' ifTrue: [
	check value: 'a body that cannot be copied' value: [
		wfp start: index head: ('POST' , String lf , base , '/echo') utf8Encoded body: (ByteArray new: 77777) window: 10 timeout: 0 flags: 0 ]
		value: { #'insufficient C memory' }.
	check value: 'a smaller one' value: [ (wfp start: index head: ('POST' , String lf , base , '/echo') utf8Encoded body: (ByteArray new: 7) window: 10 timeout: 0 flags: 0) isInteger ]
		value: true ].

probe = 'unavailable' ifTrue: [
	check value: 'availability' value: [ wfp availability ] value: 0.
	check value: 'start' value: [ wfp start: index head: head body: nil window: 10 timeout: 0 flags: 0 ] value: { #'unsupported operation' }.
	check value: 'close' value: [ wfp close: 1 ] value: false ].

probe = 'off' ifTrue: [
	check value: 'availability' value: [ wfp availability ] value: 2.
	check value: 'start' value: [ wfp start: index head: head body: nil window: 10 timeout: 0 flags: 0 ] value: { #'unsupported operation' }.
	(OSEnvironment current at: 'WEBFETCH_LANE_GIT' ifAbsent: [ '' ]) = 'yes' ifTrue: [
		check value: 'git' value: [
			[ IceRepositoryCreator new
				remote: (IceGitRemote url: base , '/git/x.git');
				location: Smalltalk imageDirectory / 'webfetch-git';
				createRepository ]
				on: Error
				do: [ :error | error messageText lines first ] ] value: 'git http: network access is off in Settings' ] ].

probe = 'missing' ifTrue: [
	check value: 'availability' value: [ wfp availability ] value: { #'not found' }.
	check value: 'start' value: [ wfp start: index head: head body: nil window: 10 timeout: 0 flags: 0 ] value: { #'not found' }.
	check value: 'mayWait' value: [ wfp mayWait ] value: { #'not found' } ].

(#( 'exit' 'trap' ) includes: probe) ifTrue: [
	(wfp whole: base , '/hello' window: 100 timeout: 0) asString = 'hello' ifFalse: [ out value: 'not ok no server' ].
	open := #( '/hang' '/endless' '/hang' ) collect: [ :each |
		wfp get: base , each semaphore: Semaphore new window: 100000 timeout: 60000 ].
	(Delay forMilliseconds: 300) wait.
	out value: 'ending with ' , (open allSatisfy: [ :each | (wfp state: each) isInteger ]) printString.
	probe = 'exit' ifTrue: [ Smalltalk exit: 3 ].
	(ExternalAddress fromAddress: 16r7FFFFFFF0000) unsignedLongAt: 1 put: 7.
	out value: 'not trapped' ].

out value: 'webfetch-lane ' , probe , ': ' , passed printString , ' passed, ' , failed printString , ' failed'.
