"Driver for ExtSemSoak: the foreign-thread signalling soak CLAUDE.md names as
the prerequisite for async DNS, the job pool, the fd watches and AsyncPlugin.

Build and install the plugin, then run this script:

	cd rust/plugins && cargo build --release -p ext-sem-soak
	install -m755 target/release/libExtSemSoak.so <vm directory>/
	<vm directory>/pharo --headless my.image \
		eval 'Smalltalk compiler evaluate: ''<abs path>/soak.st'' asFileReference contents'

`install`, never `cp`: writing into an inode the VM has mapped is SIGBUS.

`eval` rather than `st`: the `st` command-line handler hangs on a headless
Pharo 12.0 image before it runs a line of the script -- on the stock
files.pharo.org VM as well as on this one, so it is the image''s, not ours,
and `eval` compiling the same file is the way past it.

Environment variables tune the run, all optional:

	SOAK_SECONDS   throughput phase duration, default 60
	SOAK_THREADS   worker threads, default 8
	SOAK_INDICES   contiguous external-semaphore indices, default 8
	SOAK_MICROS    microseconds each worker pauses between signals, default 100
	SOAK_PINGS     wake-latency samples, default 200

SOAK_MICROS=0 is the starvation probe rather than the fastest soak: with no
pause the interpreter makes no progress at all, because `signalSemaphoreWithIndex`
calls `forceInterruptCheck` and `aioInterruptPoll` on every signal. The
plugin''s own module comment records that run. The default of 100 us over 8
threads is on the order of 40,000 signals a second -- the pause is a floor, not
a period, so the measured rate at 50 us was ~71,000/s rather than the 160,000/s
the arithmetic alone would suggest. Either is far more than any design in
CLAUDE.md would produce, and both leave the image responsive.

What it asserts, and why each matters, is in the plugin's own module comment.
The headline is (a): the plugin counts what it sent per index, the image counts
what it received per index, and the two must be equal. The request table counts
rather than flags, so equality is the property the design claims; nothing had
ever measured it at rate."

| threads indexCount seconds micros pings semaphores indices receipts processes
  hog totalSent totalReceived deadline out ok pingSema pingIndex all allIndices |

Object compile: 'soakStart: threadCount from: firstIndex count: indexCount every: micros
	<primitive: ''primitiveSoakStart'' module: ''ExtSemSoak''>
	^ self error: ''soakStart: failed. Either libExtSemSoak is not next to the VM, or an argument was out of range, or a run is already in flight.'''.
Object compile: 'soakStop
	<primitive: ''primitiveSoakStop'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.
Object compile: 'soakSentAt: index
	<primitive: ''primitiveSoakSentAt'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.
Object compile: 'soakSignalNanos: which
	<primitive: ''primitiveSoakSignalNanos'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.
Object compile: 'soakPing: index
	<primitive: ''primitiveSoakPing'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.
Object compile: 'soakAck: index
	<primitive: ''primitiveSoakAck'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.
Object compile: 'soakWakeNanos: which
	<primitive: ''primitiveSoakWakeNanos'' module: ''ExtSemSoak''>
	^ self primitiveFailed'.

threads := (Smalltalk os environment at: 'SOAK_THREADS' ifAbsent: [ '8' ]) asInteger.
indexCount := (Smalltalk os environment at: 'SOAK_INDICES' ifAbsent: [ '8' ]) asInteger.
seconds := (Smalltalk os environment at: 'SOAK_SECONDS' ifAbsent: [ '60' ]) asInteger.
micros := (Smalltalk os environment at: 'SOAK_MICROS' ifAbsent: [ '100' ]) asInteger.
pings := (Smalltalk os environment at: 'SOAK_PINGS' ifAbsent: [ '200' ]) asInteger.

out := [ :line | line traceCr ].

"--- register a contiguous block of external semaphores --------------------
Nothing guarantees the indices Smalltalk hands back are contiguous, so ask for
them and then check; the plugin signals a range, and a gap would send signals
to somebody else's semaphore.

One more than the soak needs. The last one is the wake-latency phase's, and it
is deliberately *outside* the range the workers signal: sharing a Semaphore
between the two phases means the throughput backlog leaves excess signals on
it, and a `wait` that returns before its ping was even sent pairs that ping
with the next ack and understates every latency after it."
all := (1 to: indexCount + 1) collect: [ :i | Semaphore new ].
allIndices := all collect: [ :sema | Smalltalk registerExternalObject: sema ].
(allIndices asSortedCollection asArray = allIndices asArray
	"Parenthesised: Smalltalk binary operators are left-to-right, so
	 `a - b + 1 = n + 1` would compare first and then add 1 to a Boolean."
	and: [ allIndices last - allIndices first + 1 = (indexCount + 1) ])
		ifFalse: [ ^ self error: 'external semaphore indices are not contiguous: ', allIndices printString ].
semaphores := all first: indexCount.
indices := allIndices first: indexCount.
pingSema := all last.
pingIndex := allIndices last.

"--- one receipt counter per semaphore, each in its own Process ------------
`Semaphore>>wait` consumes one excess signal per wait, and
doSignalExternalSemaphores runs the image-side semaphore once per counted
request, so a slow Process here loses nothing -- it only falls behind."
receipts := Array new: indexCount withAll: 0.
processes := (1 to: indexCount) collect: [ :i |
	[ [ true ] whileTrue: [
		(semaphores at: i) wait.
		receipts at: i put: (receipts at: i) + 1 ] ] forkAt: Processor userInterruptPriority ].

"--- make the image GC-heavy for the duration ------------------------------
The point is to have the VM scavenging, and moving objects, while foreign
threads hammer the request table."
hog := [ [ true ] whileTrue: [
	(1 to: 200) collect: [ :i | Array new: 64 ].
	Processor yield ] ] forkAt: Processor userBackgroundPriority.

"--- (a) + (b): throughput, loss and contention ---------------------------"
out value: 'soak: ', threads printString, ' threads over indices ',
	indices first printString, '..', indices last printString,
	' for ', seconds printString, ' s, ', micros printString, ' us apart'.
nil soakStart: threads from: indices first count: indexCount every: micros.
deadline := Time millisecondClockValue + (seconds * 1000).
[ Time millisecondClockValue < deadline ] whileTrue: [ (Delay forMilliseconds: 200) wait ].
totalSent := nil soakStop.

"Let the receipt Processes drain what the VM thread has already delivered, and
then some: doSignalExternalSemaphores runs on the interrupt check, so the last
requests land a beat after the workers stop."
(Delay forSeconds: 2) wait.

totalReceived := receipts inject: 0 into: [ :a :b | a + b ].
ok := true.
1 to: indexCount do: [ :i | | sent got |
	sent := nil soakSentAt: (indices at: i).
	got := receipts at: i.
	sent = got ifFalse: [ ok := false ].
	out value: '  index ', (indices at: i) printString,
		' sent=', sent printString, ' received=', got printString,
		(sent = got ifTrue: [ '' ] ifFalse: [ '   *** LOST ', (sent - got) printString ]) ].

out value: 'total sent     = ', totalSent printString.
out value: 'total received = ', totalReceived printString.
out value: 'rate           = ', (totalSent // (seconds max: 1)) printString, ' signals/s'.
out value: 'signalSemaphoreWithIndex nanoseconds over ',
	(nil soakSignalNanos: 0) printString, ' samples (',
	(nil soakSignalNanos: 1) printString, ' dropped):'.
out value: '  mean=', (nil soakSignalNanos: 2) printString,
	' p50=', (nil soakSignalNanos: 3) printString,
	' p99=', (nil soakSignalNanos: 4) printString,
	' p99.9=', (nil soakSignalNanos: 5) printString,
	' max=', (nil soakSignalNanos: 6) printString.

"--- (d): wake latency, one send paired with one receipt ------------------
A ping is sent from a thread of its own and *this* Process waits for it, so
the measurement covers the whole path: spawn, requestMutex, forceInterruptCheck,
aioInterruptPoll, the VM's next interrupt check, doSignalExternalSemaphores,
and the scheduler.

`pingSema` is its own Semaphore on its own index, outside the range the workers
signalled, so nothing else can wake this loop and no leftover excess signal can
let a `wait` return before its ping was sent. The receipt Processes are stopped
anyway: the throughput phase is over and their scheduling is noise in a
measurement of tens of microseconds.

An earlier version reused `semaphores first` here and had a receipt Process
still waiting on it. That deadlocks: the ping's signal is as likely to wake the
receipt Process as this one, and the first time it does, this loop waits
forever."
processes do: [ :p | p terminate ].
1 to: pings do: [ :i |
	nil soakPing: pingIndex.
	pingSema wait.
	nil soakAck: pingIndex ].
out value: 'wake latency nanoseconds over ', (nil soakWakeNanos: 0) printString,
	' samples (', (nil soakWakeNanos: 1) printString, ' unpaired):'.
out value: '  mean=', (nil soakWakeNanos: 2) printString,
	' p50=', (nil soakWakeNanos: 3) printString,
	' p99=', (nil soakWakeNanos: 4) printString,
	' p99.9=', (nil soakWakeNanos: 5) printString,
	' max=', (nil soakWakeNanos: 6) printString.

"--- teardown -------------------------------------------------------------"
hog terminate.
all do: [ :sema | Smalltalk unregisterExternalObject: sema ].

out value: (ok
	ifTrue: [ 'RESULT: no lost signals' ]
	ifFalse: [ 'RESULT: SIGNALS WERE LOST -- see the per-index lines above' ]).
