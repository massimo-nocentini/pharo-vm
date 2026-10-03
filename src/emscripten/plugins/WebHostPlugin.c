/* WebHostPlugin.c -- services of the web host to the image (Emscripten only)
 *
 * A builtin plugin, written by hand in the form Slang generates, so that the
 * build's void primitive trampolines (cmake/emscripten/genPrimitiveTable.cmake)
 * apply to its exports table as they do to the generated plugins'.
 *
 * The StackVM has no user interrupt of its own, so Stop in the browser (or any
 * host calling vm_interrupt() between slices) signals an external semaphore
 * that the image registered here.  A process of the image that waits on it at
 * userInterruptPriority interrupts whatever is running.
 */

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/*  The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions */

#define true 1
#define false 0
#define null 0

#include <limits.h>
#include <emscripten.h>

#if !defined(SQUEAK_BUILTIN_PLUGIN)
# error "WebHostPlugin is a builtin plugin"
#endif


/*** Function Prototypes ***/
static const char *getModuleName(void);
static void primitiveIsAvailable(void);
static void primitiveSetInterruptSemaphore(void);
static sqInt setInterpreter(struct VirtualMachine *anInterpreter);


/*** Variables ***/
static struct VirtualMachine *interpreterProxy;
static const char *moduleName = "WebHostPlugin (i)";
static sqInt interruptSemaphoreIndex = 0;


/*	The module name is used to check, once a module is loaded, that it is the
	module that was asked for. */

static const char *
getModuleName(void)
{
	return moduleName;
}


/*	Answer true: the image runs on the web host. */

static void
primitiveIsAvailable(void)
{
	interpreterProxy->popthenPush(1, interpreterProxy->trueObject());
}


/*	Remember the index of the external semaphore that vm_interrupt() signals;
	0 forgets it.  Answer the receiver.  The index must be a SmallInteger
	that signalSemaphoreWithIndex() takes as it is: it narrows the index to
	an int, so a larger one would signal another semaphore. */

static void
primitiveSetInterruptSemaphore(void)
{
	sqInt semaphoreIndex;

	semaphoreIndex = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (semaphoreIndex < 0 || semaphoreIndex > INT_MAX) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	interruptSemaphoreIndex = semaphoreIndex;
	interpreterProxy->pop(1);
}

static sqInt
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR))
	 && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));
}


/*	Called by the host between slices: signal the interrupt semaphore, and
	answer whether there was one to signal (the image registered an index
	that the external semaphore table holds). */

EMSCRIPTEN_KEEPALIVE int
vm_interrupt(void)
{
	if (!interpreterProxy || !interruptSemaphoreIndex) {
		return false;
	}
	return interpreterProxy->signalSemaphoreWithIndex(interruptSemaphoreIndex) != 0;
}


static char _m[] = "WebHostPlugin";
void* WebHostPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "primitiveIsAvailable\000\377", (void*)primitiveIsAvailable},
	{(void*)_m, "primitiveSetInterruptSemaphore\000\377", (void*)primitiveSetInterruptSemaphore},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{NULL, NULL, NULL}
};
