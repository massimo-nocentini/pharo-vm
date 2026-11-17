/* tsCoroSwitch.h -- the private interface between the resumable sort
 * (tsCoro.c: the table of sorts, their handles, the sort side) and the
 * stacks it runs on, one implementation per target, each in its own file of
 * src/coro that compiles to nothing elsewhere:
 *
 *   tsCoroPosix.c   Linux and macOS: the stacks are mmap'ed with a guard
 *                   page, the switch is hand-written assembly,
 *   tsCoroX64.c       x86-64 System V (ELF and Mach-O spelling),
 *   tsCoroA64.c       AArch64 (ELF with BTI landing pads; Mach-O);
 *   tsCoroFibers.c  Windows x64 and arm64: Fibers.
 *
 * Not a header of include/: only src/coro and tests/corotest.c include it.
 * No VM header.
 *
 * A stack runs a loop: main(stack) runs one sort, then the stack yields
 * to its caller and is parked until the next resume, which starts the next
 * sort given to it (tsCoro.c keeps a finished stack for the next sort,
 * TS_CORO_STACK_CACHE).  Every resume saves the caller's context afresh,
 * so a stack may be resumed from any depth of the caller's C stack. */
#ifndef TS_CORO_SWITCH_H
#define TS_CORO_SWITCH_H
#include <stddef.h>
#include <stdint.h>
#include "tsCoro.h"

#if TS_CORO_SUPPORTED

/* AddressSanitizer must be told of every switch of stacks
 * (__sanitizer_start_switch_fiber, __sanitizer_finish_switch_fiber),
 * else it takes the other stack for an overflow of this one */
#if defined(__SANITIZE_ADDRESS__)
# define TS_CORO_ASAN 1
#elif defined(__has_feature)
# if __has_feature(address_sanitizer)
#  define TS_CORO_ASAN 1
# endif
#endif
#ifndef TS_CORO_ASAN
# define TS_CORO_ASAN 0
#endif

typedef struct ts_stack ts_stack;

struct ts_stack {
	void (*main)(ts_stack *);	/* runs the sort of job (tsCoro.c) */
	void *job;			/* the sort to run at the next start (a ts_coro) */
	int started;			/* resumed at least once: main's loop runs */
	int quit;			/* ts_stack_delete: leave the loop for good */
#if defined(_WIN32)
	void *fiber;			/* the fiber of the stack (CreateFiberEx) */
	void *caller;			/* the fiber that resumed it last */
#else
	void *sp;			/* the stack pointer of the parked side, */
	void *callerSp;			/* of the caller, saved by every resume */
	void *map;			/* the mapping: the guard page, then the stack */
	size_t mapSize;
	unsigned char *low;		/* the stack: low .. low + TS_CORO_STACK_SIZE */
# if TS_CORO_ASAN
	const void *callerBottom;	/* the caller's stack, as ASan knows it */
	size_t callerSize;
# endif
#endif
};

/* 1 when stacks can be switched here, 0 when not: Linux, when the process
 * runs with a shadow stack (x86-64 CET, AArch64 GCS), which a hand-written
 * switch would break; Windows, when the thread could not be made a fiber.
 * With prepare (ts_coro_open), Windows makes the calling thread a fiber
 * unless it is one; without (ts_coro_available) nothing changes. */
TS_INTERNAL int ts_stack_setup(int prepare);

/* The name of the switch (ts_coro_impl) */
TS_INTERNAL const char *ts_stack_impl(void);

/* A new stack whose loop calls main; NULL when out of memory */
TS_INTERNAL ts_stack *ts_stack_new(void (*main)(ts_stack *));

/* Frees a stack that is parked between two sorts, or was never resumed */
TS_INTERNAL void ts_stack_delete(ts_stack *s);

/* The caller side: runs the stack until it yields (its context saved,
 * whatever the depth of the call) */
TS_INTERNAL void ts_stack_resume(ts_stack *s);

/* The stack side: back to the caller of the last resume, until the next */
TS_INTERNAL void ts_stack_yield(ts_stack *s);

/* The bytes of the stack s ever written (its high-water mark, from the
 * lowest non-zero word of the mapping, which starts zeroed), 0 where this
 * cannot be seen (fibers): for tests/corotest.c */
TS_INTERNAL size_t ts_stack_used(const ts_stack *s);

/* The high-water mark of every stack the sorts of this copy hold or keep
 * (ts_stack_used of each), for tests/corotest.c (tsCoro.c) */
TS_INTERNAL size_t ts_coro_stack_high_water(void);

#if !defined(_WIN32)
/* The hand-written switch (tsCoroX64.c, tsCoroA64.c).  ts_coro_swap pushes
 * the callee-saved registers on the current stack, stores the stack
 * pointer in *save, loads load as the stack pointer, pops the registers
 * saved there and returns into that side.  ts_coro_frame lays out, below
 * top (16-byte aligned), the frame a first ts_coro_swap pops: it returns
 * into a trampoline that calls entry(arg) with a zero frame pointer (the
 * end of the chain for unwinders); entry never returns.  Answers the
 * stack pointer to load. */
TS_INTERNAL void ts_coro_swap(void **save, void *load);
TS_INTERNAL void *ts_coro_frame(void *top, void (*entry)(void *), void *arg);
TS_INTERNAL const char *ts_coro_switch_name(void);
#endif

#endif /* TS_CORO_SUPPORTED */
#endif
