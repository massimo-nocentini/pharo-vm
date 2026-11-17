/* tsCoroA64.c -- the switch of stacks of the resumable sort on AArch64
 * (Linux: ELF; macOS: Mach-O, whose symbols take a leading underscore and
 * whose hidden symbols are .private_extern): ts_coro_swap and
 * ts_coro_frame of tsCoroSwitch.h.  Compiles to nothing on any other target
 * (Windows arm64 uses Fibers, tsCoroFibers.c).
 *
 * The callee-saved state of AAPCS64 (and of Apple's arm64 ABI, which
 * reserves x18: never touched here) is x19-x28, the frame pointer x29, the
 * link register x30 and the low halves d8-d15 of v8-v15; FPCR is left
 * alone (no comparison of a sort changes the rounding mode).
 * ts_coro_swap stores them in a 160-byte frame on the current stack,
 * stores sp, loads the other side's and loads the same frame there:
 *
 *   sp+0    x19 x20 x21 x22 x23 x24 x25 x26 x27 x28
 *   sp+80   x29 x30
 *   sp+96   d8 d9 d10 d11 d12 d13 d14 d15
 *
 * A new stack starts with that frame laid out by ts_coro_frame: x19 = arg,
 * x20 = entry, x29 = 0, x30 = ts_coro_start, which calls entry(arg) (blr)
 * and traps if it returns.
 *
 * Branch protection (-mbranch-protection=standard on Linux): both
 * functions begin with a BTI landing pad (bti c, spelled hint #34: a no-op
 * without BTI, so it assembles for any arm64 core), as the GNU property
 * note the compiler gives this file promises; entry is a C function whose
 * address is taken, so the compiler gives it its own.  Return addresses
 * signed with PAC (pac-ret) need nothing: each function signs and
 * authenticates its own return address with its own sp on its own stack,
 * and ts_coro_swap keeps x30 as the caller left it.  A guarded control
 * stack (GCS) cannot follow a ret into another stack: tsCoroPosix.c refuses
 * to switch when the process runs with one. */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "tsCoroSwitch.h"

#if TS_CORO_SUPPORTED && defined(__aarch64__) && !defined(_WIN32)

#if defined(__APPLE__)
# define TS_FUNCTION(name) \
	".globl _" #name "\n" \
	".private_extern _" #name "\n" \
	".p2align 4\n" \
	"_" #name ":\n"
# define TS_END(name) ""
#else
# define TS_FUNCTION(name) \
	".globl " #name "\n" \
	".hidden " #name "\n" \
	".type " #name ",%function\n" \
	".p2align 4\n" \
	#name ":\n"
# define TS_END(name) ".size " #name ",.-" #name "\n"
#endif

__asm__(
	".text\n"
	/* void ts_coro_swap(void **save (x0), void *load (x1)) */
	TS_FUNCTION(ts_coro_swap)
	"	hint #34\n"
	"	sub sp, sp, #160\n"
	"	stp x19, x20, [sp, #0]\n"
	"	stp x21, x22, [sp, #16]\n"
	"	stp x23, x24, [sp, #32]\n"
	"	stp x25, x26, [sp, #48]\n"
	"	stp x27, x28, [sp, #64]\n"
	"	stp x29, x30, [sp, #80]\n"
	"	stp d8, d9, [sp, #96]\n"
	"	stp d10, d11, [sp, #112]\n"
	"	stp d12, d13, [sp, #128]\n"
	"	stp d14, d15, [sp, #144]\n"
	"	mov x9, sp\n"
	"	str x9, [x0]\n"
	"	mov sp, x1\n"
	"	ldp x19, x20, [sp, #0]\n"
	"	ldp x21, x22, [sp, #16]\n"
	"	ldp x23, x24, [sp, #32]\n"
	"	ldp x25, x26, [sp, #48]\n"
	"	ldp x27, x28, [sp, #64]\n"
	"	ldp x29, x30, [sp, #80]\n"
	"	ldp d8, d9, [sp, #96]\n"
	"	ldp d10, d11, [sp, #112]\n"
	"	ldp d12, d13, [sp, #128]\n"
	"	ldp d14, d15, [sp, #144]\n"
	"	add sp, sp, #160\n"
	"	ret\n"
	TS_END(ts_coro_swap)
	/* the first return of a new stack: entry (x20) of arg (x19); the CFI
	 * marks the outermost frame of the stack for unwinders */
	TS_FUNCTION(ts_coro_start)
	"	.cfi_startproc\n"
	"	.cfi_undefined x30\n"
	"	hint #34\n"
	"	mov x0, x19\n"
	"	blr x20\n"
	"	brk #1\n"
	"	.cfi_endproc\n"
	TS_END(ts_coro_start));

TS_INTERNAL void ts_coro_start(void);

/* The slots of the frame, in words from the stack pointer */
enum { X19 = 0, X20, X29 = 10, X30, FRAME_WORDS = 20 };

void *
ts_coro_frame(void *top, void (*entry)(void *), void *arg)
{
	uintptr_t aligned = (uintptr_t)top & ~(uintptr_t)15;
	void **frame = (void **)(aligned - FRAME_WORDS * sizeof(void *));

	memset(frame, 0, FRAME_WORDS * sizeof(void *));
	frame[X19] = arg;
	frame[X20] = (void *)entry;
	frame[X29] = NULL;
	frame[X30] = (void *)ts_coro_start;
	return frame;
}

const char *
ts_coro_switch_name(void)
{
	return "asm-aarch64";
}

#else

typedef int ts_coro_a64_unused;	/* (ISO C wants a declaration in every file) */

#endif
