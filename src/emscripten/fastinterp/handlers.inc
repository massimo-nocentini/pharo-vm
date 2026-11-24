/* handlers.inc - the handlers of the hot bytecodes, included once per dispatch state:
 *   HS == 0: the whole expression stack is in memory
 *   HS == 1, 2: the top of the stack is in the local tos, and not in memory (2: the second dispatch, NOS1B == 0)
 * Handlers that end with a pushed value go to NEXT1 (state 1); those that consume the top go to NEXT0. */
#if HS == 0
# define SPILL		((void)0)
# define TOPV		((sqInt)LDQ(SP,0))
# define SECV		((sqInt)LDQ(SP,8))
# define THIRDV		((sqInt)LDQ(SP,16))
# define DROP1		(SP += 8)
# define DROP2		(SP += 16)
# define DROP3		(SP += 24)
# define FLUSH		((void)0)
# define GENERICGOTO	goto hotGeneric
# define ARITHMIXED	hotArithMixed
#else
# define SPILL		(SP -= 8, STQ(SP,0,tos))
# define TOPV		tos
# define SECV		((sqInt)LDQ(SP,0))
# define THIRDV		((sqInt)LDQ(SP,8))
# define DROP1		((void)0)
# define DROP2		(SP += 8)
# define DROP3		(SP += 16)
# define FLUSH		SPILL
# define GENERICGOTO	goto hotGenericS1
# define ARITHMIXED	hotArithMixedS1
#endif
#if HS == 1
# define HC(n) ((n) + 256)
#else
# define HC(n) (n)
#endif
#if HS < 2 && !NOS1B
# define FETCH1	FETCH0
# define NEXT1		goto hotS1b
# define NEXT0		do { if (HS == 0) tos = 0; goto hotH0; } while (0)
#else
# define FETCH1	FETCHH1
# define NEXT1		goto hotH0
# define NEXT0		goto hotH0
#endif
#if HS == 0
# define NEXTSAME	NEXT0
# define FETCHSAME	FETCH0
#else
# define NEXTSAME	NEXT1
# define FETCHSAME	FETCH1
#endif
#define NOTNUM(o)	(((o) & 7) == 2 || (!((o) & 7) && (LDWU(o, 0) & 0x3FFFFF) != ClassFloatCompactIndex))
#define ARITHSLOW	goto ARITHMIXED
#define GENERIC		do { GENERICGOTO; } while (0)
#define PUSHV(v)	do { sqInt hv_ = (v); SPILL; tos = hv_; FETCH1; NEXT1; } while (0)

#define CIV(n) case HC(n): PUSHV(LDQ(LDA(lfpb, RCVROFF), 8 + 8*(n)));
CIV(0) CIV(1) CIV(2) CIV(3) CIV(4) CIV(5) CIV(6) CIV(7) CIV(8) CIV(9) CIV(10) CIV(11) CIV(12) CIV(13) CIV(14) CIV(15)
#undef CIV

case HC(16): case HC(17): case HC(18): case HC(19): case HC(20): case HC(21): case HC(22): case HC(23):
case HC(24): case HC(25): case HC(26): case HC(27): case HC(28): case HC(29): case HC(30): case HC(31): {
	sqInt lv_ = LDQ(METHODA + (OPB & 15) * 8, 16);
	if (UNLIKELY((LDQ(lv_, 0) & (0x3FFFFF - 8)) == 0)) GENERIC;
	PUSHV(LDQ(lv_, 16));
}
case HC(32): case HC(33): case HC(34): case HC(35): case HC(36): case HC(37): case HC(38): case HC(39):
case HC(40): case HC(41): case HC(42): case HC(43): case HC(44): case HC(45): case HC(46): case HC(47):
case HC(48): case HC(49): case HC(50): case HC(51): case HC(52): case HC(53): case HC(54): case HC(55):
case HC(56): case HC(57): case HC(58): case HC(59): case HC(60): case HC(61): case HC(62): case HC(63):
	PUSHV(LDQ(METHODA + (OPB & 31) * 8, 16));

#define CTEMP(n) case HC(64 + n): { uint32_t na_ = LDBU(lfpb, NARGSOFF); iaddr tb_ = lfpb + na_ * 8; \
	SPILL; if (n < na_) { tos = LDQ(tb_, BIAS + 8 - 8*(n)); KEEP(tos); } else tos = LDQ(tb_, BIAS - 40 - 8*(n)); FETCH1; NEXT1; }
CTEMP(0) CTEMP(1) CTEMP(2) CTEMP(3) CTEMP(4) CTEMP(5) CTEMP(6) CTEMP(7) CTEMP(8) CTEMP(9) CTEMP(10) CTEMP(11)
#undef CTEMP

case HC(76): PUSHV(LDQ(lfpb, RCVROFF));
case HC(77): PUSHV(GIV(trueObj));
case HC(78): PUSHV(GIV(falseObj));
case HC(79): PUSHV(GIV(nilObj));
case HC(80): PUSHV(1);
case HC(81): {
#if HS
	/* pushOne, add */
	if (LDBU(IP,1) == 96 && (tos & 1)) {
		sqInt r_;
		if (LIKELY(!__builtin_add_overflow(tos, (sqInt)8, &r_))) { tos = r_; IP += 1; FETCH1; NEXT1; }
	}
#endif
	PUSHV(9);
}
case HC(83): { sqInt d_ = TOPV; SPILL; tos = d_; FETCH1; NEXT1; }

case HC(88): FLUSH; hotRet = LDQ(lfpb, RCVROFF); goto hotReturn;
case HC(89): FLUSH; hotRet = GIV(trueObj); goto hotReturn;
case HC(90): FLUSH; hotRet = GIV(falseObj); goto hotReturn;
case HC(91): FLUSH; hotRet = GIV(nilObj); goto hotReturn;
case HC(92): hotRet = TOPV; DROP1; goto hotReturn;
case HC(93): FLUSH; hotRet = GIV(nilObj); goto hotReturnBlock;
case HC(94): hotRet = TOPV; DROP1; goto hotReturnBlock;

case HC(96): { sqInt a_ = TOPV, r_ = SECV, s_;
	if (LIKELY(r_ & a_ & 1)) {
		s_ = (sqInt)((usqInt)r_ + (usqInt)a_ - 1);
		if (UNLIKELY(((s_ ^ r_) & (s_ ^ a_)) < 0)) GENERIC;
		DROP2; tos = s_; FETCH1; NEXT1;
	}
	if ((r_ & 7) == 4 && (a_ & 7) == 4) { hotD1 = SFVAL(r_) + SFVAL(a_); FLOATRES; }
	ARITHSLOW; }
case HC(97): { sqInt a_ = TOPV, r_ = SECV, s_;
	if (LIKELY(r_ & a_ & 1)) {
		s_ = (sqInt)((usqInt)r_ - (usqInt)a_ + 1);
		if (UNLIKELY(((r_ ^ a_) & (r_ ^ s_)) < 0)) GENERIC;
		DROP2; tos = s_; FETCH1; NEXT1;
	}
	if ((r_ & 7) == 4 && (a_ & 7) == 4) { hotD1 = SFVAL(r_) - SFVAL(a_); FLOATRES; }
	ARITHSLOW; }
#define CCMP(bc, op) case HC(bc): { sqInt a_ = TOPV, r_ = SECV; \
	if (LIKELY(r_ & a_ & 1)) { DROP2; if (r_ op a_) goto hotCmpTrue; else goto hotCmpFalse; } \
	if ((r_ & 7) == 4 && (a_ & 7) == 4) { DROP2; if (SFVAL(r_) op SFVAL(a_)) goto hotCmpTrue; else goto hotCmpFalse; } \
	ARITHSLOW; }
CCMP(98, <) CCMP(99, >) CCMP(100, <=) CCMP(101, >=) CCMP(102, ==) CCMP(103, !=)
#undef CCMP
case HC(104): { sqInt a_ = TOPV, r_ = SECV;
	if ((r_ & a_ & 1) && (sqInt)(int32_t)(r_ >> 3) == (r_ >> 3) && (sqInt)(int32_t)(a_ >> 3) == (a_ >> 3)) {
		sqInt s_ = (r_ >> 3) * (a_ >> 3);
		if (UNLIKELY((((usqInt)s_ >> 60) + 1 & 15) > 1)) GENERIC;
		DROP2; tos = (sqInt)(((usqInt)s_ << 3) | 1); FETCH1; NEXT1;
	}
	if ((r_ & 7) == 4 && (a_ & 7) == 4) { hotD1 = SFVAL(r_) * SFVAL(a_); FLOATRES; }
	ARITHSLOW; }
case HC(105): { sqInt a_ = TOPV, r_ = SECV;
	if ((r_ & a_ & 1) && a_ > 1 && r_ > 0 && !((usqInt)(r_ | a_) >> 35)) {
		uint32_t n_ = (uint32_t)(r_ >> 3), d_ = (uint32_t)(a_ >> 3), q_ = n_ / d_;
		if (q_ * d_ == n_) { DROP2; tos = ((sqInt)q_ << 3) | 1; FETCH1; NEXT1; }
		GENERIC;
	}
	if ((r_ & 7) == 4 && (a_ & 7) == 4 && (usqInt)a_ > 15) { hotD1 = SFVAL(r_) / SFVAL(a_); FLOATRES; }
	ARITHSLOW; }
case HC(106): { sqInt a_ = TOPV, r_ = SECV;
	if ((r_ & a_ & 1) && a_ > 1 && r_ > 0) {
		usqInt m_ = ((usqInt)(r_ | a_) >> 35) ? (usqInt)(r_ >> 3) % (usqInt)(a_ >> 3) : (usqInt)((uint32_t)(r_ >> 3) % (uint32_t)(a_ >> 3));
		DROP2; tos = (sqInt)((m_ << 3) | 1); FETCH1; NEXT1;
	}
	GENERIC; }
case HC(109): { sqInt a_ = TOPV, r_ = SECV;
	if ((r_ & a_ & 1) && a_ > 1 && r_ > 0) {
		usqInt m_ = ((usqInt)(r_ | a_) >> 35) ? (usqInt)(r_ >> 3) / (usqInt)(a_ >> 3) : (usqInt)((uint32_t)(r_ >> 3) / (uint32_t)(a_ >> 3));
		DROP2; tos = (sqInt)((m_ << 3) | 1); FETCH1; NEXT1;
	}
	GENERIC; }
case HC(108): { sqInt a_ = TOPV, r_ = SECV;
	if (r_ & a_ & 1) {
		sqInt n_ = a_ >> 3, v_ = r_ >> 3;
		if (n_ < 0) { if (n_ >= -61) { DROP2; tos = (sqInt)(((usqInt)(v_ >> -n_) << 3) | 1); FETCH1; NEXT1; } }
		else if (n_ <= 60) { sqInt s_ = (sqInt)((usqInt)v_ << n_);
			if ((s_ >> n_) == v_ && (((usqInt)s_ >> 60) + 1 & 15) <= 1) { DROP2; tos = (sqInt)(((usqInt)s_ << 3) | 1); FETCH1; NEXT1; } }
	}
	GENERIC; }
case HC(110): { sqInt a_ = TOPV, r_ = SECV; if (LIKELY(r_ & a_ & 1)) { DROP2; tos = r_ & a_; FETCH1; NEXT1; } GENERIC; }
case HC(111): { sqInt a_ = TOPV, r_ = SECV; if (LIKELY(r_ & a_ & 1)) { DROP2; tos = r_ | a_; FETCH1; NEXT1; } GENERIC; }

case HC(112): { sqInt ix_ = TOPV, r_ = SECV;
	if (LIKELY(!(r_ & 7) && (ix_ & 7) == 1)) {
		sqIntptr_t *c_ = ATC(r_);
		if (LIKELY(c_[AtCacheOop] == r_)) {
			usqInt i_ = (usqInt)(ix_ >> 3) - 1; sqInt f_ = c_[AtCacheFmt], v_;
			if (UNLIKELY(i_ >= (usqInt)c_[AtCacheSize])) GENERIC;
			{
				if (f_ <= 4) v_ = LDQ(r_ + (i_ + c_[AtCacheFixedFields]) * 8, 8);
				else if ((f_ & ~7) == 16) v_ = LDB(r_ + i_, 8) * 8 + 1;
				else if ((f_ & ~7) == 48) v_ = LDB(r_ + i_, 8) * 8 + 2;
				else GENERIC;
				DROP2; tos = v_; FETCH1; NEXT1;
			}
		}
	}
	if (!(r_ & 7)) { hotRcvr = r_; FLUSH; hotSel = SPLSEL(16); ARGC(1); goto hotSend; }
	GENERIC; }
case HC(113): { sqInt v_ = TOPV, ix_ = SECV, r_ = THIRDV;
	if (LIKELY(!(r_ & 7) && (ix_ & 7) == 1)) {
		sqIntptr_t *c_ = ATC(r_) + AtPutBase;
		uint32_t h_ = LDWU(r_, 0);
		if (UNLIKELY(h_ & (1 << 23))) GENERIC;
		if (LIKELY(c_[AtCacheOop] == r_)) {
			usqInt i_ = (usqInt)(ix_ >> 3) - 1; sqInt f_ = c_[AtCacheFmt];
			if (UNLIKELY(i_ >= (usqInt)c_[AtCacheSize])) GENERIC;
			{
				if (f_ <= 4) { if (UNLIKELY(NEEDS_BARRIER(r_, h_, v_))) GENERIC; STQ(r_ + (i_ + c_[AtCacheFixedFields]) * 8, 8, v_); }
				else if ((f_ & ~7) == 16 && (v_ & 7) == 1 && (usqInt)(v_ >> 3) < 256) STB(r_ + i_, 8, v_ >> 3);
				else if ((f_ & ~7) == 48 && (v_ & 7) == 2 && (usqInt)v_ >> 3 < 256) STB(r_ + i_, 8, v_ >> 3);
				else GENERIC;
				DROP3; tos = v_; FETCH1; NEXT1;
			}
		}
	}
	if (!(r_ & 7)) { hotRcvr = r_; FLUSH; hotSel = SPLSEL(17); ARGC(2); goto hotSend; }
	GENERIC; }
case HC(114): { sqInt r_ = TOPV;
	if (LIKELY(!(r_ & 7))) {
		uint32_t h_ = LDWU(r_, 0), ci_ = h_ & 0x3FFFFF; usqInt n_ = LDB(r_, 7);
		if (n_ == 255) n_ = (LDQ(r_ - 8, 0) << 8) >> 8;
		{
			if (ci_ == ClassArrayCompactIndex) { DROP1; tos = n_ * 8 + 1; FETCH1; NEXT1; }
			if (ci_ == ClassByteStringCompactIndex && ((h_ >> 24) & 31) >= 16) { DROP1; tos = (n_ * 8 - ((h_ >> 24) & 7)) * 8 + 1; FETCH1; NEXT1; }
			if (ci_ != ClassByteStringCompactIndex) { hotRcvr = r_; FLUSH; hotSel = SPLSEL(18); ARGC(0); goto hotSend; }
		}
	}
	GENERIC; }
case HC(118): { sqInt a_ = TOPV, r_ = SECV;
	if (ISFWD(r_) || ISFWD(a_)) GENERIC;
	DROP2; if (r_ == a_) goto hotCmpTrue; else goto hotCmpFalse; }
case HC(120): { sqInt a_ = TOPV, r_ = SECV;
	if (ISFWD(r_) || ISFWD(a_)) GENERIC;
	DROP2; if (r_ != a_) goto hotCmpTrue; else goto hotCmpFalse; }
case HC(119): { sqInt r_ = TOPV; uint32_t ci_ = (uint32_t)r_ & 7;
	if (!ci_) ci_ = LDWU(r_, 0) & 0x3FFFFF;
	if (UNLIKELY(ci_ == 8)) GENERIC;
	DROP1; tos = LDQ(LDQ(GIV(hiddenRootsObj) + (ci_ >> 10) * 8, 8) + (ci_ & 1023) * 8, 8); FETCH1; NEXT1; }

case HC(121): FLUSH; hotRcvr = LDQ(SP, 0); ARGC(0); if (!(hotRcvr & 7) && (LDWU(hotRcvr, 0) & 0x3FFFFF) == ClassFullBlockClosureCompactIndex) { GIV(messageSelector) = 0; goto hotClosure; } hotSel = SPLSEL(25); goto hotSend;
case HC(122): FLUSH; hotRcvr = LDQ(SP, 8); ARGC(1); if (!(hotRcvr & 7) && (LDWU(hotRcvr, 0) & 0x3FFFFF) == ClassFullBlockClosureCompactIndex) { GIV(messageSelector) = 0; goto hotClosure; } hotSel = SPLSEL(26); goto hotSend;
case HC(115): case HC(116): case HC(117): case HC(123): case HC(124): case HC(125): case HC(126): case HC(127):
	FLUSH; hotSel = SPLSEL(OPB - 96); { uint32_t n_ = (uint32_t)SPLARGC(OPB - 96); ARGC(n_); hotRcvr = LDQ(SP + n_ * 8, 0); } goto hotSend;
case HC(128): case HC(129): case HC(130): case HC(131): case HC(132): case HC(133): case HC(134): case HC(135):
case HC(136): case HC(137): case HC(138): case HC(139): case HC(140): case HC(141): case HC(142): case HC(143):
	hotRcvr = TOPV; FLUSH; hotSel = LDQ(METHODA + (OPB & 15) * 8, 16); KEEP(hotSel); ARGC(0); goto hotSend;
case HC(144): case HC(145): case HC(146): case HC(147): case HC(148): case HC(149): case HC(150): case HC(151):
case HC(152): case HC(153): case HC(154): case HC(155): case HC(156): case HC(157): case HC(158): case HC(159):
	hotRcvr = SECV; FLUSH; hotSel = LDQ(METHODA + (OPB & 15) * 8, 16); KEEP(hotSel); ARGC(1); goto hotSend;
case HC(160): case HC(161): case HC(162): case HC(163): case HC(164): case HC(165): case HC(166): case HC(167):
case HC(168): case HC(169): case HC(170): case HC(171): case HC(172): case HC(173): case HC(174): case HC(175):
	hotRcvr = THIRDV; FLUSH; hotSel = LDQ(METHODA + (OPB & 15) * 8, 16); KEEP(hotSel); ARGC(2); goto hotSend;

case HC(234): { uint32_t b_ = LDBU(IP, 1); FLUSH; IP += 1;
	hotSel = LDQ(METHODA + (b_ >> 3) * 8, 16); ARGC(b_ & 7); hotRcvr = LDQ(SP + (b_ & 7) * 8, 0); goto hotSend; }
case HC(235): {
	uint32_t b_ = LDBU(IP, 1), t_; sqInt lit_, cl_, r_, sup_;
	lit_ = LDQ(METHODA + (uint32_t)(LDQ(METHODA, 8) & 0x3FFF8), 8);
	if (ISFWD(lit_) || (lit_ & 7) || ((LDWU(lit_, 0) >> 24) & 31) > 5) GENERIC;
	cl_ = LDQ(lit_, 16);
	if (ISFWD(cl_) || (cl_ & 7)) GENERIC;
	sup_ = LDQ(cl_, 8);
	if (ISFWD(sup_) || (sup_ & 7)) GENERIC;
	t_ = LDWU(sup_, 4) & 0x3FFFFF;
	if (UNLIKELY(!t_)) GENERIC;
	FLUSH;
	r_ = LDQ(SP + (b_ & 7) * 8, 0);
	if (ISFWD(r_)) goto hotGeneric;
	IP += 1; hotTag = t_; hotRcvr = r_; hotSel = LDQ(METHODA + (b_ >> 3) * 8, 16); ARGC(b_ & 7); goto hotSendTag; }

case HC(176): case HC(177): case HC(178): case HC(179): case HC(180): case HC(181): case HC(182): case HC(183):
	IP += (OPB & 7) + 1; FETCHSAME; NEXTSAME;
case HC(184): case HC(185): case HC(186): case HC(187): case HC(188): case HC(189): case HC(190): case HC(191): {
	sqInt b_ = TOPV;
	if (b_ == GIV(trueObj)) { DROP1; IP += (OPB & 7) + 1; FETCH0; NEXT0; }
	if (UNLIKELY(b_ != GIV(falseObj))) GENERIC;
	DROP1; FETCH0; NEXT0; }
case HC(192): case HC(193): case HC(194): case HC(195): case HC(196): case HC(197): case HC(198): case HC(199): {
	sqInt b_ = TOPV;
	if (b_ == GIV(falseObj)) { DROP1; IP += (OPB & 7) + 1; FETCH0; NEXT0; }
	if (UNLIKELY(b_ != GIV(trueObj))) GENERIC;
	DROP1; FETCH0; NEXT0; }
case HC(200): case HC(201): case HC(202): case HC(203): case HC(204): case HC(205): case HC(206): case HC(207): {
	sqInt r_ = LDQ(lfpb, RCVROFF), v_ = TOPV; uint32_t h_ = LDWU(r_, 0);
	if (UNLIKELY(h_ & (1 << 23)) || UNLIKELY(NEEDS_BARRIER(r_, h_, v_))) GENERIC;
	DROP1; STQ(r_ + (OPB & 7) * 8, 8, v_); FETCH0; NEXT0; }
#define CSTT(n) case HC(208 + n): { sqInt v_ = TOPV; uint32_t na_ = LDBU(lfpb, NARGSOFF); iaddr tb_ = lfpb + na_ * 8; \
	if (n < na_) { STQ(tb_, BIAS + 8 - 8*(n), v_); KEEPMEM; } else STQ(tb_, BIAS - 40 - 8*(n), v_); DROP1; FETCH0; NEXT0; }
CSTT(0) CSTT(1) CSTT(2) CSTT(3) CSTT(4) CSTT(5) CSTT(6) CSTT(7)
#undef CSTT
case HC(216): DROP1; FETCH0; NEXT0;

case HC(226): { sqInt r_ = LDQ(lfpb, RCVROFF);
	if (UNLIKELY((LDWU(r_, 0) & 0x3FFFFF) == ClassMethodContextCompactIndex) && LDBU(IP, 1) <= StackPointerIndex) { FLUSH; goto hotContextVariable; }
	SPILL; tos = LDQ(r_ + LDBU(IP, 1) * 8, 8); IP += 1; FETCH1; NEXT1; }
case HC(82): FLUSH; goto hotThisContext;
case HC(227): { sqInt lv_ = LDQ(METHODA + LDBU(IP, 1) * 8, 16);
	if (UNLIKELY((LDQ(lv_, 0) & (0x3FFFFF - 8)) == 0)) GENERIC;
	SPILL; tos = LDQ(lv_, 16); IP += 1; FETCH1; NEXT1; }
case HC(228): SPILL; tos = LDQ(METHODA + LDBU(IP, 1) * 8, 16); IP += 1; FETCH1; NEXT1;
case HC(232): SPILL; tos = LDBU(IP, 1) * 8 + 1; IP += 1; FETCH1; NEXT1;
case HC(233): SPILL; tos = LDBU(IP, 1) * 8 + 2; IP += 1; FETCH1; NEXT1;
case HC(240): case HC(243): {
	sqInt r_ = LDQ(lfpb, RCVROFF), v_ = TOPV; uint32_t h_ = LDWU(r_, 0), op_ = OPB;
	if (UNLIKELY(h_ & (1 << 23)) || UNLIKELY((h_ & 0x3FFFFF) == ClassMethodContextCompactIndex) || UNLIKELY(NEEDS_BARRIER(r_, h_, v_))) GENERIC;
	STQ(r_ + LDBU(IP, 1) * 8, 8, v_); IP += 1;
	if (op_ == 240) { DROP1; FETCH0; NEXT0; }
	FETCHSAME; NEXTSAME; }
case HC(231): FLUSH; goto hotPushArray;
case HC(249): FLUSH; goto hotPushClosure;
case HC(229): { uint32_t n_ = LDBU(IP, 1); uint32_t na_ = LDBU(lfpb, NARGSOFF);
	SPILL; tos = LDQ(lfpb + (iaddr)((sqInt)na_ - (sqInt)n_) * 8 + (n_ < na_ ? BIAS + 8 : BIAS - 40), 0); IP += 1; FETCH1; NEXT1; }
case HC(242): { uint32_t n_ = LDBU(IP, 1); uint32_t na_ = LDBU(lfpb, NARGSOFF); sqInt v_ = TOPV;
	STQ(lfpb + (iaddr)((sqInt)na_ - (sqInt)n_) * 8 + (n_ < na_ ? BIAS + 8 : BIAS - 40), 0, v_); DROP1; IP += 1; FETCH0; NEXT0; }
case HC(245): { uint32_t n_ = LDBU(IP, 1); uint32_t na_ = LDBU(lfpb, NARGSOFF); sqInt v_ = TOPV;
	STQ(lfpb + (iaddr)((sqInt)na_ - (sqInt)n_) * 8 + (n_ < na_ ? BIAS + 8 : BIAS - 40), 0, v_); IP += 1; FETCHSAME; NEXTSAME; }
case HC(251): { uint32_t n_ = LDBU(IP, 2); uint32_t na_ = LDBU(lfpb, NARGSOFF), ri_ = LDBU(IP, 1);
	sqInt tv_ = LDQ(lfpb + (iaddr)((sqInt)na_ - (sqInt)n_) * 8 + (n_ < na_ ? BIAS + 8 : BIAS - 40), 0);
	SPILL; tos = LDQ(tv_ + ri_ * 8, 8); IP += 2; FETCH1; NEXT1; }
case HC(252): case HC(253): { uint32_t op_ = OPB, n_ = LDBU(IP, 2); uint32_t na_ = LDBU(lfpb, NARGSOFF), ri_ = LDBU(IP, 1);
	sqInt tv_ = LDQ(lfpb + (iaddr)((sqInt)na_ - (sqInt)n_) * 8 + (n_ < na_ ? BIAS + 8 : BIAS - 40), 0), v_ = TOPV;
	if (UNLIKELY(NEEDS_BARRIER(tv_, LDWU(tv_, 0), v_))) GENERIC;
	STQ(tv_ + ri_ * 8, 8, v_); IP += 2;
	if (op_ == 253) { DROP1; FETCH0; NEXT0; }
	FETCHSAME; NEXTSAME; }

#if HS == 0
case HC(225): {	/* extB; with a jump: the back jump of a loop, or a long jump */
	if (LIKELY(LDBU(IP, 2) == 237)) {
		int32_t off_ = (int32_t)LDBU(IP, 3) + ((int32_t)(signed char)LDBU(IP, 1) * 256);
		IP += 3 + off_;
		if (off_ + 2 < 0) {
			if (UNLIKELY(--ioHeartbeatPollCountdown < 0) || UNLIKELY(SP < (iaddr)GIV(stackLimit))) goto hotEvents;
		}
		FETCH0; NEXT0;
	}
	GENERIC; }
#endif
case HC(237): IP += 1 + (uint32_t)LDBU(IP, 1); FETCHSAME; NEXTSAME;
case HC(238): { sqInt b_ = TOPV;
	if (b_ == GIV(trueObj)) { DROP1; IP += 1 + (uint32_t)LDBU(IP, 1); FETCH0; NEXT0; }
	if (UNLIKELY(b_ != GIV(falseObj))) GENERIC;
	DROP1; IP += 1; FETCH0; NEXT0; }
case HC(239): { sqInt b_ = TOPV;
	if (b_ == GIV(falseObj)) { DROP1; IP += 1 + (uint32_t)LDBU(IP, 1); FETCH0; NEXT0; }
	if (UNLIKELY(b_ != GIV(trueObj))) GENERIC;
	DROP1; IP += 1; FETCH0; NEXT0; }

#undef SPILL
#undef TOPV
#undef SECV
#undef THIRDV
#undef DROP1
#undef DROP2
#undef DROP3
#undef FLUSH
#undef NEXT1
#undef NEXTSAME
#undef FETCH1
#undef FETCHSAME
#undef NEXT0
#undef GENERIC
#undef NOTNUM
#undef ARITHSLOW
#undef ARITHMIXED
#undef GENERICGOTO
#undef PUSHV
#undef HC
