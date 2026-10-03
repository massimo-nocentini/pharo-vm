"S7 of wasm-smoke.sh: a priority 60 process woken by a Delay preempts a loop
 that sends nothing.  The loop's backward jump is the only place where the
 synchronous heartbeat can run, signal the timer and switch processes; without
 it the loop never ends.

	pharo --headless Pharo.image --no-default-preferences st --quit preempt.st

 prints true."

| done count |
done := false.
count := 0.
[ (Delay forMilliseconds: 100) wait.
  done := true ] forkAt: 60.
[ done ] whileFalse: [ count := count + 1 ].
Stdio stdout
	print: (done and: [ count > 0 ]);
	lf;
	flush.
