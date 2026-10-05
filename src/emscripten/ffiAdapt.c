/* ffiAdapt.c -- the callouts of the FFI whose declaration differs from the
 * function in width (Emscripten)
 *
 * libffi's ffi_call is JavaScript: it calls the function from JavaScript,
 * which converts the arguments and the result by the types of the
 * function's own WebAssembly signature, and so throws where the image's
 * declaration has another width than C (a TypeError: a BigInt for an i32,
 * a Number for an i64).  On x86-64 the same declaration works: an int
 * passed for a long fills the register, a long passed for an int is read
 * in its low half.  So the declarations of the images do not always have
 * the widths of C, and work natively:
 *
 *  - cairo_pattern_set_extend(self, ulong) for an enum, an int;
 *  - FT_Error, an int, declared long, as the result of FreeType's functions;
 *  - git_tree_entrycount declared int, for a size_t;
 *  - a pointer passed for an int.
 *
 * emscriptenAdaptedFFICall, which emscriptenFFICall (emscriptenSupport.c)
 * calls for every callout of the interpreter, makes such a callout as an
 * x86-64 caller would, from the signature of the function in the registry
 * of the libraries (include/pharovm/emscripten/ffiRegistry.h, the field
 * signature, which cmake/emscripten/ffiSignatures.mjs reads from the headers
 * of each library: 'vji' for void (void *, int), 'ijjj.2' for sprintf, of 2
 * fixed arguments).  The declaration (the cif) and the signature are
 * compared by class: i (8, 16 and 32-bit integers), j (64-bit integers and
 * pointers), f, d and v (void).  Only differences between i and j are
 * adapted, on a cif prepared for the call with the signature's types:
 *
 *  - an argument declared j where C has i is narrowed to its low 32 bits;
 *  - an argument declared i where C has j is widened, signed or unsigned as
 *    declared;
 *  - a result of C i declared j is sign-extended for an int64 and
 *    zero-extended for a uint64 or a pointer (as an x86-64 callee that
 *    writes eax leaves rax); a result of C j declared i is truncated, then
 *    extended as declared; a result of C v declared i or j is 0; a result
 *    declared v is ignored, whatever C answers;
 *  - a variadic function (the signature ends in '.<fixed arguments>') is
 *    called with its variable arguments in their buffer, as declared, also
 *    when the declaration does not say it is variadic (no
 *    fixedArgumentCount:), or says another number of fixed arguments,
 *    where libffi would pass the first variable argument as the pointer to
 *    the buffer.  When libffi cannot prepare that call (a float, or an
 *    integer narrower than an int, among the variable arguments, which C
 *    would have promoted) the callout fails, and says on stderr, through
 *    err(): 'FFI callout to a variadic function <name>: declare it with
 *    fixedArgumentCount:' (its number of fixed arguments, and among the
 *    others only types that C passes: int and wider, double, pointers).
 *
 * Everything else goes through unchanged, and the guard of
 * emscriptenSupport.c reports it when it throws: a function that is not in
 * the registry (a function pointer from elsewhere), or has no signature (the
 * FFI test library, whose table defines its functions); a struct, long
 * double or complex in the declaration; more than MAX_ARGUMENTS arguments; a
 * number of arguments other than the signature's; a variadic declaration of
 * a function that is not variadic; and a difference of i or j with f or d,
 * another class, which natively too reads garbage.  So do the callbacks: a
 * callback whose declaration differs from the C function pointer's type
 * traps in the indirect call that reaches it.
 *
 * The state of an adapted call (its cif, types, values and result) is in
 * the frame of emscriptenAdaptedFFICall: callbacks of the call that make
 * adapted callouts of their own do not share it, and nothing is cached.
 * What is kept is the index of the signatures by address, built at the
 * first callout and not changed after.
 *
 * The environment, read at the first callout:
 *
 *   PHARO_WASM_FFI_ADAPT=0  adapt no callout (the negative controls of the
 *                           tests)
 *   PHARO_WASM_FFI_TRACE=1  print, through err() (stderr), a line for each
 *                           function the first time a callout to it is
 *                           adapted:
 *                           'FFI callout adapted: <name> (declared <classes>, C <signature>)'
 *                           where <classes> are those of the cif, the
 *                           result first (with '.<fixed arguments>' when
 *                           it is variadic), and <signature> the
 *                           function's
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <ffi.h>
#include <emscripten/console.h>

#include "pharovm/emscripten/ffiRegistry.h"

/* The most arguments of an adapted callout */
#define MAX_ARGUMENTS 16

/* The flag of a variadic cif in cif->flags, VARARGS_FLAG of libffi's
 * src/wasm/ffi.c, which does not export it (libffi 3.8.0).
 */
#define LIBFFI_WASM_VARARGS_FLAG 1

typedef int (*PharoFFICall)(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue);

int emscriptenAdaptedFFICall(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue, PharoFFICall call);

typedef struct {
	uintptr_t address;
	const char *signature;
	const char *name;
} SignatureEntry;

static const SignatureEntry *signatures;	/* sorted by address */
static size_t signatureCount;
static unsigned char *traced;	/* with TRACE, a flag for each entry */
static int adapting = -1;	/* -1 until the first callout */

static int
compareAddresses(const void *a, const void *b)
{
	uintptr_t x = ((const SignatureEntry *)a)->address;
	uintptr_t y = ((const SignatureEntry *)b)->address;

	return x < y ? -1 : x > y;
}

/* The index of the functions of the registry that have a signature, by
 * address, and the switches of the environment.
 */
static void
initializeAdapter(void)
{
	const char *adapt = getenv("PHARO_WASM_FFI_ADAPT");
	const char *trace = getenv("PHARO_WASM_FFI_TRACE");
	SignatureEntry *entries;
	size_t count = 0;

	adapting = !(adapt && strcmp(adapt, "0") == 0);
	if (!adapting)
		return;
	for (int i = 0; pharoFFILibraries[i]; i++)
		for (size_t j = 0; j < pharoFFILibraries[i]->count; j++)
			if (pharoFFILibraries[i]->symbols[j].signature)
				count += 1;
	if (!count || !(entries = malloc(count * sizeof(SignatureEntry)))) {
		adapting = 0;
		return;
	}
	count = 0;
	for (int i = 0; pharoFFILibraries[i]; i++)
		for (size_t j = 0; j < pharoFFILibraries[i]->count; j++) {
			const PharoFFISymbol *symbol = &pharoFFILibraries[i]->symbols[j];

			if (symbol->signature) {
				entries[count].address = (uintptr_t)symbol->address;
				entries[count].signature = symbol->signature;
				entries[count].name = symbol->name;
				count += 1;
			}
		}
	qsort(entries, count, sizeof(SignatureEntry), compareAddresses);
	signatures = entries;
	signatureCount = count;
	if (trace && strcmp(trace, "1") == 0)
		traced = calloc(count, 1);
}

/* The class of a type of a cif, or 0 for those that are not adapted */
static char
classOf(const ffi_type *type)
{
	switch (type->type) {
	case FFI_TYPE_VOID:
		return 'v';
	case FFI_TYPE_INT:
	case FFI_TYPE_SINT8:
	case FFI_TYPE_UINT8:
	case FFI_TYPE_SINT16:
	case FFI_TYPE_UINT16:
	case FFI_TYPE_SINT32:
	case FFI_TYPE_UINT32:
		return 'i';
	case FFI_TYPE_SINT64:
	case FFI_TYPE_UINT64:
	case FFI_TYPE_POINTER:
		return 'j';
	case FFI_TYPE_FLOAT:
		return 'f';
	case FFI_TYPE_DOUBLE:
		return 'd';
	default:	/* struct, long double, complex */
		return 0;
	}
}

/* value, an integer of class i, truncated to the size of type and extended
 * to 64 bits by its signedness.
 */
static int64_t
extendAs(const ffi_type *type, int64_t value)
{
	switch (type->type) {
	case FFI_TYPE_SINT8:
		return (int8_t)value;
	case FFI_TYPE_UINT8:
		return (uint8_t)value;
	case FFI_TYPE_SINT16:
		return (int16_t)value;
	case FFI_TYPE_UINT16:
		return (uint16_t)value;
	case FFI_TYPE_UINT32:
		return (uint32_t)value;
	default:	/* FFI_TYPE_INT, FFI_TYPE_SINT32 */
		return (int32_t)value;
	}
}

/* The value at address of a type of class i, extended to 64 bits */
static int64_t
readExtended(const ffi_type *type, const void *address)
{
	switch (type->size) {
	case 1:
		return extendAs(type, *(const uint8_t *)address);
	case 2:
		return extendAs(type, *(const uint16_t *)address);
	default:
		return extendAs(type, *(const uint32_t *)address);
	}
}

/* The type of libffi of a class of a signature */
static ffi_type *
typeOfClass(char class)
{
	switch (class) {
	case 'i':
		return &ffi_type_sint32;
	case 'j':
		return &ffi_type_sint64;
	case 'f':
		return &ffi_type_float;
	case 'd':
		return &ffi_type_double;
	default:
		return &ffi_type_void;
	}
}

/* Whether a declared class can be adapted to the class of C, or needs no
 * adaptation: the same class, i and j, and for a result one of them v (but
 * a result declared f or d of a function without one).
 */
static int
isAdaptable(char declared, char c, int isResult)
{
	if (declared == c)
		return 1;
	if ((declared == 'i' || declared == 'j') && (c == 'i' || c == 'j'))
		return 1;
	return isResult && (declared == 'v' || (c == 'v' && declared != 'f' && declared != 'd'));
}

/* Make the callout fn(avalue) of cif with call, which is the guarded
 * ffi_call of emscriptenSupport.c, adapted to the signature of fn when its
 * declaration needs it.  Answer what call answers (0 when the function
 * returned, nonzero when it threw), or 1 when the adapted call of a
 * variadic function cannot be prepared.
 */
int
emscriptenAdaptedFFICall(ffi_cif *cif, void (*fn)(void), void *rvalue, void **avalue, PharoFFICall call)
{
	SignatureEntry key, *entry;
	const char *signature, *dot;
	char declared[MAX_ARGUMENTS + 2];
	unsigned nargs = cif->nargs, fixed;
	size_t length;
	int calleeFixed, cifVariadic, adapt;

	if (adapting < 0)
		initializeAdapter();
	if (!adapting || nargs > MAX_ARGUMENTS)
		return call(cif, fn, rvalue, avalue);
	key.address = (uintptr_t)fn;
	entry = bsearch(&key, signatures, signatureCount, sizeof(SignatureEntry), compareAddresses);
	if (!entry)
		return call(cif, fn, rvalue, avalue);

	/* The classes of the declaration, and what the signature has for each */
	if (!(declared[0] = classOf(cif->rtype)))
		return call(cif, fn, rvalue, avalue);
	for (unsigned i = 0; i < nargs; i++)
		if (!(declared[i + 1] = classOf(cif->arg_types[i])))
			return call(cif, fn, rvalue, avalue);
	declared[nargs + 1] = 0;
	signature = entry->signature;
	dot = strchr(signature, '.');
	length = dot ? (size_t)(dot - signature) : strlen(signature);
	calleeFixed = dot ? atoi(dot + 1) : -1;
	cifVariadic = (cif->flags & LIBFFI_WASM_VARARGS_FLAG) || cif->nfixedargs < nargs;
	if (calleeFixed < 0) {
		/* Not variadic: one class for each argument */
		if (cifVariadic || length != nargs + 1)
			return call(cif, fn, rvalue, avalue);
		fixed = nargs;
		adapt = 0;
	} else {
		/* Variadic: the fixed arguments, then the pointer to the others */
		if (length != (size_t)calleeFixed + 2 || nargs < (unsigned)calleeFixed)
			return call(cif, fn, rvalue, avalue);
		fixed = calleeFixed;
		adapt = !cifVariadic || cif->nfixedargs != fixed;
	}
	if (!isAdaptable(declared[0], signature[0], 1))
		return call(cif, fn, rvalue, avalue);
	adapt = adapt || (declared[0] != signature[0] && declared[0] != 'v');
	for (unsigned i = 1; i <= fixed; i++) {
		if (!isAdaptable(declared[i], signature[i], 0))
			return call(cif, fn, rvalue, avalue);
		adapt = adapt || declared[i] != signature[i];
	}
	if (!adapt)
		return call(cif, fn, rvalue, avalue);

	/* The adapted call, on the types of C for the result and the fixed
	 * arguments, and those of the declaration for the variable ones
	 */
	{
		ffi_cif adapted;
		ffi_type *types[MAX_ARGUMENTS];
		void *values[MAX_ARGUMENTS];
		int64_t wide[MAX_ARGUMENTS];
		int32_t narrow[MAX_ARGUMENTS];
		union { int64_t integer; double float64; float float32; } result;
		ffi_status status;
		int failed;
		int64_t value;

		for (unsigned i = 0; i < nargs; i++) {
			types[i] = cif->arg_types[i];
			values[i] = avalue[i];
			if (i >= fixed || declared[i + 1] == signature[i + 1])
				continue;
			if (declared[i + 1] == 'j') {
				narrow[i] = (int32_t)*(const int64_t *)avalue[i];
				types[i] = &ffi_type_sint32;
				values[i] = &narrow[i];
			} else {
				wide[i] = readExtended(cif->arg_types[i], avalue[i]);
				types[i] = &ffi_type_sint64;
				values[i] = &wide[i];
			}
		}
		if (calleeFixed >= 0)
			status = ffi_prep_cif_var(&adapted, cif->abi, fixed, nargs, typeOfClass(signature[0]), types);
		else
			status = ffi_prep_cif(&adapted, cif->abi, nargs, typeOfClass(signature[0]), types);
		if (status != FFI_OK) {
			if (calleeFixed < 0)
				return call(cif, fn, rvalue, avalue);
			emscripten_errf("FFI callout to a variadic function %s: declare it with fixedArgumentCount:",
				entry->name);
			return 1;
		}
		if (traced && !traced[entry - signatures]) {
			traced[entry - signatures] = 1;
			if (cifVariadic)
				emscripten_errf("FFI callout adapted: %s (declared %s.%u, C %s)",
					entry->name, declared, cif->nfixedargs, signature);
			else
				emscripten_errf("FFI callout adapted: %s (declared %s, C %s)",
					entry->name, declared, signature);
		}

		memset(&result, 0, sizeof(result));
		if ((failed = call(&adapted, fn, &result, values)))
			return failed;

		/* The result as libffi gives the declared type: an integral one
		 * widened to an ffi_arg
		 */
		value = signature[0] == 'j' ? result.integer
			: signature[0] == 'i' ? (int32_t)result.integer
			: 0;
		switch (declared[0]) {
		case 'i':
			*(ffi_arg *)rvalue = (ffi_arg)extendAs(cif->rtype, value);
			break;
		case 'j':
			if (signature[0] == 'i' && cif->rtype->type != FFI_TYPE_SINT64)
				value = (uint32_t)value;
			*(int64_t *)rvalue = value;
			break;
		case 'f':
			*(float *)rvalue = result.float32;
			break;
		case 'd':
			*(double *)rvalue = result.float64;
			break;
		default:	/* 'v' */
			break;
		}
		return 0;
	}
}
