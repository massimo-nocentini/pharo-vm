/* sqPlatformSpecific.h -- Emscripten (WebAssembly) modifications to sq.h
 *
 * Emscripten is a Unix as far as the VM is concerned, so this header takes
 * everything from the Unix one; it comes first on the include path of the
 * Emscripten build.  What it adds is the interface of the return-to-host
 * runtime (src/emscripten/emscriptenMain.c and the synchronous heartbeat in
 * src/common/heartbeat.c) that the interpreter and the builtin plugins
 * compile against.
 *
 * A browser tab (or node's event loop) has to get its thread back regularly,
 * so the VM runs in slices: the host calls vm_resume(), the interpreter runs
 * until the slice is over or the image goes idle, and vm_resume() answers one
 * of the VM_* states below.  Without threads there is no heartbeat thread
 * either: the interpreter polls the heartbeat itself (PHARO_POLL_HEARTBEAT).
 */

#include "../unix/sqPlatformSpecific.h"

/* There are no threads: the VM runs in slices on the host's thread, and its
 * heartbeat is the synchronous one below.
 */
#if defined(__EMSCRIPTEN_PTHREADS__)
# error "The WebAssembly VM runs without threads (see docs/WebAssembly.md): build it without -pthread"
#endif

/* The heartbeat is ticked by the interpreter.  Slang expands pollHeartbeat()
 * just before the stack-limit check of every interpreted method activation,
 * full block activation and backward jump, so even a loop without sends
 * counts down to ioHeartbeatPoll() (src/common/heartbeat.c), which beats
 * every few milliseconds and ends the slice when it is over.
 */
extern int ioHeartbeatPollCountdown;
extern void ioHeartbeatPoll(void);
#define PHARO_POLL_HEARTBEAT() (--ioHeartbeatPollCountdown < 0 ? ioHeartbeatPoll() : (void)0)

/* The Smalltalk frames printed per process when the VM reports an error,
 * unless --maxFramesToLog says otherwise (0 prints them all, as elsewhere).
 * A crash deep in a recursion would otherwise print hundreds of megabytes,
 * which takes a WebAssembly VM minutes.
 */
#ifndef PHARO_DEFAULT_MAX_FRAMES_TO_LOG
# define PHARO_DEFAULT_MAX_FRAMES_TO_LOG 1000
#endif

/* The return-to-host driver (src/emscripten/emscriptenMain.c). */

/* Called last in ioSynchronousCheckForEvents: ends the slice when asked to. */
extern void ioReturnToHostIfRequested(void);
/* Asks for the end of the slice once its deadline, nowMs, has passed. */
extern void emscriptenSliceCheck(double nowMs);
/* The image is idle: end the slice and resume it after usecs at the latest. */
extern void emscriptenRequestSleep(long long usecs);
/* The depth of callbacks into the image from host code; never return to the
 * host from inside one (always 0 while there is no FFI).
 */
extern int emscriptenCallbackDepth;

/* main() returns long before the VM exits, leaving the runtime alive, so a
 * later exit() does not exit the runtime; and Emscripten flushes the C
 * streams only when the runtime exits: output still in their buffers, such
 * as a last line without a newline, would be lost.  So the VM exits through
 * emscriptenExit(), which flushes them first, as a native exit() does.
 */
#include <stdlib.h>
extern void emscriptenExit(int status) __attribute__((noreturn));
#define exit(status) emscriptenExit(status)

/* The states vm_resume() and vm_state() answer. */
#define VM_RUNNING	0
#define VM_WAITING	1	/* host-side only: sleeping until there is input */
#define VM_BUSY		2	/* the slice is over; resume as soon as possible */
#define VM_EXITED	3
#define VM_SLEEPING	4	/* idle; resume after vm_wakeup_ms() or on input */

/* The kinds of the notifications the VM sends to Module.onPharoHost(). */
#define HOST_IMAGE_SAVED	1	/* an image file was written and closed */
