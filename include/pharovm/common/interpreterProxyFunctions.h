/* interpreterProxyFunctions.h -- the functions the interpreter proxy is built from.
 *
 * These are the entry points `sqGetInterpreterProxy` wires into the
 * `VirtualMachine` struct: mostly the generated interpreter's, a few the
 * platform layer supplies. They lived at the top of sqVirtualMachine.c until
 * the Rust port needed them somewhere a tool could read, and moving them here
 * keeps one source of truth rather than a second copy in the Rust.
 *
 * Nothing here is new; it is the same block, in the same order, with the same
 * conditionals. sqVirtualMachine.c includes it and is otherwise unchanged.
 *
 * Include "sq.h" before this.
 */

#ifndef __INTERPRETER_PROXY_FUNCTIONS_H
#define __INTERPRETER_PROXY_FUNCTIONS_H

/*** Function prototypes ***/

/* InterpreterProxy methodsFor: 'stack access' */
sqInt  pop(sqInt nItems);
void  popthenPush(sqInt nItems, sqInt oop);
void   push(sqInt object);
sqInt  pushBool(sqInt trueOrFalse);
void   pushFloat(double f);
sqInt  pushInteger(sqInt integerValue);
double stackFloatValue(sqInt offset);
sqInt  stackIntegerValue(sqInt offset);
sqInt  stackObjectValue(sqInt offset);
sqInt  stackValue(sqInt offset);

/*** variables ***/

/* InterpreterProxy methodsFor: 'object access' */
sqInt  argumentCountOf(sqInt methodPointer);
void  *arrayValueOf(sqInt oop);
sqInt  byteSizeOf(sqInt oop);
void  *fetchArrayofObject(sqInt fieldIndex, sqInt objectPointer);
sqInt  fetchClassOf(sqInt oop);
double fetchFloatofObject(sqInt fieldIndex, sqInt objectPointer);
sqInt  fetchIntegerofObject(sqInt fieldIndex, sqInt objectPointer);
sqInt  fetchPointerofObject(sqInt index, sqInt oop);
/* sqInt  fetchWordofObject(sqInt fieldIndex, sqInt oop);     *
 * has been rescinded as of VMMaker 3.8 and the 64bitclean VM *
 * work. To support old plugins we keep a valid function in   *
 * the same location in the VM struct but rename it to        *
 * something utterly horrible to scare off the natives. A new *
 * equivalent but 64 bit valid function is added as           *
 * 'fetchLong32OfObject'                                      */
sqInt  obsoleteDontUseThisFetchWordofObject(sqInt index, sqInt oop);
sqInt  fetchLong32ofObject(sqInt index, sqInt oop); 
void  *firstFixedField(sqInt oop);
void  *firstIndexableField(sqInt oop);
sqInt  literalofMethod(sqInt offset, sqInt methodPointer);
sqInt  literalCountOf(sqInt methodPointer);
sqInt  methodArgumentCount(void);
sqInt  methodPrimitiveIndex(void);
sqInt  primitiveMethod(void);
sqInt  primitiveIndexOf(sqInt methodPointer);
sqInt  sizeOfSTArrayFromCPrimitive(void *cPtr);
sqInt  slotSizeOf(sqInt oop);
sqInt  stObjectat(sqInt array, sqInt index);
sqInt  stObjectatput(sqInt array, sqInt index, sqInt value);
sqInt  stSizeOf(sqInt oop);
sqInt  storeIntegerofObjectwithValue(sqInt index, sqInt oop, sqInt integer);
sqInt  storePointerofObjectwithValue(sqInt index, sqInt oop, sqInt valuePointer);


/* InterpreterProxy methodsFor: 'testing' */
sqInt isKindOf(sqInt oop, char *aString);
sqInt isMemberOf(sqInt oop, char *aString);
sqInt isBytes(sqInt oop);
sqInt isFloatObject(sqInt oop);
sqInt isIndexable(sqInt oop);
sqInt isIntegerObject(sqInt oop);
sqInt isIntegerValue(sqInt intValue);
sqInt isPointers(sqInt oop);
sqInt isWeak(sqInt oop);
sqInt isWords(sqInt oop);
sqInt isWordsOrBytes(sqInt oop);
sqInt includesBehaviorThatOf(sqInt aClass, sqInt aSuperClass);
#if VM_PROXY_MINOR > 10
sqInt isKindOfClass(sqInt oop, sqInt aClass);
sqInt primitiveErrorTable(void);
sqInt primitiveFailureCode(void);
sqInt instanceSizeOf(sqInt aClass);
void tenuringIncrementalGC(void);
#endif
sqInt isArray(sqInt oop);
sqInt isOopMutable(sqInt oop);
sqInt isOopImmutable(sqInt oop);

/* InterpreterProxy methodsFor: 'converting' */
sqInt  booleanValueOf(sqInt obj);
sqInt  checkedIntegerValueOf(sqInt intOop);
sqInt  floatObjectOf(double aFloat);
double floatValueOf(sqInt oop);
sqInt  integerObjectOf(sqInt value);
sqInt  integerValueOf(sqInt oop);
sqInt  positive32BitIntegerFor(unsigned int integerValue);
unsigned int positive32BitValueOf(sqInt oop);
sqInt  signed32BitIntegerFor(sqInt integerValue);
int    signed32BitValueOf(sqInt oop);
sqInt  positive64BitIntegerFor(usqLong integerValue);
usqLong positive64BitValueOf(sqInt oop);
sqInt  signed64BitIntegerFor(sqLong integerValue);
sqLong signed64BitValueOf(sqInt oop);
sqIntptr_t   signedMachineIntegerValueOf(sqInt);
sqIntptr_t   stackSignedMachineIntegerValue(sqInt);
usqIntptr_t  positiveMachineIntegerValueOf(sqInt);
usqIntptr_t  stackPositiveMachineIntegerValue(sqInt);

/* InterpreterProxy methodsFor: 'special objects' */
sqInt falseObject(void);
sqInt nilObject(void);
sqInt trueObject(void);


/* InterpreterProxy methodsFor: 'special classes' */
sqInt classArray(void);
sqInt classBitmap(void);
sqInt classByteArray(void);
sqInt classCharacter(void);
sqInt classFloat(void);
sqInt classLargePositiveInteger(void);
sqInt classLargeNegativeInteger(void);
sqInt classPoint(void);
sqInt classSemaphore(void);
sqInt classSmallInteger(void);
sqInt classString(void);


/* InterpreterProxy methodsFor: 'instance creation' */
sqInt clone(sqInt oop);
sqInt instantiateClassindexableSize(sqInt classPointer, sqInt size);
sqInt makePointwithxValueyValue(sqInt xValue, sqInt yValue);
sqInt popRemappableOop(void);
void pushRemappableOop(sqInt oop);


/* InterpreterProxy methodsFor: 'other' */
sqInt becomewith(sqInt array1, sqInt array2);
sqInt byteSwapped(sqInt w);
sqInt failed(void);
void fullGC(void);
sqInt primitiveFail(void);
sqInt primitiveFailFor(sqInt reasonCode);
sqInt signalSemaphoreWithIndex(sqInt semaIndex);
sqInt success(sqInt aBoolean);
sqInt superclassOf(sqInt classPointer);
sqInt ioMicroMSecs(void);
unsigned volatile long long  ioUTCMicroseconds(void);
unsigned volatile long long  ioUTCMicrosecondsNow(void);
sqInt forceInterruptCheck(void);
sqInt getThisSessionID(void);
sqInt ioFilenamefromStringofLengthresolveAliases(char* aCharBuffer, char* filenameIndex, sqInt filenameLength, sqInt resolveFlag);
sqInt vmEndianness(void);	
sqInt getInterruptPending(void);

/* InterpreterProxy methodsFor: 'BitBlt support' */
sqInt loadBitBltFrom(sqInt bbOop);
sqInt copyBits(void);
sqInt copyBitsFromtoat(sqInt leftX, sqInt rightX, sqInt yValue);

/* InterpreterProxy methodsFor: 'FFI support' */
sqInt classExternalAddress(void);
void *ioLoadModuleOfLength(sqInt moduleNameIndex, sqInt moduleNameLength);
void *ioLoadSymbolOfLengthFromModule(sqInt functionNameIndex, sqInt functionNameLength, void* moduleHandle);
sqInt isInMemory(sqInt address);

void *startOfAlienData(sqInt);
usqInt sizeOfAlienData(sqInt);
sqInt signalNoResume(sqInt);
#if VM_PROXY_MINOR > 12 /* Spur */
sqInt isImmediate(sqInt oop);
sqInt isCharacterObject(sqInt oop);
sqInt isCharacterValue(int charCode);
sqInt characterObjectOf(sqInt charCode);
sqInt characterValueOf(sqInt oop);
sqInt isPinned(sqInt objOop);
sqInt pinObject(sqInt objOop);
sqInt unpinObject(sqInt objOop);
char *cStringOrNullFor(sqInt);
#endif
#if VM_PROXY_MINOR > 13 /* More Spur + OS Errors available via prim error code */
sqInt statNumGCs(void);
sqInt stringForCString(const char *);
sqInt primitiveFailForOSError(sqLong);
#endif
#if VM_PROXY_MINOR > 14 /* SmartSyntaxPlugin validation rewrite support */
sqInt isBooleanObject(sqInt oop);
sqInt isPositiveMachineIntegerObject(sqInt oop);
#endif

void *ioLoadFunctionFrom(char *fnName, char *modName);

void waitOnExternalSemaphoreIndex(sqInt semaphoreIndex);


/* Proxy declarations for v1.8 */
sqInt addGCRoot(sqInt *varLoc);
sqInt removeGCRoot(sqInt *varLoc);

/* Proxy declarations for v1.10 */
# if VM_PROXY_MINOR > 13 /* OS Errors available in primitives; easy return forms */
sqInt  methodReturnBool(sqInt);
sqInt  methodReturnFloat(double);
sqInt  methodReturnInteger(sqInt);
sqInt  methodReturnReceiver(void);
sqInt  methodReturnString(char *);
# else
sqInt methodArg(sqInt index);
sqInt objectArg(sqInt index);
sqInt integerArg(sqInt index);
double floatArg(sqInt index);
#endif
sqInt methodReturnValue(sqInt oop);
sqInt topRemappableOop(void);

#if ASYNC_FFI_QUEUE

sqInt ptExitInterpreterToCallback(void*);
sqInt ptEnterInterpreterFromCallback(void*);

#endif //ASYNC_FFI_QUEUE

sqInt isNonImmediate(sqInt oop);

/* Declared further down in sqVirtualMachine.c, but needed for the same table. */
extern sqInt isYoung(sqInt);

/* High-priority and synchronous ticker function support. */
void addHighPriorityTickee(void (*ticker)(void), unsigned periodms);
void addSynchronousTickee(void (*ticker)(void), unsigned periodms, unsigned roundms);

#endif /* __INTERPRETER_PROXY_FUNCTIONS_H */
