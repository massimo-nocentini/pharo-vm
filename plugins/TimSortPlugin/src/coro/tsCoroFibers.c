/* tsCoroFibers.c -- the stacks of the resumable sort on Windows x64 and
 * arm64 (tsCoroSwitch.h): Windows Fibers.  Compiles to nothing elsewhere.
 *
 * Why Fibers and not a hand-written switch as on Linux and macOS:
 * SwitchToFiber runs in user mode (no system call) and switches, besides
 * the registers of the ABI (on x64 also rdi, rsi and xmm6-xmm15), the
 * stack base, limit and deallocation stack of the thread's TEB and its
 * fiber-local storage, which structured exception handling, the stack
 * probes of __chkstk and the growth of the stack through its guard page
 * read; it also follows the shadow stack of hardware-enforced stack
 * protection (CET).
 *
 * A stack is a fiber (CreateFiberEx: TS_CORO_STACK_SIZE bytes reserved,
 * with the guard page Windows puts below a growing stack; the floating
 * point state switched too).  A switch needs the calling thread to be a
 * fiber: ts_stack_setup makes it one with ConvertThreadToFiberEx unless it
 * is one already (IsThreadAFiber: a thread that the VM, or a library, made
 * a fiber is never converted twice), and never converts it back (a thread
 * that is a fiber runs exactly as before).  Every resume takes the caller's
 * fiber afresh (GetCurrentFiber). */

#if defined(_WIN32) && !defined(_WIN32_WINNT)
# define _WIN32_WINNT 0x0600	/* IsThreadAFiber (CMakeLists.txt defines the same) */
#endif

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#include "tsCoroSwitch.h"

#if TS_CORO_SUPPORTED && defined(_WIN32)

#include <windows.h>

int
ts_stack_setup(int prepare)
{
	static int failed;	/* ConvertThreadToFiberEx failed once */

	if (IsThreadAFiber()) {
		return 1;
	}
	if (!prepare || failed) {
		return !failed;
	}
	if (!ConvertThreadToFiberEx(NULL, FIBER_FLAG_FLOAT_SWITCH)) {
		failed = 1;
		return 0;
	}
	return 1;
}

const char *
ts_stack_impl(void)
{
	return "fibers";
}

/* The loop of a stack: the fiber's start routine, never left */
static VOID CALLBACK
stackEntry(LPVOID arg)
{
	ts_stack *s = arg;

	for (;;) {
		s->main(s);
		ts_stack_yield(s);
	}
}

ts_stack *
ts_stack_new(void (*main)(ts_stack *))
{
	ts_stack *s = calloc(1, sizeof *s);

	if (!s) {
		return NULL;
	}
	s->main = main;
	s->fiber = CreateFiberEx(0, TS_CORO_STACK_SIZE, FIBER_FLAG_FLOAT_SWITCH, stackEntry, s);
	if (!s->fiber) {
		free(s);
		return NULL;
	}
	return s;
}

void
ts_stack_delete(ts_stack *s)
{
	if (!s) {
		return;
	}
	DeleteFiber(s->fiber);	/* parked, or never started: nothing of it runs */
	free(s);
}

void
ts_stack_resume(ts_stack *s)
{
	s->started = 1;
	s->caller = GetCurrentFiber();
	SwitchToFiber(s->fiber);
}

void
ts_stack_yield(ts_stack *s)
{
	SwitchToFiber(s->caller);
}

size_t
ts_stack_used(const ts_stack *s)
{
	(void)s;
	return 0;	/* not seen through a fiber */
}

#else

typedef int ts_coro_fibers_unused;	/* (ISO C wants a declaration in every file) */

#endif
