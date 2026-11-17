/* tsCoroPosix.c -- the stacks of the resumable sort on Linux and macOS
 * (tsCoroSwitch.h), over the hand-written switch of tsCoroX64.c or
 * tsCoroA64.c.  Compiles to nothing on Windows and where tsCoro.h says
 * TS_CORO_SUPPORTED is 0.
 *
 * A stack is an anonymous private mapping of one guard page (PROT_NONE:
 * an overflow faults at once instead of running into other memory) below
 * TS_CORO_STACK_SIZE bytes of stack.  Its first resume returns into
 * stackEntry (the frame of ts_coro_frame), whose loop runs the sorts given
 * to the stack, yielding to the caller between two of them;
 * ts_stack_delete of a stack that ran resumes it once more to leave the
 * loop (AddressSanitizer then forgets it), and unmaps it.
 *
 * Under AddressSanitizer every switch is announced
 * (__sanitizer_start_switch_fiber before, __sanitizer_finish_switch_fiber
 * after), and the shadow of a stack is cleared before it is unmapped. */

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "tsCoroSwitch.h"

#if TS_CORO_SUPPORTED && !defined(_WIN32)

#include <sys/mman.h>
#include <unistd.h>
#if defined(__linux__)
# include <sys/prctl.h>
# include <sys/syscall.h>
#endif
#if TS_CORO_ASAN
# include <sanitizer/asan_interface.h>
# include <sanitizer/common_interface_defs.h>
#endif

#if !defined(MAP_ANONYMOUS) && defined(MAP_ANON)
# define MAP_ANONYMOUS MAP_ANON
#endif
#ifdef MAP_STACK
# define TS_MAP_FLAGS (MAP_PRIVATE | MAP_ANONYMOUS | MAP_STACK)
#else
# define TS_MAP_FLAGS (MAP_PRIVATE | MAP_ANONYMOUS)
#endif

/* Whether the process runs with a shadow stack, which checks every ret
 * against the call that it returns from and so cannot follow the switch:
 * x86-64 CET (ARCH_SHSTK_STATUS of arch_prctl, Linux 6.6) and AArch64 GCS
 * (PR_GET_SHADOW_STACK_STATUS of prctl, Linux 6.13).  An older kernel
 * refuses the call: no shadow stack. */
static int
shadowStack(void)
{
#if defined(__linux__) && defined(__x86_64__)
	unsigned long features = 0;

	return syscall(SYS_arch_prctl, 0x5005 /* ARCH_SHSTK_STATUS */, &features) == 0
		&& (features & 1 /* ARCH_SHSTK_SHSTK */);
#elif defined(__linux__) && defined(__aarch64__)
	unsigned long status = 0;

	return prctl(74 /* PR_GET_SHADOW_STACK_STATUS */, &status, 0, 0, 0) == 0
		&& (status & 1 /* PR_SHADOW_STACK_ENABLE */);
#else
	return 0;
#endif
}

int
ts_stack_setup(int prepare)
{
	static int state;	/* 0 not asked yet, 1 can switch, -1 cannot */

	(void)prepare;
	if (state == 0) {
		state = shadowStack() ? -1 : 1;
	}
	return state > 0;
}

const char *
ts_stack_impl(void)
{
	return ts_coro_switch_name();
}

static size_t
pageSize(void)
{
	static size_t size;

	if (size == 0) {
		long page = sysconf(_SC_PAGESIZE);

		size = page > 0 ? (size_t)page : 4096;
	}
	return size;
}

/* The loop of a stack (the first ts_coro_swap into it returns into
 * ts_coro_start, which calls it) */
static void
stackEntry(void *arg)
{
	ts_stack *s = arg;

#if TS_CORO_ASAN
	__sanitizer_finish_switch_fiber(NULL, &s->callerBottom, &s->callerSize);
#endif
	while (!s->quit) {
		s->main(s);
		ts_stack_yield(s);
	}
	/* ts_stack_delete: back to the caller for good (a NULL fake stack
	 * tells AddressSanitizer that this stack is gone) */
#if TS_CORO_ASAN
	__sanitizer_start_switch_fiber(NULL, s->callerBottom, s->callerSize);
#endif
	ts_coro_swap(&s->sp, s->callerSp);
	abort();	/* never resumed */
}

ts_stack *
ts_stack_new(void (*main)(ts_stack *))
{
	size_t page = pageSize();
	ts_stack *s = calloc(1, sizeof *s);

	if (!s) {
		return NULL;
	}
	s->mapSize = page + ((TS_CORO_STACK_SIZE + page - 1) & ~(page - 1));
	s->map = mmap(NULL, s->mapSize, PROT_READ | PROT_WRITE, TS_MAP_FLAGS, -1, 0);
	if (s->map == MAP_FAILED) {
		free(s);
		return NULL;
	}
	if (mprotect(s->map, page, PROT_NONE) != 0) {
		munmap(s->map, s->mapSize);
		free(s);
		return NULL;
	}
	s->main = main;
	s->low = (unsigned char *)s->map + page;
	s->sp = ts_coro_frame(s->low + TS_CORO_STACK_SIZE, stackEntry, s);
	return s;
}

void
ts_stack_delete(ts_stack *s)
{
	if (!s) {
		return;
	}
	if (s->started) {
		s->quit = 1;
		ts_stack_resume(s);
	}
#if TS_CORO_ASAN
	ASAN_UNPOISON_MEMORY_REGION(s->low, TS_CORO_STACK_SIZE);
#endif
	munmap(s->map, s->mapSize);
	free(s);
}

void
ts_stack_resume(ts_stack *s)
{
#if TS_CORO_ASAN
	void *fake = NULL;

	__sanitizer_start_switch_fiber(&fake, s->low, TS_CORO_STACK_SIZE);
#endif
	s->started = 1;
	ts_coro_swap(&s->callerSp, s->sp);
#if TS_CORO_ASAN
	__sanitizer_finish_switch_fiber(fake, NULL, NULL);
#endif
}

void
ts_stack_yield(ts_stack *s)
{
#if TS_CORO_ASAN
	void *fake = NULL;

	__sanitizer_start_switch_fiber(&fake, s->callerBottom, s->callerSize);
#endif
	ts_coro_swap(&s->sp, s->callerSp);
#if TS_CORO_ASAN
	__sanitizer_finish_switch_fiber(fake, &s->callerBottom, &s->callerSize);
#endif
}

/* (reads the stack word by word: not under AddressSanitizer, whose shadow
 * of a parked stack still poisons the redzones of its frames) */
#if TS_CORO_ASAN
__attribute__((no_sanitize_address))
#endif
size_t
ts_stack_used(const ts_stack *s)
{
	const uint64_t *word = (const uint64_t *)s->low;
	const uint64_t *end = (const uint64_t *)(s->low + TS_CORO_STACK_SIZE);

	while (word < end && *word == 0) {
		word++;
	}
	return (size_t)((const unsigned char *)end - (const unsigned char *)word);
}

#else

typedef int ts_coro_posix_unused;	/* (ISO C wants a declaration in every file) */

#endif
