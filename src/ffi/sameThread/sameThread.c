#include "sigjmp_support.h"

#include "pThreadedFFI.h"
#include "vmCallback.h"
#include "pharovm/macros.h"

void sameThreadCallbackEnter(struct _Runner* runner, struct _CallbackInvocation* callback);
void sameThreadCallbackExit(struct _Runner* runner, struct _CallbackInvocation* callback);
void sameThreadPrepareCallback(struct _Runner* runner, struct _CallbackInvocation* callback);

static Runner sameThreadRunner = {
	sameThreadCallbackEnter,
	sameThreadCallbackExit,
	sameThreadPrepareCallback,
    NULL
};

Primitive(primitiveGetSameThreadRunnerAddress) {

	sqInt externalAddress;

	externalAddress = instantiateClassindexableSize(classExternalAddress(), sizeof(void*));
    checkFailed();

    writeAddress(externalAddress, &sameThreadRunner);
    checkFailed();

    primitiveEndReturn(externalAddress);
}

void sameThreadCallbackEnter(struct _Runner* runner, struct _CallbackInvocation* callback){

	VMCallbackContext *vmcc;

	vmcc = malloc(sizeof(VMCallbackContext));

	callback->payload = vmcc;

#if defined(__EMSCRIPTEN__)
	/* The callback runs a nested interpreter on top of the C (and the
	 * JavaScript) frames of the callout: the slice must not end, by a longjmp
	 * to the host, until it has returned.  The depth is set back to what it
	 * was, rather than decremented, on both returns, so that an inner
	 * callback whose frames a longjmp abandoned does not leave it above 0.
	 * (savedDepth is not modified after the sigsetjmp, so it survives the
	 * longjmp.)
	 */
	int savedDepth = emscriptenCallbackDepth;
#endif

	if ((!sigsetjmp(vmcc->trampoline, 0))) {
		//Used to mark that is a fake callback!
		vmcc->thunkp = NULL;
		vmcc->stackp = NULL;
		vmcc->intregargsp = NULL;
		vmcc->floatregargsp = NULL;
#if defined(__EMSCRIPTEN__)
		emscriptenCallbackDepth = savedDepth + 1;
#endif
		ptEnterInterpreterFromCallback(vmcc);
#if defined(__EMSCRIPTEN__)
		emscriptenCallbackDepth = savedDepth;
#endif
		fprintf(stderr,"Warning; callback failed to invoke\n");
		return;
	}
#if defined(__EMSCRIPTEN__)
	emscriptenCallbackDepth = savedDepth;
#endif

	free(vmcc);

}

void sameThreadCallbackExit(struct _Runner* runner, struct _CallbackInvocation* callback){

	VMCallbackContext *vmcc;
	vmcc = (VMCallbackContext*)callback->payload;

	ptExitInterpreterToCallback(vmcc);
}

void sameThreadPrepareCallback(struct _Runner* runner, struct _CallbackInvocation* callback){
	// I do not do nothing
}
