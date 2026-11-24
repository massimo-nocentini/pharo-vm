/* interpret.inc - the fast interpreter of the Emscripten VM (WASM_FAST_INTERP=ON)
 *
 * Included into the generated interpreter by cmake/emscripten/fastInterpreter.py, which renames the generated
 * interpret() to interpretGeneric(): it runs one bytecode (with its extensions) or finishes one send, and returns.
 *
 * The state of the interpreter here is
 *	IP, SP, lfpb	the instruction, stack and (less BIAS) frame pointers: 32-bit in the web VM (PHARO_WASM_ADDRESS32), so that the
 *			engine folds constant offsets into its addressing modes; lfpb is biased so that they are not negative
 *	tos		in state 1, the top of the stack, which is not in memory
 *	BC		the next bytecode, plus 256 in state 1
 * and nothing else lives across bytecodes.  No extension is pending here: extA and extB are the generic code's.
 * All the back edges go to the one loop head, hotH0, which dispatches both states: two loop heads would be irreducible.
 * Calls are made with the state in the interpreter's globals (EXTERNALIZE), so that no register is spilled on the
 * paths without calls.  NOS1B leaves out hotS1b, the second dispatch of state 1, reached by forward jumps only: it is
 * faster with it in Chromium 156 and Firefox, and without it in node 24 and 25.  */
#ifndef NOS1B
# ifdef PHARO_WASM_ADDRESS32
#  define NOS1B 0
# else
#  define NOS1B 1
# endif
#endif
#include "checks.inc"
#define IP local_instructionPointer
#define SP local_stackPointer
#define BC currentBytecode
#define BIAS 128
#define RCVROFF (BIAS - 32)
#define NARGSOFF (BIAS - 24 + 1)
#define METHOD GIV(method)
#define METHODA (*(iaddr_a *)&GIV(method))
enum { HE_DISPATCH, HE_SEND, HE_EXECUTE, HE_ACTIVATE, HE_RETURN, HE_CALLERRETURN, HE_INIT };
#define LDBU(p,c) ((uint32_t)*(unsigned char *)IPTR(p,c))
#define LDWU(p,c) (*(uint32_a *)IPTR(p,c))
#define STB(p,c,v) (*(unsigned char *)IPTR(p,c) = (unsigned char)(v))
#define KEEP(x) __asm__ volatile("" : "+r"(x))	/* keeps the two arms of a branch, or the loads of several handlers, apart */
#define KEEPMEM __asm__ volatile("")
#define EXTERNALIZE (GIV(framePointer) = (char *)(uintptr_t)(lfpb + BIAS), GIV(instructionPointer) = (usqInt)IP, GIV(stackPointer) = (char *)(uintptr_t)SP)
#define INTERNALIZE (lfpb = (iaddr)GIV(framePointer) - BIAS, IP = (iaddr)GIV(instructionPointer), SP = (iaddr)GIV(stackPointer))
#define OPB LDBU(IP, 0)
#define FETCH0 do { uint32_t b_ = LDBU(IP, 1); KEEP(b_); BC = b_; IP += 1; } while (0)
#define FETCHH1 do { uint32_t b_ = LDBU(IP, 1) + 256; KEEP(b_); BC = b_; IP += 1; } while (0)
#define ISFWD(o) (!((o) & 7) && UNLIKELY((LDQ(o,0) & (0x3FFFFF - 8)) == 0))
#define SPLSEL(i) ((sqInt)LDQ(LDA(GIV(specialObjectsOop), 8 + SpecialSelectors*8) + (i) * 16, 8))
#define SPLARGC(i) (((sqInt)LDQ(LDA(GIV(specialObjectsOop), 8 + SpecialSelectors*8) + (i) * 16, 16)) >> 3)
#define ATC(r) (&GIV(atCache)[ATIX(r)])
#ifdef PHARO_WASM_ADDRESS32
# define PERMSTORE(r) 0
#else
# define PERMSTORE(r) ((r) >= 0x20000000000LL)
#endif
/* the store needs the write barrier (or the permanent-space one): left to the generic code */
#define NEEDS_BARRIER(r,h,v) (!((v) & 7) && (PERMSTORE(r) || (((r) & GIV(memoryMap)->spaceMaskToUse) == GIV(memoryMap)->oldSpaceMask \
	&& ((v) & GIV(memoryMap)->spaceMaskToUse) == GIV(memoryMap)->newSpaceMask && (v) >= (sqInt)GIV(memoryMap)->newSpaceStart && !((h) & (1u << 29)))))
#define SFVAL(o) hotSmallFloatValue(o)
/* only for the special selector at: or at:put: itself: the cache says that its receiver answers it with the primitive */
#define ATINST(b,f,n) do { if (LDBU(IP, 0) == ((b) ? 113 : 112)) { sqIntptr_t *c_ = ATC(hotRcvr) + (b); c_[AtCacheOop] = hotRcvr; c_[AtCacheFmt] = (f); c_[AtCacheSize] = (n); c_[AtCacheFixedFields] = 0; } } while (0)
#define MCTEST if (LIKELY(LDQ(e_, 8) == hotSel && LDQ(e_, 16) == hotTag)) goto hotHit;
#define ARGC(n) (GIV(argumentCount) = (n))
#define MCBASE ((iaddr)(uintptr_t)GIV(methodCache))
#define FLOATRES do { DROP2; goto hotFloatRes; } while (0)

/* The call of a primitive, in a function too large for the engine to inline: V8 (wasm-inlining-call-indirect) would otherwise
 * inline the hottest primitives into interpret(), which costs its other paths registers. */
static volatile int hotFiller;
static __attribute__((noinline)) void hotCallPrimitive(void)
{
	primitiveFunctionPointer();
	if (UNLIKELY(GIV(primFailCode) == 0x7FFFFFFF)) {
#define F8 hotFiller = 1; hotFiller = 2; hotFiller = 3; hotFiller = 4; hotFiller = 5; hotFiller = 6; hotFiller = 7; hotFiller = 8;
		F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8 F8
#undef F8
	}
}
static __attribute__((noinline)) sqInt hotAfterPrimitive(iaddr fp, iaddr sp)
{	/* after a primitive that failed, or needs a check: answer if it succeeded */
	if (GIV(primFailCode)) retryPrimitiveOnFailure();
	if (checkAllocFiller && (freeStart()) < GIV(scavengeThreshold) && (unsignedLongAt(freeStart())) != (freeStart())) GIV(primFailCode) = PrimErrWritePastObject;
	if (!GIV(primFailCode) && (iaddr)GIV(framePointer) == fp && (iaddr)GIV(stackPointer) != sp) {
		failUnbalancedPrimitive();
		GIV(stackPointer) = (char *)(uintptr_t)(sp - GIV(argumentCount) * 8); }
	if (GIV(nextProfileTick) > 0) checkProfileTick(GIV(newMethod));
	return !GIV(primFailCode);
}
#define GENATTR __attribute__((noinline))
static sqInt interpretGeneric(sqInt hotEntry, sqInt hotArg);
sqInt interpret(void)
{
	iaddr lfpb, local_instructionPointer, local_stackPointer;
	uint32_t currentBytecode, hotTag;
	sqInt tos, hotRet, hotRcvr, hotSel, hotEntry, hotArg = 0;
	double hotD1;
	volatile iaddr hotSavedFP, hotSavedSP;
	if (GIV(stackLimit) == 0) return interpretGeneric(HE_INIT, 0);
	INTERNALIZE;
	FETCH0; tos = 0;
hotH0:
	switch (BC) {
#define HS 0
#include "handlers.inc"
#undef HS
#define HS 1
#include "handlers.inc"
#undef HS
	default: if (BC < 256) goto hotGeneric; goto hotGenericS1;
	}
#if !NOS1B
hotS1b:
	switch (BC) {
#define HS 2
#include "handlers.inc"
#undef HS
	default: goto hotGenericS1;
	}
#endif

hotCmpTrue: {
	uint32_t nb_ = LDBU(IP, 1);
	if (LIKELY((nb_ & 0xF8) == 192)) { BC = LDBU(IP, 2); IP += 2; tos = 0; goto hotH0; }
	if (nb_ == 239) { BC = LDBU(IP, 3); IP += 3; tos = 0; goto hotH0; }
	if ((nb_ & 0xF8) == 184) { IP += 2 + (nb_ & 7) + 1; BC = LDBU(IP, 0); tos = 0; goto hotH0; }
	if (nb_ == 238) { IP += 3 + LDBU(IP, 2); BC = LDBU(IP, 0); tos = 0; goto hotH0; }
	tos = GIV(trueObj); FETCHH1; goto hotH0; }
hotCmpFalse: {
	uint32_t nb_ = LDBU(IP, 1);
	if (LIKELY((nb_ & 0xF8) == 192)) { IP += 2 + (nb_ & 7) + 1; BC = LDBU(IP, 0); tos = 0; goto hotH0; }
	if (nb_ == 239) { IP += 3 + LDBU(IP, 2); BC = LDBU(IP, 0); tos = 0; goto hotH0; }
	if ((nb_ & 0xF8) == 184) { BC = LDBU(IP, 2); IP += 2; tos = 0; goto hotH0; }
	if (nb_ == 238) { BC = LDBU(IP, 3); IP += 3; tos = 0; goto hotH0; }
	tos = GIV(falseObj); FETCHH1; goto hotH0; }

hotFloatRes: {
	usqLong rot_; memcpy(&rot_, &hotD1, 8);
	rot_ = (rot_ << 1) | (rot_ >> 63);
	if (rot_ > 1) rot_ -= (usqLong)896 << 53;
	if (LIKELY(rot_ - 2 < ((usqLong)1 << 61) - 2)) tos = (sqInt)((rot_ << 3) | 4);
	else { EXTERNALIZE; tos = floatObjectOf(hotD1); INTERNALIZE; }
	FETCHH1; goto hotH0; }

hotReturn: {	/* hotRet from a method or a block */
	uint32_t fl_ = LDWU(lfpb, BIAS - 24);
	iaddr cfp_ = LDA(lfpb, BIAS);
	if (UNLIKELY(fl_ >> 24)) { if (OPB == 92) { SP -= 8; STQ(SP, 0, hotRet); } hotArg = hotRet; hotEntry = HE_RETURN; goto hotCallGeneric; }
	if (UNLIKELY(cfp_ == 0)) { if (OPB == 92) { SP -= 8; STQ(SP, 0, hotRet); } hotArg = hotRet; hotEntry = HE_CALLERRETURN; goto hotCallGeneric; }
	tos = hotRet;
	SP = lfpb + ((fl_ >> 8) & 255) * 8 + (BIAS + 24);
	IP = LDA(lfpb, BIAS + 8);
	lfpb = cfp_ - BIAS;
	METHOD = LDQ(lfpb, BIAS - 8);
	FETCHH1;
	goto hotH0; }
hotReturnBlock: {
	uint32_t fl_ = LDWU(lfpb, BIAS - 24);
	iaddr cfp_ = LDA(lfpb, BIAS);
	if (UNLIKELY(cfp_ == 0)) { if (OPB == 94) { SP -= 8; STQ(SP, 0, hotRet); } hotArg = hotRet; hotEntry = HE_CALLERRETURN; goto hotCallGeneric; }
	tos = hotRet;
	SP = lfpb + ((fl_ >> 8) & 255) * 8 + (BIAS + 24);
	IP = LDA(lfpb, BIAS + 8);
	lfpb = cfp_ - BIAS;
	METHOD = LDQ(lfpb, BIAS - 8);
	FETCHH1;
	goto hotH0; }


hotClosure: {	/* hotRcvr, a FullBlockClosure, is at SP + argumentCount * 8 */
	uint32_t ac_ = (uint32_t)GIV(argumentCount);
	sqInt m_ = LDQ(hotRcvr, 16), br_ = LDQ(hotRcvr, 32);
	uint32_t ns_ = LDBU(hotRcvr, 7);
	if (LIKELY((LDQ(hotRcvr, 24) >> 3) == ac_ && ns_ != 255 && !(m_ & 7) && ((LDWU(m_, 0) >> 24) & 31) >= 24 && !ISFWD(br_))) {
		uint32_t nc_ = ns_ - 4, i_, nt_; usqInt hd_ = LDQ(m_, 8); sqInt nil_ = GIV(nilObj);
		SP -= 48;
		STQ(SP, 40, IP); STQ(SP, 32, lfpb + BIAS); STQ(SP, 24, m_); STQ(SP, 16, nil_);
		STQ(SP, 8, 1 + ((usqInt)ac_ << 8) + (1u << 24)); STQ(SP, 0, br_);
		lfpb = SP + 32 - BIAS;
		for (i_ = 0; i_ < nc_; i_++) { SP -= 8; STQ(SP, 0, LDQ(hotRcvr + i_ * 8, 40)); }
		nt_ = (uint32_t)(hd_ >> 21) & 0x3F;
		for (i_ = ac_ + nc_; i_ < nt_; i_++) { SP -= 8; STQ(SP, 0, nil_); }
		IP = (iaddr)m_ + ((uint32_t)hd_ & 0x3FFF8) + 15;
		METHOD = m_;
		if (UNLIKELY(--ioHeartbeatPollCountdown < 0) || UNLIKELY(SP < (iaddr)GIV(stackLimit))) goto hotOverflow;
		FETCH0; tos = 0; goto hotH0;
	}
	if (!GIV(messageSelector)) GIV(messageSelector) = SPLSEL(ac_ ? 26 : 25);
	hotArg = ClassFullBlockClosureCompactIndex; hotEntry = HE_SEND; goto hotCallGeneric; }

hotSend:
	hotTag = (uint32_t)hotRcvr & 7;
	if (!hotTag) hotTag = LDWU(hotRcvr, 0) & 0x3FFFFF;
hotSendTag: {
	uint32_t h_; iaddr e_;
	h_ = MCHASH((uint32_t)hotSel, hotTag);
	e_ = MCBASE + MCPROBE0(h_); KEEP(e_);
	MCTEST
	e_ = MCBASE + MCPROBE1(h_); KEEP(e_);
	MCTEST
	e_ = MCBASE + MCPROBE2(h_); KEEP(e_);
	MCTEST
	GIV(messageSelector) = hotSel;
	hotArg = hotTag; hotEntry = HE_SEND; goto hotCallGeneric;
	hotHit: {
		sqIntptr_t pf_ = (sqInt)LDQ(e_, 32);
		if (LIKELY(pf_ == 0)) {
			sqInt m_ = (sqInt)LDQ(e_, 24); uint32_t ipo_ = LDWU(e_, 40), nn_ = LDWU(e_, 44), fl_ = LDWU(e_, 48);
			sqInt nil_ = GIV(nilObj);
			if (UNLIKELY(nn_ == 255)) goto hotSendPrim;
			SP -= 48;
			STQ(SP, 40, IP); STQ(SP, 32, lfpb + BIAS); STQ(SP, 24, m_); STQ(SP, 16, nil_);
			STQ(SP, 8, (usqInt)fl_); STQ(SP, 0, hotRcvr);
			lfpb = SP + 32 - BIAS;
			IP = (iaddr)m_ + ipo_;
			METHOD = m_;
			while (nn_--) { SP -= 8; STQ(SP, 0, nil_); }
			if (UNLIKELY(--ioHeartbeatPollCountdown < 0) || UNLIKELY(SP < (iaddr)GIV(stackLimit))) goto hotOverflow;
			FETCH0; tos = 0; goto hotH0;
		}
		if (pf_ <= MaxQuickPrimitiveIndex) {
			if (LIKELY(pf_ >= 264)) { tos = LDQ((iaddr)hotRcvr + ((iaddr)pf_ - 264) * 8, 8); SP += 8; FETCHH1; goto hotH0; }
			tos = pf_ == 256 ? hotRcvr : pf_ == 257 ? GIV(trueObj) : pf_ == 258 ? GIV(falseObj) : pf_ == 259 ? GIV(nilObj) : (((pf_ - 261) << 3) | 1);
			SP += 8; FETCHH1; goto hotH0;
		}
		hotTag = (uint32_t)hotRcvr & 7;	/* the class of the receiver: of a super send, not the one looked up */
		if (!hotTag) hotTag = LDWU(hotRcvr, 0) & 0x3FFFFF;
		GIV(messageSelector) = LDQ(e_, 8);
		if (pf_ == (sqIntptr_t)primitiveFullClosureValue && hotTag == ClassFullBlockClosureCompactIndex) goto hotClosure;
		{ uint32_t ac_ = (uint32_t)GIV(argumentCount);
		if (pf_ == (sqIntptr_t)primitiveClass && ac_ == 0) {
			tos = LDQ(LDQ(GIV(hiddenRootsObj) + (hotTag >> 10) * 8, 8) + (hotTag & 1023) * 8, 8); SP += 8; FETCHH1; goto hotH0; }
		if (pf_ == (sqIntptr_t)primitiveIdentical && ac_ == 1) {
			sqInt a_ = LDQ(SP, 0);
			if (!ISFWD(a_)) { SP += 16; tos = a_ == hotRcvr ? GIV(trueObj) : GIV(falseObj); FETCHH1; goto hotH0; } }
		if (pf_ == (sqIntptr_t)primitiveNew && ac_ == 0 && !(hotRcvr & 7) && LDBU(hotRcvr, 7) > 2) {
			usqInt f_ = LDQ(hotRcvr, 24); uint32_t ci_ = LDWU(hotRcvr, 4) & 0x3FFFFF, n_ = (uint32_t)(f_ >> 3) & 0xFFFF, sp_ = (uint32_t)(f_ >> 19) & 31, by_ = 8 + (n_ ? n_ : 1) * 8, i_;
			if (LIKELY((f_ & 7) == 1 && sp_ <= 1 && ci_ && n_ < 64 && GIV(freeStart) + by_ <= GIV(scavengeThreshold))) {
				iaddr o_ = (iaddr)GIV(freeStart); sqInt nil_ = GIV(nilObj);
				STQ(o_, 0, ((usqInt)n_ << 56) + (sp_ << 24) + ci_);
				GIV(freeStart) += by_;
				for (i_ = 0; i_ < n_; i_++) STQ(o_ + i_ * 8, 8, nil_);
				tos = o_; SP += 8; FETCHH1; goto hotH0; } }
		if (pf_ == (sqIntptr_t)primitiveAt || pf_ == (sqIntptr_t)primitiveStringAt) {
			sqInt ix_ = LDQ(SP, 0);
			if (ac_ == 1 && !(hotRcvr & 7) && (ix_ & 7) == 1) {
				uint32_t f_ = (LDWU(hotRcvr, 0) >> 24) & 31; usqInt n_ = LDB(hotRcvr, 7), i_ = (usqInt)(ix_ >> 3) - 1;
				if (n_ == 255) n_ = (LDQ(hotRcvr - 8, 0) << 8) >> 8;
				if (f_ == 2 && i_ < n_ && pf_ == (sqIntptr_t)primitiveAt) { ATINST(0, f_, n_); tos = LDQ(hotRcvr + i_ * 8, 8); SP += 16; FETCHH1; goto hotH0; }
				if ((f_ & ~7u) == 16 && i_ < n_ * 8 - (f_ & 7)) { ATINST(0, pf_ == (sqIntptr_t)primitiveAt ? f_ : f_ + 32, n_ * 8 - (f_ & 7)); tos = LDB(hotRcvr + i_, 8) * 8 + (pf_ == (sqIntptr_t)primitiveAt ? 1 : 2); SP += 16; FETCHH1; goto hotH0; }
				if (LDBU(IP, 0) == 112) goto hotGeneric;	/* fills the at cache */
			} }
		if (pf_ == (sqIntptr_t)primitiveAtPut || pf_ == (sqIntptr_t)primitiveStringAtPut) {
			sqInt ix_ = LDQ(SP, 8), v_ = LDQ(SP, 0);
			if (ac_ == 2 && !(hotRcvr & 7) && (ix_ & 7) == 1 && !(LDWU(hotRcvr, 0) & (1 << 23))) {
				uint32_t h_ = LDWU(hotRcvr, 0), f_ = (h_ >> 24) & 31; usqInt n_ = LDB(hotRcvr, 7), i_ = (usqInt)(ix_ >> 3) - 1;
				if (n_ == 255) n_ = (LDQ(hotRcvr - 8, 0) << 8) >> 8;
				if (f_ == 2 && i_ < n_ && pf_ == (sqIntptr_t)primitiveAtPut && !NEEDS_BARRIER(hotRcvr, h_, v_)) { ATINST(AtPutBase, f_, n_); STQ(hotRcvr + i_ * 8, 8, v_); tos = v_; SP += 24; FETCHH1; goto hotH0; }
				if ((f_ & ~7u) == 16 && i_ < n_ * 8 - (f_ & 7) && (v_ & 7) == (pf_ == (sqIntptr_t)primitiveAtPut ? 1 : 2) && (usqInt)v_ >> 3 < 256) { ATINST(AtPutBase, pf_ == (sqIntptr_t)primitiveAtPut ? f_ : f_ + 32, n_ * 8 - (f_ & 7)); STB(hotRcvr + i_, 8, v_ >> 3); tos = v_; SP += 24; FETCHH1; goto hotH0; }
				if (LDBU(IP, 0) == 113) goto hotGeneric;
			} }
		}
	hotSendPrim:
		GIV(messageSelector) = LDQ(e_, 8);
		GIV(newMethod) = (sqInt)LDQ(e_, 24); primitiveFunctionPointer = (void (*)())pf_;
		if (pf_ <= MaxQuickPrimitiveIndex) { hotEntry = HE_EXECUTE; goto hotCallGeneric; }
		GIV(primFailCode) = 0;
		hotSavedFP = lfpb; hotSavedSP = SP + (uint32_t)GIV(argumentCount) * 8;
		EXTERNALIZE;
		hotCallPrimitive();
		if (LIKELY(!GIV(primFailCode) && !checkAllocFiller && GIV(nextProfileTick) <= 0
				&& ((iaddr)GIV(framePointer) - BIAS != hotSavedFP || (iaddr)GIV(stackPointer) == hotSavedSP))
			|| hotAfterPrimitive(hotSavedFP + BIAS, hotSavedSP)) {
			INTERNALIZE; BC = LDBU(IP, 1); IP += 1; tos = 0; goto hotH0;
		}
		hotEntry = HE_ACTIVATE; goto hotCallGenericExternalized;
	}
}
hotOverflow:
	EXTERNALIZE;
	if (ioHeartbeatPollCountdown < 0) ioHeartbeatPoll();
	if (GIV(stackPointer) < GIV(stackLimit))
		handleStackOverflowOrEventAllowContextSwitch(byteAt((sqInt)GIV(framePointer) + FoxIFrameFlags + 3) ? 1 : canContextSwitchIfActivatingheader(METHOD, LDQ(METHODA, 8)));
	INTERNALIZE; BC = LDBU(IP, 1); IP += 1; tos = 0; goto hotH0;
hotEvents:
	EXTERNALIZE;
	if (ioHeartbeatPollCountdown < 0) ioHeartbeatPoll();
	if (GIV(stackPointer) < GIV(stackLimit)) checkForEventsMayContextSwitch(1);
	INTERNALIZE; BC = LDBU(IP, 1); IP += 1; tos = 0; goto hotH0;
hotArithMixedS1:
	SP -= 8; STQ(SP, 0, tos);
hotArithMixed: {	/* + - < > <= >= = ~= * / of operands that are not two SmallIntegers or two SmallFloats, or that overflowed */
	sqInt r_ = LDQ(SP, 8), a_ = LDQ(SP, 0); double x_, y_; uint32_t t_;
	if (r_ & a_ & 1) goto hotGeneric;
	t_ = (uint32_t)r_ & 7;
	if (t_ == 4) x_ = SFVAL(r_);
	else if (t_ == 1) { if ((sqInt)((usqInt)r_ << 9) >> 9 != r_) goto hotGeneric; x_ = (double)(r_ >> 3); }
	else if (t_ == 0 && (LDWU(r_, 0) & 0x3FFFFF) == ClassFloatCompactIndex) x_ = *(double *)IPTR(r_, 8);
	else goto hotArithSend;
	t_ = (uint32_t)a_ & 7;
	if (t_ == 4) y_ = SFVAL(a_);
	else if (t_ == 1) { if ((sqInt)((usqInt)a_ << 9) >> 9 != a_) goto hotGeneric; y_ = (double)(a_ >> 3); }
	else if (t_ == 0 && (LDWU(a_, 0) & 0x3FFFFF) == ClassFloatCompactIndex) y_ = *(double *)IPTR(a_, 8);
	else goto hotArithSend;
	switch OPB {
	case 96: hotD1 = x_ + y_; SP += 16; goto hotFloatRes;
	case 97: hotD1 = x_ - y_; SP += 16; goto hotFloatRes;
	case 104: hotD1 = x_ * y_; SP += 16; goto hotFloatRes;
	case 105: if (y_ == 0.0) goto hotGeneric; hotD1 = x_ / y_; SP += 16; goto hotFloatRes;
	case 98: t_ = x_ < y_; break;
	case 99: t_ = x_ > y_; break;
	case 100: t_ = x_ <= y_; break;
	case 101: t_ = x_ >= y_; break;
	case 102: t_ = x_ == y_; break;
	case 103: t_ = x_ != y_; break;
	default: goto hotGeneric;
	}
	SP += 16;
	if (t_) goto hotCmpTrue; else goto hotCmpFalse; }
hotArithSend:
	hotSel = SPLSEL(OPB - 96); ARGC(1); hotRcvr = LDQ(SP, 8); goto hotSend;
hotContextVariable:	/* sender, pc or stackp of a context, the receiver */
	hotArg = LDBU(IP, 1); hotRcvr = LDQ(lfpb, RCVROFF); IP += 2;
	EXTERNALIZE; tos = instVarofContext(hotArg, hotRcvr); INTERNALIZE;
	IP -= 1; FETCHH1; goto hotH0;
hotThisContext:
	if (LDBU(lfpb, BIAS - 24 + 2)) tos = LDQ(lfpb, BIAS - 16);
	else { EXTERNALIZE; tos = marryFrameSP((char *)(uintptr_t)(lfpb + BIAS), (char *)(uintptr_t)SP); INTERNALIZE; }
	FETCHH1; goto hotH0;
hotPushArray: {
	uint32_t n_ = LDBU(IP, 1), k_ = n_ & 127, i_; iaddr o_ = (iaddr)GIV(freeStart); uint32_t by_ = 8 + (k_ ? k_ : 1) * 8;
	if (UNLIKELY(GIV(freeStart) + by_ > GIV(scavengeThreshold))) goto hotGeneric;
	STQ(o_, 0, ((usqInt)k_ << 56) + (2u << 24) + ClassArrayCompactIndex);
	GIV(freeStart) += by_;
	if (n_ > 127) { for (i_ = 0; i_ < k_; i_++) STQ(o_ + i_ * 8, 8, LDQ(SP + (k_ - i_) * 8 - 8, 0)); SP += k_ * 8; }
	else { sqInt nil_ = GIV(nilObj); for (i_ = 0; i_ < k_; i_++) STQ(o_ + i_ * 8, 8, nil_); }
	tos = o_; IP += 1; FETCHH1; goto hotH0; }
hotPushClosure: {
	uint32_t b_ = LDBU(IP, 2), nc_ = b_ & 63, n_ = nc_ + 4, by_ = 8 + n_ * 8, i_; iaddr o_;
	sqInt cb_ = LDQ(METHODA + LDBU(IP, 1) * 8, 16), ctx_;
	if (UNLIKELY(GIV(freeStart) + by_ + 64 * 8 > GIV(scavengeThreshold)) || ISFWD(cb_)) goto hotGeneric;
	if (b_ & 64) ctx_ = GIV(nilObj);
	else if (LDBU(lfpb, BIAS - 24 + 2)) ctx_ = LDQ(lfpb, BIAS - 16);
	else {	/* marry the frame first; then this bytecode again */
		EXTERNALIZE; marryFrameSP((char *)(uintptr_t)(lfpb + BIAS), (char *)(uintptr_t)(SP + nc_ * 8)); INTERNALIZE;
		BC = LDBU(IP, 0); tos = 0; goto hotH0; }
	o_ = (iaddr)GIV(freeStart);
	STQ(o_, 0, ((usqInt)n_ << 56) + (3u << 24) + ClassFullBlockClosureCompactIndex);
	GIV(freeStart) += by_;
	STQ(o_, 8, ctx_); STQ(o_, 16, cb_); STQ(o_, 24, ((((usqInt)LDQ(cb_, 8) >> 27) & 15) << 3) | 1);
	if (b_ & 128) { for (i_ = 0; i_ <= nc_; i_++) STQ(o_ + i_ * 8, 32, LDQ(SP + (nc_ - i_) * 8, 0)); SP += (nc_ + 1) * 8; }
	else { STQ(o_, 32, LDQ(lfpb, RCVROFF)); for (i_ = 0; i_ < nc_; i_++) STQ(o_ + i_ * 8, 40, LDQ(SP + (nc_ - 1 - i_) * 8, 0)); SP += nc_ * 8; }
	tos = o_; IP += 2; FETCHH1; goto hotH0; }
hotGenericS1:
	SP -= 8; STQ(SP, 0, tos);
hotGeneric:
	hotEntry = HE_DISPATCH;
hotCallGeneric:
	EXTERNALIZE;
hotCallGenericExternalized:
	interpretGeneric(hotEntry, hotArg);
	INTERNALIZE; BC = LDBU(IP, 0); tos = 0; goto hotH0;
}
#undef IP
#undef SP
#undef BC
#undef METHOD
