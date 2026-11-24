/* checks.inc - what the fast interpreter assumes of the generated one, checked when it is compiled
 *
 * interpret.inc and handlers.inc, and the code that cmake/emscripten/fastInterpreter.py adds, have the numbers of the
 * layout of objects, methods, frames and caches in them.  Each is compared here with the constant of the generated
 * interp.c that it repeats; the script checks the generated code itself.  When one of these fails, the number has to
 * change in the fast interpreter too (docs/WebAssembly.md, Faster interpreters), or WASM_FAST_INTERP=OFF to be given. */
#define FASTINTERP_IS(name, value) _Static_assert((name) == (value), "the fast interpreter assumes that " #name " is " #value)
#define FASTINTERP_STR1(x) #x
#define FASTINTERP_STR(x) FASTINTERP_STR1(x)

/* words, tags and SmallFloats */
FASTINTERP_IS(sizeof(sqInt), 8);
FASTINTERP_IS(sizeof(sqIntptr_t), 8);
FASTINTERP_IS(BytesPerWord, 8);
FASTINTERP_IS(BytesPerOop, 8);
FASTINTERP_IS(VMBIGENDIAN, 0);
FASTINTERP_IS(tagMask(), 7);
FASTINTERP_IS(numTagBits(), 3);
FASTINTERP_IS(smallIntegerTag(), 1);
FASTINTERP_IS(characterTag(), 2);
FASTINTERP_IS(smallFloatTag(), 4);
FASTINTERP_IS(smallFloatExponentBits(), 8);
FASTINTERP_IS(smallFloatExponentOffset(), 896);
FASTINTERP_IS(smallFloatMantissaBits(), 52);
FASTINTERP_IS(ConstZero, 1);
FASTINTERP_IS(ConstOne, 9);

/* the header of an object, and the formats of classes */
FASTINTERP_IS(BaseHeaderSize, 8);
FASTINTERP_IS(classIndexMask(), 0x3FFFFF);
FASTINTERP_IS(identityHashHalfWordMask(), 0x3FFFFF);
FASTINTERP_IS(identityHashFullWordShift(), 32);
FASTINTERP_IS(isForwardedObjectClassIndexPun(), 8);
FASTINTERP_IS(immutableBitShift(), 23);
FASTINTERP_IS(formatShift(), 24);
FASTINTERP_IS(formatMask(), 0x1F);
FASTINTERP_IS(rememberedBitShift(), 29);
FASTINTERP_IS(numSlotsFullShift(), 56);
FASTINTERP_IS(numSlotsMask(), 255);
FASTINTERP_IS(shiftForWord(), 3);
FASTINTERP_IS(indexablePointersFormat(), 3);
FASTINTERP_IS(weakArrayFormat(), 4);
FASTINTERP_IS(sixtyFourBitIndexableFormat(), 9);
FASTINTERP_IS(firstLongFormat(), 10);
FASTINTERP_IS(firstShortFormat(), 12);
FASTINTERP_IS(firstByteFormat(), 16);
FASTINTERP_IS(firstCompiledMethodFormat(), 24);
FASTINTERP_IS(classTableMajorIndexShift(), 10);
FASTINTERP_IS(fixedFieldsFieldWidth(), 16);
FASTINTERP_IS(formatFieldWidthShift(), 5);
FASTINTERP_IS(SuperclassIndex, 0);
FASTINTERP_IS(InstanceSpecificationIndex, 2);
FASTINTERP_IS(KeyIndex, 0);
FASTINTERP_IS(ValueIndex, 1);
FASTINTERP_IS(IMMUTABILITY, 1);
FASTINTERP_IS(TempVectReadBarrier, 0);

/* the special objects */
FASTINTERP_IS(SpecialSelectors, 23);
FASTINTERP_IS(ClassFloatCompactIndex, 34);
FASTINTERP_IS(ClassMethodContextCompactIndex, 36);
FASTINTERP_IS(ClassFullBlockClosureCompactIndex, 38);
FASTINTERP_IS(ClassArrayCompactIndex, 51);
FASTINTERP_IS(ClassByteStringCompactIndex, 52);

/* methods, blocks and contexts */
FASTINTERP_IS(HeaderIndex, 0);
FASTINTERP_IS(LiteralStart, 1);
FASTINTERP_IS(AlternateHeaderHasPrimFlag, 0x80000);
FASTINTERP_IS(AlternateHeaderNumLiteralsMask, 0x7FFF);
FASTINTERP_IS(MethodHeaderArgCountShift, 27);
FASTINTERP_IS(MethodHeaderTempCountShift, 21);
FASTINTERP_IS(FullClosureOuterContextIndex, 0);
FASTINTERP_IS(FullClosureCompiledBlockIndex, 1);
FASTINTERP_IS(FullClosureNumArgsIndex, 2);
FASTINTERP_IS(FullClosureReceiverIndex, 3);
FASTINTERP_IS(FullClosureFirstCopiedValueIndex, 4);
FASTINTERP_IS(SenderIndex, 0);
FASTINTERP_IS(InstructionPointerIndex, 1);
FASTINTERP_IS(StackPointerIndex, 2);
FASTINTERP_IS(MaxQuickPrimitiveIndex, 519);

/* frames: the words over and under the frame pointer (the stack grows down) */
FASTINTERP_IS(IFrameSlots, 6);
FASTINTERP_IS(FoxCallerSavedIP, 8);
FASTINTERP_IS(FoxSavedFP, 0);
FASTINTERP_IS(FoxMethod, -8);
FASTINTERP_IS(FoxThisContext, -16);
FASTINTERP_IS(FoxIFrameFlags, -24);
FASTINTERP_IS(FoxIFReceiver, -32);

/* the caches, as fastInterpreter.py makes them */
FASTINTERP_IS(MethodCacheSelector, 1);
FASTINTERP_IS(MethodCacheClass, 2);
FASTINTERP_IS(MethodCacheMethod, 3);
FASTINTERP_IS(MethodCachePrimFunction, 4);
FASTINTERP_IS(MethodCacheEntrySize, 8);
FASTINTERP_IS(MethodCacheMask, (MethodCacheEntries - 1) * MethodCacheEntrySize);
FASTINTERP_IS(MethodCacheSize, MethodCacheEntries * MethodCacheEntrySize);
FASTINTERP_IS(CacheProbeMax, 3);
FASTINTERP_IS(AtCacheOop, 1);
FASTINTERP_IS(AtCacheSize, 2);
FASTINTERP_IS(AtCacheFmt, 3);
FASTINTERP_IS(AtCacheFixedFields, 4);
FASTINTERP_IS(AtCacheMask, AtPutBase - 4);
FASTINTERP_IS(AtCacheTotalSize, AtPutBase * 2);

/* the VM: a StackVM for images with extended bytecodes, and the poll of the heartbeat that the handlers inline */
FASTINTERP_IS(COGVM, 0);
FASTINTERP_IS(SistaVM, 0);
FASTINTERP_IS(BytecodeSetHasExtensions, 1);
FASTINTERP_IS(FailImbalancedPrimitives, 1);
_Static_assert(__builtin_strcmp(FASTINTERP_STR(PHARO_POLL_HEARTBEAT()), "(--ioHeartbeatPollCountdown < 0 ? ioHeartbeatPoll() : (void)0)") == 0,
	"the fast interpreter has the poll of the heartbeat of sqPlatformSpecific.h in it");
