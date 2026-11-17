/* tsCoroX64.c -- the switch of stacks of the resumable sort on x86-64
 * System V (Linux: ELF; macOS: Mach-O, whose symbols take a leading
 * underscore and whose hidden symbols are .private_extern): ts_coro_swap
 * and ts_coro_frame of tsCoroSwitch.h.  Compiles to nothing on any other
 * target (Windows x64 uses Fibers, tsCoroFibers.c).
 *
 * The callee-saved state of the ABI is rbx, rbp, r12-r15, the control bits
 * of MXCSR and the x87 control word; ts_coro_swap pushes the six registers
 * and the two control words (in one 8-byte slot) on the current stack,
 * stores rsp, loads the other side's and pops the same frame there:
 *
 *   rsp+0   MXCSR (4 bytes), x87 control word (2 bytes)
 *   rsp+8   r15, r14, r13, r12, rbx, rbp
 *   rsp+56  the return address
 *
 * A new stack starts with that frame laid out by ts_coro_frame: r12 = arg,
 * r13 = entry, rbp = 0, the control words of the creator, and a return
 * into ts_coro_start, which calls entry(arg) with rsp 16-byte aligned (the
 * ABI's alignment at a call) and traps if it returns.
 *
 * Indirect branch tracking (CET IBT, -fcf-protection): ts_coro_swap is
 * only called directly and ts_coro_start only entered by a ret, neither
 * needs an endbr64, but ts_coro_swap gets one when the code is built for
 * IBT.  A shadow stack (CET SHSTK) cannot follow a ret into another stack:
 * tsCoroPosix.c refuses to switch when the process runs with one. */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "tsCoroSwitch.h"

#if TS_CORO_SUPPORTED && defined(__x86_64__) && !defined(_WIN32)

#if defined(__APPLE__)
# define TS_SYMBOL(name) "_" #name
# define TS_FUNCTION(name) \
	".globl " TS_SYMBOL(name) "\n" \
	".private_extern " TS_SYMBOL(name) "\n" \
	".p2align 4\n" \
	TS_SYMBOL(name) ":\n"
# define TS_END(name) ""
#else
# define TS_SYMBOL(name) #name
# define TS_FUNCTION(name) \
	".globl " #name "\n" \
	".hidden " #name "\n" \
	".type " #name ",@function\n" \
	".p2align 4\n" \
	#name ":\n"
# define TS_END(name) ".size " #name ",.-" #name "\n"
#endif

#if defined(__CET__) && (__CET__ & 1)
# define TS_ENDBR "	endbr64\n"
#else
# define TS_ENDBR ""
#endif

__asm__(
	".text\n"
	/* void ts_coro_swap(void **save (rdi), void *load (rsi)) */
	TS_FUNCTION(ts_coro_swap)
	TS_ENDBR
	"	pushq %rbp\n"
	"	pushq %rbx\n"
	"	pushq %r12\n"
	"	pushq %r13\n"
	"	pushq %r14\n"
	"	pushq %r15\n"
	"	subq $8, %rsp\n"
	"	stmxcsr (%rsp)\n"
	"	fnstcw 4(%rsp)\n"
	"	movq %rsp, (%rdi)\n"
	"	movq %rsi, %rsp\n"
	"	ldmxcsr (%rsp)\n"
	"	fldcw 4(%rsp)\n"
	"	addq $8, %rsp\n"
	"	popq %r15\n"
	"	popq %r14\n"
	"	popq %r13\n"
	"	popq %r12\n"
	"	popq %rbx\n"
	"	popq %rbp\n"
	"	ret\n"
	TS_END(ts_coro_swap)
	/* the first return of a new stack: entry (r13) of arg (r12); the CFI
	 * marks the outermost frame of the stack for unwinders */
	TS_FUNCTION(ts_coro_start)
	"	.cfi_startproc\n"
	"	.cfi_undefined %rip\n"
	"	movq %r12, %rdi\n"
	"	callq *%r13\n"
	"	ud2\n"
	"	.cfi_endproc\n"
	TS_END(ts_coro_start));

TS_INTERNAL void ts_coro_start(void);

/* The slots of the frame, in words from the stack pointer */
enum { CONTROL = 0, R15, R14, R13, R12, RBX, RBP, RETURN, FRAME_WORDS };

void *
ts_coro_frame(void *top, void (*entry)(void *), void *arg)
{
	uintptr_t aligned = (uintptr_t)top & ~(uintptr_t)15;
	void **frame = (void **)(aligned - FRAME_WORDS * sizeof(void *));
	uint32_t mxcsr;
	uint16_t fcw;

	memset(frame, 0, FRAME_WORDS * sizeof(void *));
	__asm__ volatile ("stmxcsr %0" : "=m" (mxcsr));
	__asm__ volatile ("fnstcw %0" : "=m" (fcw));
	memcpy((char *)&frame[CONTROL], &mxcsr, 4);
	memcpy((char *)&frame[CONTROL] + 4, &fcw, 2);
	frame[R12] = arg;
	frame[R13] = (void *)entry;
	frame[RETURN] = (void *)ts_coro_start;
	return frame;
}

const char *
ts_coro_switch_name(void)
{
	return "asm-x86_64-sysv";
}

#else

typedef int ts_coro_x64_unused;	/* (ISO C wants a declaration in every file) */

#endif
