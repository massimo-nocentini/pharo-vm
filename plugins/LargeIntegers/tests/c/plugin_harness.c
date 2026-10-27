/* plugin_harness.c: drives a build of LargeIntegers.c through a mock VM.
 *
 * Loads the plugin as a shared object (run-c-tests.sh compiles it with
 * -fsanitize=address,undefined when the compiler can) and calls its exported
 * primitives through a mock `struct VirtualMachine` whose objects are Spur-shaped:
 * byte bodies are malloc'ed separately at their 8-byte rounded size and zero-filled,
 * so ASan sees any access outside the body, including a read one word *below* it and
 * the read of slot 0 of a 0-byte object (Spur leaves that slot uninitialised; here it
 * is a 0-byte malloc, which ASan reports on any access).
 *
 * Modes (exit status 0 when every check holds):
 *   calls <plugin.so>     every call of the differential set (all primitives over a
 *                         pool of SmallIntegers, normalized, unnormalized and empty
 *                         LargeIntegers of 1 to 2100 bytes, wrong types; shifts,
 *                         anyBit, Montgomery; wrong-type arguments of those
 *                         three), one line each: '<n> <prim> <shape> => <answer>'.
 *                         run-c-tests.sh digests them and compares with
 *                         expected/mock-calls.sha256, recorded from the plugin as it
 *                         was before the kernels.  The shapes of `ub` are left out.
 *   diff <reference.so> <plugin.so>
 *                         the same calls on both, every mismatch printed.
 *   ub <plugin.so> [li01|li05|li06]
 *                         the shapes where the plugin used to write past an object
 *                         (li01: left shift of an unnormalized receiver), read below
 *                         two empty operands (li05: subtract) or read slot 0 of an
 *                         empty operand (li06: Montgomery); the answers must be the
 *                         shift of the normalized receiver, SmallInteger 0, and a
 *                         primitive failure.  The old plugin dies under ASan.
 *   oom <plugin.so>       fail the k-th allocation for k = 1, 2, ... in each call of a
 *                         call set; every call runs in a forked child.  Checks: no
 *                         crash, failure code PrimErrNoMemory, stack untouched,
 *                         remappable-oop stack balanced.
 *   arity <plugin.so>     every primitive at every wrong argument count 0..4, with the
 *                         caller frame filled with a word that is not an oop: must
 *                         fail with PrimErrBadNumArgs, the stack untouched, without
 *                         dereferencing that word.
 */
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <sys/wait.h>

#include "sqConfig.h"
#include "virtualMachine.h"

/* ------------------------------------------------------------------ objects */

enum { C_NIL, C_TRUE, C_FALSE, C_SMALLINT, C_LPI, C_LNI, C_ARRAY, C_STRING, C_BYTEARRAY, C_FLOAT, C_CLASS, C_NCLASSES };
static const char *clsName[] = { "nil", "true", "false", "SmallInteger", "LPI", "LNI", "Array", "String", "ByteArray", "Float", "Class" };

typedef struct MObj {
	int cls;            /* class id, or C_CLASS for class objects (id in n) */
	sqInt n;            /* bytes for byte objects, slots for pointer objects */
	unsigned char *body;
	struct MObj *next;  /* all-objects list */
} MObj;

static MObj *allObjs;
static MObj *classObj[C_NCLASSES];
static MObj *nilObj, *trueObj, *falseObj;

#define OOP(o) ((sqInt)(o))
#define OBJ(oop) ((MObj *)(oop))
#define SI(v) ((sqInt)((((usqInt)(sqInt)(v)) << 3) | 1))
#define SIVAL(oop) ((sqInt)(oop) >> 3)
#define MAXSI ((sqInt)0x0FFFFFFFFFFFFFFFLL)
#define MINSI (-MAXSI - 1)

static MObj *newObj(int cls, sqInt n, int pointers)
{
	MObj *o = calloc(1, sizeof(MObj));
	o->cls = cls;
	o->n = n;
	if (pointers) {
		sqInt *b = malloc((n ? n : 0) * sizeof(sqInt));
		for (sqInt i = 0; i < n; i++) b[i] = OOP(nilObj);
		o->body = (unsigned char *)b;
	} else {
		size_t r = (n + 7) & ~(size_t)7;
		o->body = malloc(r);          /* 0 bytes -> any access is reported */
		if (r) memset(o->body, 0, r);
	}
	o->next = allObjs;
	allObjs = o;
	return o;
}

static void freeAllObjs(MObj *keep)
{
	/* free everything allocated after `keep` (objects are pushed at the head) */
	while (allObjs && allObjs != keep) {
		MObj *o = allObjs;
		allObjs = o->next;
		free(o->body);
		free(o);
	}
}

/* ------------------------------------------------------------------ mock VM */

static sqInt stk[512];
static int sp;                 /* index of top of stack */
static int argCount;
static sqInt primFailCode;
static sqInt remap[64];
static int remapDepth, remapMax;
static long allocCount, failAllocAt;  /* fault injection: fail the failAllocAt-th allocation */
/* the arity run fills the caller frame with POISON, a word that is not an oop (like a
   frame-header word); any proxy call that would dereference it counts in poisonDerefs */
#define POISON ((sqInt)0x7FFF0010)
static sqInt fillerOop;
static int poisonDerefs;

static sqInt vMajor(void) { return VM_PROXY_MAJOR; }
static sqInt vMinor(void) { return VM_PROXY_MINOR; }
static sqInt vFailed(void) { return primFailCode != 0; }
static sqInt vPrimitiveFail(void) { if (!primFailCode) primFailCode = 1; return 0; }
static sqInt vPrimitiveFailFor(sqInt c) { return primFailCode = c; }
static sqInt vSuccess(sqInt b) { if (!b && !primFailCode) primFailCode = 1; return b; }
static sqInt vStackValue(sqInt off) { return stk[sp - off]; }
static sqInt vIsIntegerObject(sqInt oop) { return oop & 1; }
static sqInt vIntegerValueOf(sqInt oop) { return SIVAL(oop); }
static sqInt vIntegerObjectOf(sqInt v) { return SI(v); }
static sqInt vStackIntegerValue(sqInt off)
{
	sqInt oop = vStackValue(off);
	if (oop & 1) return SIVAL(oop);
	vPrimitiveFail();
	return 0;
}
static void vPopthenPush(sqInt n, sqInt oop) { sp -= n; stk[++sp] = oop; }
static sqInt vMethodArgumentCount(void) { return argCount; }
static sqInt vFetchClassOf(sqInt oop)
{
	if (oop == POISON) { poisonDerefs++; return OOP(classObj[C_NIL]); }
	if (oop & 1) return OOP(classObj[C_SMALLINT]);
	return OOP(classObj[OBJ(oop)->cls]);
}
static sqInt vClassLPI(void) { return OOP(classObj[C_LPI]); }
static sqInt vClassLNI(void) { return OOP(classObj[C_LNI]); }
static sqInt vClassArray(void) { return OOP(classObj[C_ARRAY]); }
static sqInt vClassString(void) { return OOP(classObj[C_STRING]); }
static sqInt vTrueObject(void) { return OOP(trueObj); }
static sqInt vFalseObject(void) { return OOP(falseObj); }
static sqInt vIsBooleanObject(sqInt oop) { return oop == OOP(trueObj) || oop == OOP(falseObj); }
static sqInt vBooleanValueOf(sqInt oop)
{
	if (oop == OOP(trueObj)) return 1;
	if (oop == OOP(falseObj)) return 0;
	vPrimitiveFail();
	return 0;
}
static void *vFirstIndexableField(sqInt oop)
{
	if (oop & 1 || oop == 0) { fprintf(stderr, "HARNESS: firstIndexableField(%#lx)\n", (long)oop); abort(); }
	if (oop == POISON) { poisonDerefs++; return nilObj; }
	return OBJ(oop)->body;
}
static sqInt vSlotSizeOf(sqInt oop)
{
	if (oop & 1) return 0;
	if (oop == 0) { fprintf(stderr, "HARNESS: slotSizeOf(0)\n"); abort(); }
	if (oop == POISON) { poisonDerefs++; return 0; }
	return OBJ(oop)->n;
}
static sqInt vInstantiate(sqInt classOop, sqInt size)
{
	MObj *c = OBJ(classOop);
	int id = (int)c->n;
	if (failAllocAt && ++allocCount == failAllocAt) {
		primFailCode = PrimErrNoMemory;   /* what Spur's instantiateClass:indexableSize: does */
		return 0;
	}
	return OOP(newObj(id, size, id == C_ARRAY));
}
static sqInt vStObjectatput(sqInt array, sqInt idx, sqInt value)
{
	MObj *o = OBJ(array);
	if (o->cls != C_ARRAY || idx < 1 || idx > o->n) { vPrimitiveFail(); return 0; }
	((sqInt *)o->body)[idx - 1] = value;
	return value;
}
static unsigned int vPositive32BitValueOf(sqInt oop)
{
	if (oop & 1) {
		sqInt v = SIVAL(oop);
		if (v >= 0 && v <= 0xFFFFFFFFLL) return (unsigned int)v;
		vPrimitiveFail();
		return 0;
	}
	if (oop == POISON) { poisonDerefs++; vPrimitiveFail(); return 0; }
	if (OBJ(oop)->cls == C_LPI && OBJ(oop)->n <= 4) {
		unsigned int v = 0;
		for (sqInt i = OBJ(oop)->n - 1; i >= 0; i--) v = (v << 8) | OBJ(oop)->body[i];
		return v;
	}
	vPrimitiveFail();
	return 0;
}
static void vPushRemappableOop(sqInt oop) { remap[remapDepth++] = oop; if (remapDepth > remapMax) remapMax = remapDepth; }
static sqInt vPopRemappableOop(void) { return remap[--remapDepth]; }

static struct VirtualMachine vm;

static void initVM(void)
{
	nilObj = calloc(1, sizeof(MObj)); nilObj->cls = C_NIL;
	trueObj = calloc(1, sizeof(MObj)); trueObj->cls = C_TRUE;
	falseObj = calloc(1, sizeof(MObj)); falseObj->cls = C_FALSE;
	for (int i = 0; i < C_NCLASSES; i++) {
		classObj[i] = calloc(1, sizeof(MObj));
		classObj[i]->cls = C_CLASS;
		classObj[i]->n = i;
	}
	vm.majorVersion = vMajor;
	vm.minorVersion = vMinor;
	vm.booleanValueOf = vBooleanValueOf;
	vm.classArray = vClassArray;
	vm.classLargeNegativeInteger = vClassLNI;
	vm.classLargePositiveInteger = vClassLPI;
	vm.classString = vClassString;
	vm.failed = vFailed;
	vm.falseObject = vFalseObject;
	vm.fetchClassOf = vFetchClassOf;
	vm.firstIndexableField = vFirstIndexableField;
	vm.instantiateClassindexableSize = vInstantiate;
	vm.integerObjectOf = vIntegerObjectOf;
	vm.integerValueOf = vIntegerValueOf;
	vm.isBooleanObject = vIsBooleanObject;
	vm.isIntegerObject = vIsIntegerObject;
	vm.popthenPush = vPopthenPush;
	vm.popRemappableOop = vPopRemappableOop;
	vm.positive32BitValueOf = vPositive32BitValueOf;
	vm.primitiveFail = vPrimitiveFail;
	vm.primitiveFailFor = vPrimitiveFailFor;
	vm.pushRemappableOop = vPushRemappableOop;
	vm.slotSizeOf = vSlotSizeOf;
	vm.stObjectatput = vStObjectatput;
	vm.stackIntegerValue = vStackIntegerValue;
	vm.stackValue = vStackValue;
	vm.success = vSuccess;
	vm.trueObject = vTrueObject;
	vm.methodArgumentCount = vMethodArgumentCount;
}

/* ------------------------------------------------------------------ plugins */

typedef struct { const char *name; void *h; } Plugin;
static Plugin plugins[2];      /* [0] the reference (diff only), [1] the plugin under test */

static void loadPlugin(Plugin *p, const char *name, const char *path)
{
	p->name = name;
	p->h = dlopen(path, RTLD_NOW | RTLD_LOCAL);
	if (!p->h) { fprintf(stderr, "dlopen %s: %s\n", path, dlerror()); exit(2); }
	sqInt (*setInterpreter)(struct VirtualMachine *) = (sqInt (*)(struct VirtualMachine *))dlsym(p->h, "setInterpreter");
	if (!setInterpreter || !setInterpreter(&vm)) { fprintf(stderr, "setInterpreter failed for %s\n", path); exit(2); }
}

static const struct { const char *prim; int nargs; } prims[] = {
	{ "primAnyBitFromTo", 2 }, { "primDigitAdd", 1 }, { "primDigitBitAnd", 1 }, { "primDigitBitOr", 1 },
	{ "primDigitBitShiftMagnitude", 1 }, { "primDigitBitXor", 1 }, { "primDigitCompare", 1 },
	{ "primDigitDivNegative", 2 }, { "primDigitMultiplyNegative", 2 }, { "primDigitSubtract", 1 },
	{ "primGetModuleName", 0 }, { "primMontgomeryDigitLength", 0 }, { "primMontgomeryTimesModulo", 3 },
	{ "primNormalizeNegative", 0 }, { "primNormalizePositive", 0 },
};
#define NPRIMS ((int)(sizeof prims / sizeof prims[0]))

/* ------------------------------------------------------------------ operands */

/* An operand spec, materialised afresh for every call so the two plugins never share objects. */
typedef struct { int kind; sqInt v; int n; unsigned char bytes[40000]; } Spec;
enum { K_SI, K_LPI, K_LNI, K_NIL, K_TRUE, K_FALSE, K_FLOAT, K_BYTES /* a ByteArray: bytes, not an integer */ };

static sqInt materialise(const Spec *s)
{
	MObj *o;
	switch (s->kind) {
	case K_SI: return SI(s->v);
	case K_NIL: return OOP(nilObj);
	case K_TRUE: return OOP(trueObj);
	case K_FALSE: return OOP(falseObj);
	case K_FLOAT: o = newObj(C_FLOAT, 8, 0); return OOP(o);
	case K_BYTES:
		o = newObj(C_BYTEARRAY, s->n, 0);
		if (s->n) memcpy(o->body, s->bytes, s->n);
		return OOP(o);
	default:
		o = newObj(s->kind == K_LPI ? C_LPI : C_LNI, s->n, 0);
		if (s->n) memcpy(o->body, s->bytes, s->n);
		return OOP(o);
	}
}

static uint64_t rs = 0x9E3779B97F4A7C15ull;
static uint64_t rnd(void) { rs ^= rs >> 12; rs ^= rs << 25; rs ^= rs >> 27; return rs * 0x2545F4914F6CDD1Dull; }
static uint32_t patDigit(void)
{
	static const uint32_t pats[] = { 0, 1, 0xFFFFFFFFu, 0xFFFFFFFEu, 0x80000000u, 0x7FFFFFFFu };
	unsigned k = rnd() % 10;
	return k < 6 ? pats[k] : (uint32_t)rnd();
}

static Spec *spec(int kind, int n, int mode /* 0 rnd 1 pat 2 zero */, int topNonZero)
{
	Spec *s = calloc(1, sizeof(Spec));
	s->kind = kind;
	s->n = n;
	for (int i = 0; i < n; i += 4) {
		uint32_t d = mode == 0 ? (uint32_t)rnd() : mode == 1 ? patDigit() : 0;
		for (int k = 0; k < 4 && i + k < n; k++) s->bytes[i + k] = (unsigned char)(d >> (8 * k));
	}
	if (topNonZero && n && !s->bytes[n - 1]) s->bytes[n - 1] = 1 + rnd() % 255;
	return s;
}
static Spec *specSI(sqInt v) { Spec *s = calloc(1, sizeof(Spec)); s->kind = K_SI; s->v = v; return s; }
static Spec *specK(int kind) { Spec *s = calloc(1, sizeof(Spec)); s->kind = kind; return s; }
static Spec *specUnnorm(int kind, uint64_t v, int lead)
{
	Spec *s = calloc(1, sizeof(Spec));
	int n = 0;
	s->kind = kind;
	while (v) { s->bytes[n++] = (unsigned char)v; v >>= 8; }
	s->n = n + lead;
	return s;
}

static int digitLen(const Spec *s)
{
	if (s->kind == K_SI) { uint64_t m = s->v < 0 ? (uint64_t)0 - (uint64_t)s->v : (uint64_t)s->v; return m > 0xFFFFFFFFu ? 2 : 1; }
	return (s->n + 3) / 4;
}
static int isIntSpec(const Spec *s) { return s->kind == K_SI || s->kind == K_LPI || s->kind == K_LNI; }

/* the li01 shape: the old digit:Lshift: writes past the new object (see runUB) */
static int lshiftOverflows(const Spec *s, sqInt shift)
{
	int h = 0;
	if (s->kind != K_LPI && s->kind != K_LNI) return 0;
	if (shift < 0 || shift % 32 == 0) return 0;
	for (int i = s->n; i > 0; i--)
		if (s->bytes[i - 1]) { h = (i - 1) * 8 + (32 - __builtin_clz(s->bytes[i - 1])); break; }
	if (!h) return 0;
	sqInt newByteLen = (h + shift + 7) / 8;
	sqInt cap = (newByteLen + 7) / 8 * 2;
	return digitLen(s) + shift / 32 > cap;
}

/* ------------------------------------------------------------------ calls */

typedef struct { char text[512]; int crashedOrBad; } Outcome;

static void describe(sqInt r, sqInt rcv, char *buf, size_t len)
{
	size_t k = 0;
	if (r & 1) { snprintf(buf, len, "SI %ld", (long)SIVAL(r)); return; }
	if (r == 0) { snprintf(buf, len, "OOP0"); return; }
	MObj *o = OBJ(r);
	if (o->cls == C_ARRAY) {
		char a[200], b[200];
		describe(((sqInt *)o->body)[0], rcv, a, sizeof a);
		describe(o->n > 1 ? ((sqInt *)o->body)[1] : OOP(nilObj), rcv, b, sizeof b);
		snprintf(buf, len, "Array%ld{%s ; %s}", (long)o->n, a, b);
		return;
	}
	if (o->cls == C_TRUE || o->cls == C_FALSE || o->cls == C_NIL) { snprintf(buf, len, "%s", clsName[o->cls]); return; }
	k += snprintf(buf + k, len - k, "%s%s %ld ", r == rcv ? "ID " : "", clsName[o->cls], (long)o->n);
	if (o->cls == C_STRING) { snprintf(buf + k, len - k, "'%.*s'", (int)o->n, o->body); return; }
	if (o->n <= 48) {
		for (sqInt i = 0; i < o->n && k + 3 < len; i++) k += snprintf(buf + k, len - k, "%02x", o->body[i]);
	} else {
		uint64_t h = 1469598103934665603ull;
		for (sqInt i = 0; i < o->n; i++) h = (h ^ o->body[i]) * 1099511628211ull;
		snprintf(buf + k, len - k, "h%016llx", (unsigned long long)h);
	}
}

/* When set, the first argument is the receiver's object itself, not a copy (x * x). */
static int sameObject;

/* Call `prim` of plugin p with receiver and args materialised from specs.
   argc < 0 means the true arity.  Returns a textual outcome. */
static void callPrim(Plugin *p, const char *prim, const Spec *rcv, const Spec **args, int nargs, int argc, Outcome *out)
{
	MObj *mark = allObjs;
	sqInt (*fn)(void) = (sqInt (*)(void))dlsym(p->h, prim);
	int base, i;
	sqInt rcvOop;
	if (!fn) { snprintf(out->text, sizeof out->text, "NOPRIM"); out->crashedOrBad = 1; return; }
	for (i = 0; i < 16; i++) stk[i] = fillerOop ? fillerOop : OOP(nilObj);   /* caller frame filler */
	sp = 15;
	base = sp;
	rcvOop = materialise(rcv);
	stk[++sp] = rcvOop;
	for (i = 0; i < nargs; i++) stk[++sp] = i == 0 && sameObject ? rcvOop : materialise(args[i]);
	argCount = argc < 0 ? nargs : argc;
	primFailCode = 0;
	remapDepth = remapMax = 0;
	out->crashedOrBad = 0;
	fn();
	if (primFailCode) {
		int k = snprintf(out->text, sizeof out->text, "FAIL %ld", (long)primFailCode);
		if (sp != base + nargs + 1) { snprintf(out->text + k, sizeof out->text - k, " STACK-MOVED(%d)", sp - base); out->crashedOrBad = 1; }
	} else if (sp != base + nargs + 1 - argCount) {
		snprintf(out->text, sizeof out->text, "UNBALANCED sp=%d", sp - base);
		out->crashedOrBad = 1;
	} else {
		describe(stk[sp], rcvOop, out->text, sizeof out->text);
	}
	if (remapDepth) {
		size_t k = strlen(out->text);
		snprintf(out->text + k, sizeof out->text - k, " REMAP-LEAK(%d)", remapDepth);
	}
	freeAllObjs(mark);
}

/* ------------------------------------------------------------------ pools */

static Spec *pool[400];  /* sizes above: 2 per size + SmallIntegers + specials */
static int npool;

static void buildPool(void)
{
	static const sqInt sis[] = { 0, 1, -1, 7, -255, 256, 65535, -65536, 0xFFFFFF, 0x1000000, 0xFFFFFFFFLL,
		-0x100000000LL, 0xFFFFFFFFFFLL, 1LL << 56, MAXSI, MINSI, MAXSI - 1, MINSI + 1 };
	static const int sizes[] = { 1, 2, 3, 4, 5, 7, 8, 9, 12, 13, 16, 17, 20, 24, 31, 32, 33, 48, 64, 100, 257,
		/* around the 128-digit stack buffers of the plugin (LIStackDigits), and beyond:
		   Karatsuba (from 48 digits) and malloc'ed kernel scratch */
		253, 256, 505, 508, 509, 512, 513, 1030, 2100 };
	for (unsigned i = 0; i < sizeof sis / sizeof sis[0]; i++) pool[npool++] = specSI(sis[i]);
	for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
		pool[npool++] = spec(K_LPI, sizes[i], rnd() & 1, 1);
		pool[npool++] = spec(K_LNI, sizes[i], rnd() & 1, 1);
	}
	pool[npool++] = specUnnorm(K_LPI, 1, 1);
	pool[npool++] = specUnnorm(K_LNI, 5, 3);
	pool[npool++] = specUnnorm(K_LPI, 0xABCDEF, 4);
	pool[npool++] = specUnnorm(K_LPI, 0x12345678, 5);
	pool[npool++] = specUnnorm(K_LNI, 0x1122334455ull, 8);
	pool[npool++] = specUnnorm(K_LPI, 7, 12);
	pool[npool++] = specUnnorm(K_LPI, 0, 0);   /* zero-length */
	pool[npool++] = specUnnorm(K_LNI, 0, 0);
	pool[npool++] = specUnnorm(K_LPI, 0, 1);
	pool[npool++] = specUnnorm(K_LNI, 0, 8);
	pool[npool++] = specK(K_NIL);
	pool[npool++] = specK(K_FLOAT);
}

/* calls mode: print every answer of plugins[1]; diff mode: compare it with plugins[0] */
static int diffMode;
static long callNo;

static int diffOne(const char *label, const char *prim, const Spec *rcv, const Spec **args, int nargs)
{
	Outcome a, b;
	callNo++;
	callPrim(&plugins[1], prim, rcv, args, nargs, -1, &b);
	if (!diffMode) {
		printf("%ld %s %s => %s\n", callNo, prim, label, b.text);
		return b.crashedOrBad;
	}
	callPrim(&plugins[0], prim, rcv, args, nargs, -1, &a);
	if (strcmp(a.text, b.text) || a.crashedOrBad || b.crashedOrBad) {
		printf("MISMATCH %ld %s %s\n  reference: %s\n  plugin:    %s\n", callNo, label, prim, a.text, b.text);
		return 1;
	}
	return 0;
}

static void note(const char *s)
{
	if (!diffMode) printf("# %s\n", s);
}

static uint32_t montInv(uint32_t m0)
{
	uint32_t x = 1;
	for (int i = 0; i < 5; i++) x *= 2 - m0 * x;
	return (uint32_t)0 - x;
}

static int runDiff(void)
{
	long calls = 0;
	int bad = 0;
	Spec *t = specK(K_TRUE), *f = specK(K_FALSE);
	static const sqInt shifts[] = { 0, 1, 5, 31, 32, 33, 64, 95, 200, -1, -5, -31, -32, -33, -64, -200, -100000 };
	static const sqInt ranges[][2] = { { 1, 1 }, { 1, 8 }, { 1, 32 }, { 5, 40 }, { 33, 64 }, { 31, 33 }, { 1, 1000 }, { 64, 1 }, { 0, 3 }, { 3, 0 } };
	buildPool();
	note("pairs, shifts, anyBit, normalize, module name");
	for (int i = 0; i < npool; i++) {
		for (int j = 0; j < npool; j++) {
			const Spec *b1[1] = { pool[j] };
			const Spec *b2t[2] = { pool[j], t }, *b2f[2] = { pool[j], f };
			bad += diffOne("pair", "primDigitAdd", pool[i], b1, 1);
			/* the li05 shape: two zero-length operands make the old plugin read below both */
			if (!(isIntSpec(pool[i]) && pool[i]->kind != K_SI && pool[i]->n == 0
			      && isIntSpec(pool[j]) && pool[j]->kind != K_SI && pool[j]->n == 0))
				bad += diffOne("pair", "primDigitSubtract", pool[i], b1, 1);
			bad += diffOne("pair", "primDigitBitAnd", pool[i], b1, 1);
			bad += diffOne("pair", "primDigitBitOr", pool[i], b1, 1);
			bad += diffOne("pair", "primDigitBitXor", pool[i], b1, 1);
			bad += diffOne("pair", "primDigitCompare", pool[i], b1, 1);
			bad += diffOne("pair", "primDigitMultiplyNegative", pool[i], b2t, 2);
			bad += diffOne("pair", "primDigitMultiplyNegative", pool[i], b2f, 2);
			bad += diffOne("pair", "primDigitDivNegative", pool[i], b2t, 2);
			bad += diffOne("pair", "primDigitDivNegative", pool[i], b2f, 2);
			calls += 10;
		}
		for (unsigned s = 0; s < sizeof shifts / sizeof shifts[0]; s++) {
			Spec *sh = specSI(shifts[s]);
			const Spec *a1[1] = { sh };
			if (!lshiftOverflows(pool[i], shifts[s])) { bad += diffOne("shift", "primDigitBitShiftMagnitude", pool[i], a1, 1); calls++; }
			free(sh);
		}
		for (unsigned r = 0; r < sizeof ranges / sizeof ranges[0]; r++) {
			Spec *x = specSI(ranges[r][0]), *y = specSI(ranges[r][1]);
			const Spec *a2[2] = { x, y };
			bad += diffOne("anyBit", "primAnyBitFromTo", pool[i], a2, 2);
			calls++;
			free(x); free(y);
		}
		bad += diffOne("norm", "primNormalizePositive", pool[i], NULL, 0);
		bad += diffOne("norm", "primNormalizeNegative", pool[i], NULL, 0);
		bad += diffOne("misc", "primMontgomeryDigitLength", pool[i], NULL, 0);
		bad += diffOne("misc", "primGetModuleName", pool[i], NULL, 0);
		calls += 4;
	}
	note("montgomery");
	static const int montKs[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
		127, 128, 129, 140 /* around the 128-digit stack buffer */ };
	for (unsigned ki = 0; ki < sizeof montKs / sizeof montKs[0]; ki++) {
		int k = montKs[ki];
		for (int rep = 0; rep < 8; rep++) {
			Spec *m = spec(K_LPI, 4 * k, 0, 1);
			m->bytes[0] |= 1;
			uint32_t m0 = m->bytes[0] | m->bytes[1] << 8 | m->bytes[2] << 16 | (uint32_t)m->bytes[3] << 24;
			Spec *inv = specSI(montInv(m0));
			Spec *a = spec(K_LPI, 1 + rnd() % (4 * k), rep & 1, 1);
			Spec *b = spec(K_LPI, 1 + rnd() % (4 * k), 1, 1);
			Spec *z = spec(K_LPI, 0, 0, 0);
			const Spec *args[3] = { b, m, inv };
			bad += diffOne("mont", "primMontgomeryTimesModulo", a, args, 3);
			args[0] = a;
			bad += diffOne("mont", "primMontgomeryTimesModulo", b, args, 3);
			args[0] = b;
			bad += diffOne("mont0", "primMontgomeryTimesModulo", z, args, 3);
			args[0] = z;
			bad += diffOne("mont00", "primMontgomeryTimesModulo", z, args, 3);
			calls += 4;
			free(m); free(inv); free(a); free(b); free(z);
		}
	}
	note("wrong-type arguments");
	{
		/* the receivers and pair arguments above come from the pool, which has wrong
		   types; these are the other arguments: the shift count, anyBit's bounds, and
		   Montgomery's three */
		Spec *bytes = spec(K_LPI, 9, 0, 1);
		bytes->kind = K_BYTES;
		Spec *wrong[] = { specK(K_NIL), specK(K_TRUE), bytes, specK(K_FLOAT), spec(K_LNI, 9, 0, 1),
			spec(K_LPI, 9, 0, 1), spec(K_LPI, 5, 1, 1), specSI(-5), specSI(-0x100000000LL), specSI(0x100000000LL) };
		const int nwrong = (int)(sizeof wrong / sizeof wrong[0]);
		Spec *rcvs[] = { spec(K_LPI, 9, 0, 1), spec(K_LNI, 13, 1, 1), specSI(12345) };
		Spec *one = specSI(1), *eight = specSI(8);
		for (unsigned r = 0; r < sizeof rcvs / sizeof rcvs[0]; r++)
			for (int w = 0; w < nwrong; w++) {
				const Spec *a1[1] = { wrong[w] }, *lo[2] = { wrong[w], eight }, *hi[2] = { one, wrong[w] };
				/* the SmallIntegers among them are right for a shift and anyBit, and done above */
				if (wrong[w]->kind == K_SI) continue;
				bad += diffOne("shift", "primDigitBitShiftMagnitude", rcvs[r], a1, 1);
				bad += diffOne("anyBit", "primAnyBitFromTo", rcvs[r], lo, 2);
				bad += diffOne("anyBit", "primAnyBitFromTo", rcvs[r], hi, 2);
				calls += 3;
			}
		/* Montgomery: a right call (a * b mod m, 3 digits), then each argument and the
		   receiver in turn replaced by every wrong value */
		Spec *m = spec(K_LPI, 12, 0, 1);
		m->bytes[0] |= 1;
		uint32_t m0 = m->bytes[0] | m->bytes[1] << 8 | m->bytes[2] << 16 | (uint32_t)m->bytes[3] << 24;
		Spec *inv = specSI(montInv(m0)), *a = spec(K_LPI, 11, 0, 1), *b = spec(K_LPI, 7, 1, 1);
		const Spec *args[3] = { b, m, inv };
		bad += diffOne("mont", "primMontgomeryTimesModulo", a, args, 3);
		calls++;
		for (int w = 0; w < nwrong; w++) {
			bad += diffOne("mont-receiver", "primMontgomeryTimesModulo", wrong[w], args, 3);
			calls++;
			for (int pos = 0; pos < 3; pos++) {
				static const char *label[] = { "mont-operand", "mont-modulus", "mont-inverse" };
				const Spec *wargs[3] = { b, m, inv };
				wargs[pos] = wrong[w];
				bad += diffOne(label[pos], "primMontgomeryTimesModulo", a, wargs, 3);
				calls++;
			}
		}
		for (int w = 0; w < nwrong; w++) free(wrong[w]);
		for (unsigned r = 0; r < sizeof rcvs / sizeof rcvs[0]; r++) free(rcvs[r]);
		free(one); free(eight); free(m); free(inv); free(a); free(b);
	}
	if (!diffMode) printf("# done %ld calls\n", calls);

	/* x * x as the image sends it, e.g. from #squared: the receiver again as the argument,
	   one object, so that the plugin multiplies the same digits by themselves (the squaring
	   kernels; the pairs above pass equal digits in two objects).  Last, so that the calls
	   above and their digests stay as they were recorded. */
	long sameCalls = 0;
	note("same object: x * x");
	{
		/* byte sizes around the squaring thresholds: 3, 44 and 88 64-bit limbs (24, 352, 704
		   bytes), 8, 48 and 96 32-bit limbs (32, 192, 384 bytes), a digit and a byte off */
		static const int centres[] = { 24, 32, 192, 352, 384, 704 }, deltas[] = { -5, -4, -1, 0, 1, 4, 5 };
		static const int large[] = { 1500, 4001, 8000, 40000 };
		static const uint32_t tops[] = { 0xFFFFFFFFu, 0x80000000u, 1 };
		Spec *specs[96];
		int ns = 0;
		for (unsigned c = 0; c < sizeof centres / sizeof centres[0]; c++)
			for (unsigned d = 0; d < sizeof deltas / sizeof deltas[0]; d++) {
				/* one draw per statement: the arguments of a call are evaluated in no fixed order */
				int kind = rnd() & 1 ? K_LNI : K_LPI;
				int mode = (int)(rnd() % 2);
				specs[ns++] = spec(kind, centres[c] + deltas[d], mode, 1);
			}
		for (unsigned i = 0; i < sizeof large / sizeof large[0]; i++)
			specs[ns++] = spec(K_LPI, large[i], 0, 1);
		/* top digit all ones, 0x80000000 or 1; all ones throughout */
		for (unsigned c = 0; c < sizeof centres / sizeof centres[0]; c++)
			for (unsigned k = 0; k < 4; k++) {
				Spec *s = spec(K_LPI, centres[c], 0, 1);
				if (k == 3) memset(s->bytes, 0xFF, s->n);
				else for (int b = 0; b < 4; b++) s->bytes[s->n - 4 + b] = (unsigned char)(tops[k] >> (8 * b));
				specs[ns++] = s;
			}
		sameObject = 1;
		for (int i = 0; i < npool + ns; i++) {
			/* the pool too: SmallIntegers, empty and unnormalized LargeIntegers, wrong types */
			const Spec *x = i < npool ? pool[i] : specs[i - npool];
			const Spec *a2t[2] = { x, t }, *a2f[2] = { x, f };
			bad += diffOne("same", "primDigitMultiplyNegative", x, a2t, 2);
			bad += diffOne("same", "primDigitMultiplyNegative", x, a2f, 2);
			sameCalls += 2;
		}
		sameObject = 0;
		for (int i = 0; i < ns; i++) free(specs[i]);
	}
	if (diffMode) printf("diff: %ld calls, %d mismatches\n", calls + sameCalls, bad);
	else printf("# done %ld same-object calls\n", sameCalls);
	return bad ? 1 : 0;
}

/* ------------------------------------------------------------------ UB shapes */

/* Each shape prints its answer and checks it; under ASan the old plugin dies instead. */
static int ubExpect(const char *what, const Outcome *o, const char *want, int prefixOnly)
{
	int ok = !o->crashedOrBad && (prefixOnly ? !strncmp(o->text, want, strlen(want)) : !strcmp(o->text, want));
	printf("%s %s => %s (%s %s)\n", ok ? "ok  " : "FAIL", what, o->text, prefixOnly ? "expected a" : "expected", want);
	return !ok;
}

static int runUB(const char *which, Plugin *p)
{
	Outcome o, want;
	int bad = 0, all = !which;
	if (all || !strcmp(which, "li01")) {
		/* 1 with 8 (400) zero bytes on top, shifted by 1: the old plugin shifts every
		   digit of the receiver into the new 1-slot object, past its end.  The answer must be that of the
		   normalized receiver. */
		Spec *s = specSI(1), *one = specUnnorm(K_LPI, 1, 0);
		const Spec *a[1] = { s };
		callPrim(p, "primDigitBitShiftMagnitude", one, a, 1, -1, &want);
		for (int lead = 8; lead <= 400; lead += 392) {
			char what[64];
			Spec *x = specUnnorm(K_LPI, 1, lead);
			callPrim(p, "primDigitBitShiftMagnitude", x, a, 1, -1, &o);
			snprintf(what, sizeof what, "li01: P%d(1 + %d zero bytes) shift 1", lead + 1, lead);
			bad += ubExpect(what, &o, want.text, 0);
			free(x);
		}
		bad += ubExpect("li01: P1(1) shift 1", &want, "LPI 1 02", 0);
		free(s); free(one);
	}
	if (all || !strcmp(which, "li05")) {
		/* two empty operands: the old plugin reads the word below both */
		Spec *z1 = specUnnorm(K_LPI, 0, 0), *z2 = specUnnorm(K_LNI, 0, 0);
		const Spec *a[1] = { z2 };
		callPrim(p, "primDigitSubtract", z1, a, 1, -1, &o);
		bad += ubExpect("li05: P0 - N0", &o, "SI 0", 0);
		free(z1); free(z2);
	}
	if (all || !strcmp(which, "li06")) {
		/* an empty second operand: the old plugin reads its slot 0, which Spur never
		   initialises; the primitive must fail instead */
		Spec *a = specUnnorm(K_LPI, 0x12345, 0), *e = specUnnorm(K_LPI, 0, 0), *m = specUnnorm(K_LPI, 0xFFFFFFFB, 0);
		Spec *inv = specSI(montInv(0xFFFFFFFB));
		const Spec *args[3] = { e, m, inv };
		callPrim(p, "primMontgomeryTimesModulo", a, args, 3, -1, &o);
		bad += ubExpect("li06: P3 * P0 mod P4", &o, "FAIL ", 1);
		free(a); free(e); free(m); free(inv);
	}
	if (!all && strcmp(which, "li01") && strcmp(which, "li05") && strcmp(which, "li06")) {
		fprintf(stderr, "unknown UB case %s\n", which);
		return 2;
	}
	printf("ub %s: %d wrong answers\n", p->name, bad);
	return bad ? 1 : 0;
}

/* ------------------------------------------------------------------ OOM */

typedef struct { const char *label; const char *prim; Spec *rcv; Spec *args[3]; int nargs; } Case;

static int runCaseForked(Plugin *p, Case *c, long k, Outcome *o)
{
	int fds[2], status;
	pid_t pid;
	if (pipe(fds)) { perror("pipe"); exit(2); }
	fflush(stdout);
	pid = fork();
	if (pid == 0) {
		close(fds[0]);
		failAllocAt = k;
		allocCount = 0;
		callPrim(p, c->prim, c->rcv, (const Spec **)c->args, c->nargs, -1, o);
		/* report whether the injected failure was actually reached */
		snprintf(o->text + strlen(o->text), sizeof o->text - strlen(o->text), "%s", allocCount >= k ? "" : " (no-inject)");
		if (write(fds[1], o, sizeof *o) != sizeof *o) _exit(3);
		_exit(0);
	}
	close(fds[1]);
	memset(o, 0, sizeof *o);
	ssize_t got = read(fds[0], o, sizeof *o);
	close(fds[0]);
	waitpid(pid, &status, 0);
	if (!WIFEXITED(status) || WEXITSTATUS(status) != 0 || got != (ssize_t)sizeof *o) {
		snprintf(o->text, sizeof o->text, "CRASH(%s %d)", WIFSIGNALED(status) ? "signal" : "exit",
			WIFSIGNALED(status) ? WTERMSIG(status) : WEXITSTATUS(status));
		o->crashedOrBad = 1;
		return 1;
	}
	return 0;
}

static int runOOM(Plugin *p)
{
	Spec *s7 = specSI(7), *s3 = specSI(3), *big = spec(K_LPI, 40, 0, 1), *bigN = spec(K_LNI, 40, 0, 1);
	Spec *t = specK(K_TRUE), *one = specSI(1), *m = specUnnorm(K_LPI, 0xFFFFFFFB, 0), *inv = specSI(montInv(0xFFFFFFFB));
	Spec *ffff = spec(K_LPI, 8, 0, 1), *s33 = specSI(33), *sm5 = specSI(-5), *s1 = specSI(1), *s64 = specSI(64), *s100 = specSI(100);
	memset(ffff->bytes, 0xFF, 8);
	Spec *big600 = spec(K_LPI, 600, 0, 1), *bigN600 = spec(K_LNI, 600, 0, 1), *big1200 = spec(K_LPI, 1200, 0, 1);
	Spec *ff600 = spec(K_LPI, 600, 0, 1), *mBig = spec(K_LPI, 4 * 140, 0, 1);
	memset(ff600->bytes, 0xFF, 600);
	mBig->bytes[0] |= 1;
	Spec *invBig = specSI(montInv(mBig->bytes[0] | mBig->bytes[1] << 8 | mBig->bytes[2] << 16 | (uint32_t)mBig->bytes[3] << 24));
	Case cases[] = {
		{ "SI+SI", "primDigitAdd", s7, { s3 }, 1 },
		{ "L+SI", "primDigitAdd", big, { s7 }, 1 },
		{ "carry", "primDigitAdd", ffff, { s1 }, 1 },
		{ "SI-L", "primDigitSubtract", s7, { bigN }, 1 },
		{ "SI&SI", "primDigitBitAnd", s7, { s3 }, 1 },
		{ "L|SI", "primDigitBitOr", big, { s3 }, 1 },
		{ "SI^L", "primDigitBitXor", s3, { big }, 1 },
		{ "SI<<33", "primDigitBitShiftMagnitude", s7, { s33 }, 1 },
		{ "L>>5", "primDigitBitShiftMagnitude", big, { sm5 }, 1 },
		{ "SI*SI", "primDigitMultiplyNegative", s7, { s3, t }, 2 },
		{ "L*L", "primDigitMultiplyNegative", big, { bigN, t }, 2 },
		{ "SI/SI", "primDigitDivNegative", s7, { s3, t }, 2 },
		{ "L/SI", "primDigitDivNegative", big, { s7, t }, 2 },
		{ "SI/L", "primDigitDivNegative", s7, { big, t }, 2 },
		{ "anyBit", "primAnyBitFromTo", s7, { s1, s64 }, 2 },
		{ "anyBitL", "primAnyBitFromTo", big, { s1, s100 }, 2 },
		{ "mont", "primMontgomeryTimesModulo", s7, { s3, m, inv }, 3 },
		{ "normPos", "primNormalizePositive", ffff, { NULL }, 0 },
		{ "moduleName", "primGetModuleName", s7, { NULL }, 0 },
		/* results above the 128-digit stack buffers are computed into a LargeInteger */
		{ "BIG+SI", "primDigitAdd", big600, { s7 }, 1 },
		{ "BIG+BIG(carry)", "primDigitAdd", ff600, { ff600 }, 1 },
		{ "SI-BIG", "primDigitSubtract", s7, { bigN600 }, 1 },
		{ "BIG&BIG", "primDigitBitAnd", big600, { big600 }, 1 },
		{ "BIG*SI", "primDigitMultiplyNegative", big600, { s7, t }, 2 },
		{ "BIG>>5", "primDigitBitShiftMagnitude", big600, { sm5 }, 1 },
		{ "BIG/SI", "primDigitDivNegative", big600, { s7, t }, 2 },
		{ "BIG/BIG", "primDigitDivNegative", big1200, { big600, t }, 2 },
		{ "montBIG", "primMontgomeryTimesModulo", s7, { s3, mBig, invBig }, 3 },
	};
	int bad = 0;
	for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {
		for (long k = 1; k < 50; k++) {
			Outcome o;
			runCaseForked(p, &cases[i], k, &o);
			printf("oom %s %-10s k=%ld => %s\n", p->name, cases[i].label, k, o.text);
			if (o.crashedOrBad || (!strstr(o.text, "no-inject") && strncmp(o.text, "FAIL 9", 6))) bad++;
			if (strstr(o.text, "no-inject")) break;
		}
	}
	printf("oom %s: %d bad outcomes\n", p->name, bad);
	return bad ? 1 : 0;
}

/* ------------------------------------------------------------------ arity */

static int runArity(Plugin *p)
{
	/* every wrong count 0..4, receivers LargeInteger / SmallInteger / nil, the caller
	   frame filled with POISON; a clean failure is PrimErrBadNumArgs (5) with the stack
	   untouched and no dereference of a POISON word */
	Spec *ops[4] = { specSI(7), specSI(3), specSI(5), specSI(9) };
	Spec *rcvs[3] = { spec(K_LPI, 12, 0, 1), specSI(11), specK(K_NIL) };
	int bad = 0;
	fillerOop = POISON;
	for (int i = 0; i < NPRIMS; i++) {
		int n = prims[i].nargs;
		for (int r = 0; r < 3; r++)
		for (int argc = 0; argc <= 4; argc++) {
			Outcome o;
			if (argc == n) continue;
			poisonDerefs = 0;
			/* push argc arguments, but tell the primitive nothing else: it reads its own arity */
			callPrim(p, prims[i].prim, rcvs[r], (const Spec **)ops, argc, argc, &o);
			printf("arity %s %-28s n=%d argc=%d rcv=%d => %s%s\n", p->name, prims[i].prim, n, argc, r, o.text,
				poisonDerefs ? " POISON-DEREF" : "");
			if (strcmp(o.text, "FAIL 5") || o.crashedOrBad || poisonDerefs) bad++;
		}
	}
	fillerOop = 0;
	printf("arity %s: %d calls not failing with PrimErrBadNumArgs, stack intact, no frame-word dereference\n", p->name, bad);
	return bad ? 1 : 0;
}

static void usage(const char *me)
{
	fprintf(stderr, "usage: %s calls <plugin.so> | diff <reference.so> <plugin.so> | ub <plugin.so> [li01|li05|li06]\n"
		"       | oom <plugin.so> | arity <plugin.so>\n", me);
	exit(2);
}

int main(int argc, char **argv)
{
	if (argc < 3) usage(argv[0]);
	setvbuf(stdout, NULL, _IOLBF, 0);
	initVM();
	if (!strcmp(argv[1], "diff")) {
		if (argc != 4) usage(argv[0]);
		loadPlugin(&plugins[0], "reference", argv[2]);
		loadPlugin(&plugins[1], "plugin", argv[3]);
		diffMode = 1;
		return runDiff();
	}
	loadPlugin(&plugins[1], "plugin", argv[2]);
	if (!strcmp(argv[1], "calls") && argc == 3) return runDiff();
	if (!strcmp(argv[1], "ub") && argc <= 4) return runUB(argc == 4 ? argv[3] : NULL, &plugins[1]);
	if (!strcmp(argv[1], "oom") && argc == 3) return runOOM(&plugins[1]);
	if (!strcmp(argv[1], "arity") && argc == 3) return runArity(&plugins[1]);
	usage(argv[0]);
	return 2;
}
