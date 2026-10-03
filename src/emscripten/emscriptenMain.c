/* emscriptenMain.c -- the entry point of the Emscripten (WebAssembly) VM
 *
 * The VM must not keep its thread: a browser worker, or node's event loop,
 * needs it back for input, timers and output.  So the VM runs in slices and
 * returns to the host between them.
 *
 * main() only gets ready: it keeps the runtime alive, calls
 * Module.onVMStarted and returns.  From then on the host runs every slice
 * with vm_resume().  The first one starts the VM with vm_main(), the later
 * ones call interpret() again: the stack pages exist already, so interpret()
 * carries on from the state the interpreter saved.  A slice ends in
 * ioReturnToHostIfRequested(), which ioSynchronousCheckForEvents() calls last:
 * there the interpreter has saved its state in its globals and has nothing
 * left to do on the C stack but return, so the driver longjmps back to
 * vm_resume().  The forceInterruptCheck() before the longjmp makes the rest of
 * the event check, which the longjmp skipped, run first thing in the next
 * slice.
 *
 * vm_resume() answers how the slice ended:
 *  - VM_BUSY: its time was up (PHARO_WASM_SLICE_MS, see ioHeartbeatPoll() in
 *    heartbeat.c), or emscriptenRequestYield() asked for it; resume at once;
 *  - VM_SLEEPING: the image is idle; resume after vm_wakeup_ms() at the
 *    latest, or as soon as there is input for it;
 *  - VM_EXITED: interpret() returned.
 * When the VM exits, in any slice, exit() does not return to vm_resume() at
 * all: it unwinds to the host as an ExitStatus that carries the status
 * (emscriptenExit() below flushes the C streams first).
 *
 * The exports take and answer only int and double, so that no pointer has
 * to cross to JavaScript as a BigInt in wasm64.
 */

#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pharovm/pharo.h"
#include "pharovm/pharoClient.h"

#include <emscripten.h>
#include <emscripten/stack.h>

#ifndef PHARO_WASM_SLICE_MS
# define PHARO_WASM_SLICE_MS 20
#endif

extern char **environ;
extern void printAllStacks(void);
extern void emscriptenInstallFileAccessHandler(void);

int emscriptenCallbackDepth = 0;

static jmp_buf hostReturn;
static int inSlice = 0;
static int yieldRequest = 0;	/* 0, VM_BUSY or VM_SLEEPING */
static int state = VM_RUNNING;
static int started = 0;
static int vmMainCalled = 0;
static int vmArgc = 0;
static char **vmArgv = NULL;
static double sliceMs = PHARO_WASM_SLICE_MS;
static double sliceDeadline = 0;
static long long wakeupUsecs = 0;
static unsigned long slices = 0;
static unsigned long busySlices = 0;
static unsigned long sleepingSlices = 0;

static void
beginSlice(void)
{
	yieldRequest = 0;
	inSlice = 1;
	sliceDeadline = emscripten_get_now() + sliceMs;
}

static void
endSlice(void)
{
	inSlice = 0;
	state = yieldRequest;
	yieldRequest = 0;
	slices += 1;
	if (state == VM_SLEEPING)
		sleepingSlices += 1;
	else
		busySlices += 1;
}

#if !defined(NDEBUG)
/* Every slice must start at the same depth of the C stack.  If it does not,
 * something the longjmp unwound did not leave the stack as it found it.
 */
static void
checkStackDrift(void)
{
	static uintptr_t firstStackPointer = 0;
	uintptr_t stackPointer = emscripten_stack_get_current();

	if (!firstStackPointer)
		firstStackPointer = stackPointer;
	else if (stackPointer != firstStackPointer)
		fprintf(stderr, "STACK DRIFT: slice %lu starts at %p, the first one at %p\n",
			slices, (void *)stackPointer, (void *)firstStackPointer);
}
#else
# define checkStackDrift() ((void)0)
#endif

/* The end of a slice.  The interpreter calls this last in its event check,
 * so the longjmp skips nothing but returns; forceInterruptCheck() makes the
 * rest of the event check (external semaphores, timers, process switches)
 * happen at the first check of the next slice.
 */
void
ioReturnToHostIfRequested(void)
{
	if (inSlice && yieldRequest && emscriptenCallbackDepth == 0) {
		forceInterruptCheck();
		longjmp(hostReturn, yieldRequest);
	}
}

void
emscriptenSliceCheck(double nowMs)
{
	if (inSlice && !yieldRequest && nowMs >= sliceDeadline) {
		yieldRequest = VM_BUSY;
		forceInterruptCheck();
	}
}

void
emscriptenRequestSleep(long long usecs)
{
	if (!inSlice)
		return;
	wakeupUsecs = usecs;
	yieldRequest = VM_SLEEPING;
	forceInterruptCheck();
}

void
emscriptenRequestYield(void)
{
	if (inSlice && !yieldRequest) {
		yieldRequest = VM_BUSY;
		forceInterruptCheck();
	}
}

/* Every exit() of the VM, through the macro of sqPlatformSpecific.h: the
 * runtime stays alive, so nothing else flushes the C streams.
 */
#undef exit

void
emscriptenExit(int status)
{
	fflush(NULL);
	exit(status);
}

/* vm_main() returns only when the VM does not run at all, e.g. after
 * --version or when the image cannot be read.  Exit as returning from main()
 * would have, although main() has returned long ago.
 */
static void
exitFromVMMain(int status)
{
	fflush(NULL);
	emscripten_force_exit(status);
}

/* Run the next slice; answer how it ended.  Only the host may call this, and
 * never from inside a slice.
 */
EMSCRIPTEN_KEEPALIVE int
vm_resume(void)
{
	if (!started || inSlice || state == VM_EXITED)
		return state;
	if (setjmp(hostReturn) == 0) {
		checkStackDrift();
		beginSlice();
		if (!vmMainCalled) {
			vmMainCalled = 1;
			exitFromVMMain(vm_main(vmArgc, (const char **)vmArgv, (const char **)environ));
		}
		interpret();
		inSlice = 0;
		return state = VM_EXITED;
	}
	endSlice();
	return state;
}

EMSCRIPTEN_KEEPALIVE int
vm_state(void)
{
	return state;
}

EMSCRIPTEN_KEEPALIVE int
vm_started(void)
{
	return started;
}

/* How long a VM_SLEEPING VM may be left alone, in milliseconds. */
EMSCRIPTEN_KEEPALIVE double
vm_wakeup_ms(void)
{
	return wakeupUsecs / 1000.0;
}

EMSCRIPTEN_KEEPALIVE void
vm_set_slice_ms(double ms)
{
	if (ms > 0)
		sliceMs = ms;
}

EMSCRIPTEN_KEEPALIVE int
vm_signal_semaphore(int semaphoreIndex)
{
	return (int)signalSemaphoreWithIndex(semaphoreIndex);
}

/* For a post-mortem, e.g. after a trap: print the Smalltalk stacks. */
EMSCRIPTEN_KEEPALIVE int
vm_dump_stacks(void)
{
	if (slices == 0)
		return 0;
	printAllStacks();
	fflush(stdout);
	return 1;
}

EMSCRIPTEN_KEEPALIVE int
vm_stats(void)
{
	fprintf(stderr, "[pharo-wasm] slices=%lu busy=%lu sleeping=%lu\n",
		slices, busySlices, sleepingSlices);
	return (int)slices;
}

/* Not in a function that calls setjmp: clang cannot compile EM_ASM there. */
static void __attribute__((noinline))
notifyStarted(void)
{
	EM_ASM({
		if (Module['onVMStarted'])
			Module['onVMStarted']();
	});
}

int
main(int argc, char *argv[])
{
	char *sliceMsString;

	if ((sliceMsString = getenv("PHARO_WASM_SLICE_MS")) && atof(sliceMsString) > 0)
		sliceMs = atof(sliceMsString);
	emscriptenInstallFileAccessHandler();

	/* the VM reads its arguments long after main() has returned */
	vmArgc = argc;
	vmArgv = calloc(argc + 1, sizeof(char *));
	for (int i = 0; i < argc; i++)
		vmArgv[i] = strdup(argv[i]);

	started = 1;
	notifyStarted();
	emscripten_exit_with_live_runtime();
	return 0;
}
