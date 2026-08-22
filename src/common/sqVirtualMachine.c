#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <setjmp.h>

#include "sq.h"

/*** Function prototypes ***/

/* Moved to a header so that the Rust port can read the same declarations
 * rather than restating them. */
#include "pharovm/common/interpreterProxyFunctions.h"


struct VirtualMachine *VM = NULL;

static sqInt majorVersion(void) {
	return VM_PROXY_MAJOR;
}

static sqInt minorVersion(void) {
	return VM_PROXY_MINOR;
}

#if !IMMUTABILITY
static sqInt isNonIntegerObject(sqInt objectPointer)
{
	return !isIntegerObject(objectPointer);
}
#endif


static sqInt
interceptFetchIntegerofObject(sqInt fieldIndex, sqInt objectPointer)
{
	if (fieldIndex == 0
	 && isCharacterObject(objectPointer))
		return characterValueOf(objectPointer);

	return fetchIntegerofObject(fieldIndex, objectPointer);
}

sqInt  fetchIntegerofObject(sqInt fieldIndex, sqInt objectPointer);
struct VirtualMachine* sqGetInterpreterProxy(void)
{
	if(VM) return VM;
	VM = (struct VirtualMachine *)calloc(1, sizeof(VirtualMachine));
	/* Initialize Function pointers */
	VM->majorVersion = majorVersion;
	VM->minorVersion = minorVersion;

	/* InterpreterProxy methodsFor: 'stack access' */
	VM->pop = pop;
	VM->popthenPush = popthenPush;
	VM->push = push;
	VM->pushBool = pushBool;
	VM->pushFloat = pushFloat;
	VM->pushInteger = pushInteger;
	VM->stackFloatValue = stackFloatValue;
	VM->stackIntegerValue = stackIntegerValue;
	VM->stackObjectValue = stackObjectValue;
	VM->stackValue = stackValue;

	/* InterpreterProxy methodsFor: 'object access' */
	VM->argumentCountOf = argumentCountOf;
	VM->arrayValueOf = arrayValueOf;
	VM->byteSizeOf = byteSizeOf;
	VM->fetchArrayofObject = fetchArrayofObject;
	VM->fetchClassOf = fetchClassOf;
	VM->fetchFloatofObject = fetchFloatofObject;
	VM->fetchIntegerofObject = interceptFetchIntegerofObject;
	VM->fetchPointerofObject = fetchPointerofObject;
	VM->obsoleteDontUseThisFetchWordofObject = obsoleteDontUseThisFetchWordofObject;
	VM->firstFixedField = firstFixedField;
	VM->firstIndexableField = firstIndexableField;
	VM->literalofMethod = literalofMethod;
	VM->literalCountOf = literalCountOf;
	VM->methodArgumentCount = methodArgumentCount;
	VM->methodPrimitiveIndex = methodPrimitiveIndex;
	VM->primitiveIndexOf = primitiveIndexOf;
	VM->primitiveMethod = primitiveMethod;
	VM->sizeOfSTArrayFromCPrimitive = sizeOfSTArrayFromCPrimitive;
	VM->slotSizeOf = slotSizeOf;
	VM->stObjectat = stObjectat;
	VM->stObjectatput = stObjectatput;
	VM->stSizeOf = stSizeOf;
	VM->storeIntegerofObjectwithValue = storeIntegerofObjectwithValue;
	VM->storePointerofObjectwithValue = storePointerofObjectwithValue;

	/* InterpreterProxy methodsFor: 'testing' */
	VM->isKindOf = isKindOf;
	VM->isMemberOf = isMemberOf;
	VM->isBytes = isBytes;
	VM->isFloatObject = isFloatObject;
	VM->isIndexable = isIndexable;
	VM->isIntegerObject = isIntegerObject;
	VM->isIntegerValue = isIntegerValue;
	VM->isPointers = isPointers;
	VM->isWeak = isWeak;
	VM->isWords = isWords;
	VM->isWordsOrBytes = isWordsOrBytes;

	/* InterpreterProxy methodsFor: 'converting' */
	VM->booleanValueOf = booleanValueOf;
	VM->checkedIntegerValueOf = checkedIntegerValueOf;
	VM->floatObjectOf = floatObjectOf;
	VM->floatValueOf = floatValueOf;
	VM->integerObjectOf = integerObjectOf;
	VM->integerValueOf = integerValueOf;
	VM->positive32BitIntegerFor = positive32BitIntegerFor;
	VM->positive32BitValueOf = positive32BitValueOf;

	/* InterpreterProxy methodsFor: 'special objects' */
	VM->falseObject = falseObject;
	VM->nilObject = nilObject;
	VM->trueObject = trueObject;
	
	/* InterpreterProxy methodsFor: 'special classes' */
	VM->classArray = classArray;
	VM->classBitmap = classBitmap;
	VM->classByteArray = classByteArray;
	VM->classCharacter = classCharacter;
	VM->classFloat = classFloat;
	VM->classLargePositiveInteger = classLargePositiveInteger;
	VM->classPoint = classPoint;
	VM->classSemaphore = classSemaphore;
	VM->classSmallInteger = classSmallInteger;
	VM->classString = classString;

	/* InterpreterProxy methodsFor: 'instance creation' */
	VM->clone = clone;
	VM->instantiateClassindexableSize = instantiateClassindexableSize;
	VM->makePointwithxValueyValue = makePointwithxValueyValue;
	VM->popRemappableOop = popRemappableOop;
	VM->pushRemappableOop = pushRemappableOop;

	/* InterpreterProxy methodsFor: 'other' */
	VM->becomewith = becomewith;
	VM->byteSwapped = byteSwapped;
	VM->failed = failed;
	VM->fullGC = fullGC;
	VM->primitiveFail = primitiveFail;
	VM->signalSemaphoreWithIndex = signalSemaphoreWithIndex;
	VM->success = success;
	VM->superclassOf = superclassOf;

#if VM_PROXY_MINOR <= 13 /* reused in 14 and above */

	VM->compilerHookVector= 0;
	VM->setCompilerInitialized= 0;

#endif

#if VM_PROXY_MINOR > 1

	/* InterpreterProxy methodsFor: 'BitBlt support' */
	VM->loadBitBltFrom = loadBitBltFrom;
	VM->copyBits = copyBits;
	VM->copyBitsFromtoat = copyBitsFromtoat;

#endif

#if VM_PROXY_MINOR > 2

	/* InterpreterProxy methodsFor: 'FFI support' */
	VM->classExternalAddress = classExternalAddress;
	VM->ioLoadModuleOfLength = ioLoadModuleOfLength;
	VM->ioLoadSymbolOfLengthFromModule = ioLoadSymbolOfLengthFromModule;
	VM->isInMemory = isInMemory;
	VM->signed32BitIntegerFor = signed32BitIntegerFor;
	VM->signed32BitValueOf = signed32BitValueOf;
	VM->includesBehaviorThatOf = includesBehaviorThatOf;
	VM->classLargeNegativeInteger = classLargeNegativeInteger;

#endif

#if VM_PROXY_MINOR > 3

	VM->ioLoadFunctionFrom = ioLoadFunctionFrom;
	VM->ioMicroMSecs = ioMicroMSecs;

#endif

#if VM_PROXY_MINOR > 4

	VM->positive64BitIntegerFor = positive64BitIntegerFor;
	VM->positive64BitValueOf = positive64BitValueOf;
	VM->signed64BitIntegerFor = signed64BitIntegerFor;
	VM->signed64BitValueOf = signed64BitValueOf;

#endif

#if VM_PROXY_MINOR > 5
	VM->isArray = isArray;
	VM->forceInterruptCheck = forceInterruptCheck;
#endif

#if VM_PROXY_MINOR > 6
	VM->fetchLong32ofObject = fetchLong32ofObject;
	VM->getThisSessionID = getThisSessionID;
	VM->ioFilenamefromStringofLengthresolveAliases = ioFilenamefromStringofLengthresolveAliases;
	VM->vmEndianness = vmEndianness;
#endif

#if VM_PROXY_MINOR > 7
	VM->addGCRoot = addGCRoot;
	VM->removeGCRoot = removeGCRoot;
#endif

#if VM_PROXY_MINOR > 8
	VM->primitiveFailFor    = primitiveFailFor;
	VM->isOopImmutable = isOopImmutable;
	VM->isOopMutable   = isOopMutable;
#endif

#if VM_PROXY_MINOR > 9
# if VM_PROXY_MINOR > 13 /* OS Errors available in primitives; easy return forms */
	VM->methodReturnBool = methodReturnBool;
	VM->methodReturnFloat = methodReturnFloat;
	VM->methodReturnInteger = methodReturnInteger;
	VM->methodReturnReceiver = methodReturnReceiver;
	VM->methodReturnString = methodReturnString;
# else
	VM->methodArg = methodArg;
	VM->objectArg = objectArg;
	VM->integerArg = integerArg;
	VM->floatArg = floatArg;
# endif
	VM->methodReturnValue = methodReturnValue;
	VM->topRemappableOop = topRemappableOop;
#endif

#if VM_PROXY_MINOR > 10
	VM->addHighPriorityTickee = addHighPriorityTickee;
	VM->addSynchronousTickee = addSynchronousTickee;
	VM->utcMicroseconds = ioUTCMicroseconds;
	VM->tenuringIncrementalGC = tenuringIncrementalGC;
	VM->isYoung = isYoung;
	VM->isKindOfClass = isKindOfClass;
	VM->primitiveErrorTable = primitiveErrorTable;
	VM->primitiveFailureCode = primitiveFailureCode;
	VM->instanceSizeOf = instanceSizeOf;
#endif

#if VM_PROXY_MINOR > 11
	VM->signedMachineIntegerValueOf = signedMachineIntegerValueOf;
	VM->stackSignedMachineIntegerValue = stackSignedMachineIntegerValue;
	VM->positiveMachineIntegerValueOf = positiveMachineIntegerValueOf;
	VM->stackPositiveMachineIntegerValue = stackPositiveMachineIntegerValue;
	VM->cStringOrNullFor = cStringOrNullFor;
	VM->signalNoResume = signalNoResume;
#endif

#if VM_PROXY_MINOR > 12 /* Spur */
	VM->isImmediate = isImmediate;
	VM->characterObjectOf = characterObjectOf;
	VM->characterValueOf = characterValueOf;
	VM->isCharacterObject = isCharacterObject;
	VM->isCharacterValue = isCharacterValue;
	VM->isPinned = isPinned;
	VM->pinObject = pinObject;
	VM->unpinObject = unpinObject;
#endif

#if VM_PROXY_MINOR > 13 /* More Spur + OS Errors available via prim error code */
	VM->statNumGCs = statNumGCs;
	VM->stringForCString = stringForCString;
	VM->primitiveFailForOSError = primitiveFailForOSError;
#endif

#if VM_PROXY_MINOR > 14 /* SmartSyntaxPlugin validation rewrite support */
	VM->isBooleanObject = isBooleanObject ;
	VM->isPositiveMachineIntegerObject = isPositiveMachineIntegerObject;
#endif


	VM->ptEnterInterpreterFromCallback = ptEnterInterpreterFromCallback;
	VM->ptExitInterpreterToCallback = ptExitInterpreterToCallback;

	VM->isNonImmediate = isNonImmediate;

	VM->platformSemaphoreNew = platform_semaphore_new;
	VM->scheduleInMainThread = NULL;

	VM->waitOnExternalSemaphoreIndex = waitOnExternalSemaphoreIndex;

	return VM;
}

void
printPhaseTime(int phase)
{
	static	int printTimes;
	static	usqLong lastusecs;
			usqLong nowusecs, usecs;

	if (phase == 1) {
		time_t nowt;
		struct tm nowtm;
		printTimes = 1;
		nowt = time(0);
		nowtm = *localtime(&nowt);
		printf("started at %s", asctime(&nowtm));
		lastusecs = ioUTCMicrosecondsNow();
		return;
	}

	if (!printTimes) return;

	nowusecs = ioUTCMicrosecondsNow();
	usecs = nowusecs - lastusecs;
	lastusecs = nowusecs;
#define m 1000000ULL
#define k 1000ULL
#define ul(v) (unsigned long)(v)
	if (phase == 2)
		printf("loaded in %lu.%03lus\n", ul(usecs/m), ul((usecs % m + k/2)/k));
	if (phase == 3) {
		printTimes = 0; /* avoid repeated printing if error during exit */
		if (usecs >= 1ULL<<32)
			printf("ran for a long time\n");
		else
			printf("ran for %lu.%03lus\n", ul(usecs/m), ul((usecs % m + k/2)/k));
	}
#undef m
#undef k
}
