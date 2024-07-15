
char __buildInfo[] = "UtilsPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
// #include <timsort.h>

/* Default EXPORT macro that does nothing (see comment in sq.h): */
#define EXPORT(returnType) returnType

#include "pharovm/pharo.h"
// #include "sqConfig.h"			/* Configuration options */
// #include "sqVirtualMachine.h"	/*  The virtual machine proxy definition */
// #include "sqPlatformSpecific.h" /* Platform specific definitions */
// #include "sqMemoryAccess.h"

#define true 1
#define false 0
#define null 0 /* using 'null' because nil is predefined in Think C */
#ifdef SQUEAK_BUILTIN_PLUGIN
#undef EXPORT
#define EXPORT(returnType) returnType
#endif

#include "UtilsPlugin.h"

/*** Function Prototypes ***/
EXPORT(const char *)
getModuleName(void);
EXPORT(sqInt)
initialiseModule(void);
// EXPORT(sqInt)
// primitiveCompileThenFormat(void);
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter);
sqInt sqAssert(sqInt aBool);

struct VirtualMachine *interpreterProxy;
const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"UtilsPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"UtilsPlugin VMMaker.oscog-eem.2495 (e)"
#endif
	;

/*	Note: This is hardcoded so it can be run from Squeak.
	The module name is used for validating a module *after*
	it is loaded to check if it does really contain the module
	we're thinking it contains. This is important! */

/* InterpreterPlugin>>#getModuleName */
EXPORT(const char *)
getModuleName(void)
{
	return moduleName;
}

EXPORT(sqInt)
initialiseModule(void)
{
	return true;
}

void writeAddress(sqInt anExternalAddress, void *value)
{
	if (!interpreterProxy->isKindOfClass(anExternalAddress, interpreterProxy->classExternalAddress()))
	{
		interpreterProxy->primitiveFail();
		return;
	}

	*((void **)interpreterProxy->firstIndexableField(anExternalAddress)) = value;
}

sqInt newExternalAddress()
{
	return interpreterProxy->instantiateClassindexableSize(interpreterProxy->classExternalAddress(), sizeof(void *));
}

char *checked_cStringOrNullFor(sqInt oop, int *should_free)
{
	char *str = (char *)interpreterProxy->firstIndexableField(oop);
	sqInt size = interpreterProxy->stSizeOf(oop);

	*should_free = str[size] != '\0';

	return *should_free ? interpreterProxy->cStringOrNullFor(oop) : str;
}

EXPORT(sqInt)
primitive_decasteljau(void)
{
	sqInt designpoints = interpreterProxy->stackValue(2);
	sqInt controlpoints = interpreterProxy->stackValue(1);
	sqInt parameterDomain = interpreterProxy->stackValue(0);

	sqInt qPoint, hPoint;

	sqInt designpoints_length = interpreterProxy->stSizeOf(designpoints);
	sqInt domain_length = interpreterProxy->stSizeOf(parameterDomain);

	// printf("design point size %d, parameter domain size %d\n", designpoints_length, domain_length);

	//  sqInt aSequenceableOfPoints = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), designpoints_length);

	double h, u, t, x, y, h1, qx, qy;

	for (int j = 1; j <= domain_length; j++)
	{
		// memcpy(interpreterProxy->firstIndexableField(aSequenceableOfPoints),
		// 	   interpreterProxy->firstIndexableField(designpoints),
		// 	   sizeof(sqInt) * designpoints_length);

		t = interpreterProxy->floatValueOf(interpreterProxy->stObjectat(parameterDomain, j));

		h = 1.0;
		u = h - t;

		qPoint = interpreterProxy->stObjectat(designpoints, 1);
		qx = interpreterProxy->fetchFloatofObject(0, qPoint);
		qy = interpreterProxy->fetchFloatofObject(1, qPoint);

		// printf("design point (%f, %f) for param %f (%dith)\n", qx, qy, t, j);

		for (int k = 1; k < designpoints_length; k++)
		{
			h *= fma(t, (double)designpoints_length, (-t) * (double)k);
			h /= fma(u, k, h);

			hPoint = interpreterProxy->stObjectat(designpoints, k + 1);

			x = interpreterProxy->fetchFloatofObject(0, hPoint) * h;
			y = interpreterProxy->fetchFloatofObject(1, hPoint) * h;

			h1 = 1.0 - h;
			qx = fma(qx, h1, x);
			qy = fma(qy, h1, y);

			// interpreterProxy->storePointerofObjectwithValue(0, interpreterProxy->floatObjectOf(x), qPoint);
			// interpreterProxy->storePointerofObjectwithValue(1, interpreterProxy->floatObjectOf(y), qPoint);

			// qPoint = interpreterProxy->makePointwithxValueyValue(interpreterProxy->floatObjectOf(x), interpreterProxy->floatObjectOf(y));
			// printf("intermediate (%f, %f) for param %f (%dith)\n", qx, qy, t, j);
		}

		qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
		interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->floatObjectOf(qx));
		interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->floatObjectOf(qy));

		interpreterProxy->stObjectatput(controlpoints, j, qPoint);

		// printf("finished (%f, %f) for param %f (%dith)\n", qx, qy, t, j);
	}

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
	}

	// printf("finished\n");

	return null;
}

EXPORT(sqInt)
primitive_strtok_r(void)
{
	sqInt oop;
	int length;

	int free_orig;
	char *orig = checked_cStringOrNullFor(interpreterProxy->stackValue(2), &free_orig); // the receiver, indeed.

	int free_delimiters;
	char *delimiters = checked_cStringOrNullFor(interpreterProxy->stackValue(1), &free_delimiters);

	sqInt include_empty_lines = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	length = strlen(orig);
	char *str = (char *)malloc(sizeof(char) * length + 1);

	char *del = strcpy(str, orig); // take a reference to the start of `str` in order to free it later; btw, `str` will be moved forward.

	// auxiliary pointers for tokens and followers.
	char *pch = NULL;
	char *ptr = str;

	int lines = 0;

	sqInt *buffer = (sqInt *)malloc(sizeof(sqInt) * length);

	while ((pch = strtok_r(str, delimiters, &str)) != NULL)
	{
		while (include_empty_lines && ptr != pch)
		{
			oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), 0);
			// strcpy(interpreterProxy->firstIndexableField(oop), "");
			*((char *)(interpreterProxy->firstIndexableField(oop))) = '\0';
			buffer[lines] = oop;

			lines++;
			ptr += 1;
		}

		length = strlen(pch);
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), length);
		strcpy(interpreterProxy->firstIndexableField(oop), pch);
		buffer[lines] = oop;

		lines++;
		ptr = str;
	}

	while (include_empty_lines && *ptr != '\0')
	{
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), 0);
		// strcpy(interpreterProxy->firstIndexableField(oop), "");
		*((char *)(interpreterProxy->firstIndexableField(oop))) = '\0';
		buffer[lines] = oop;

		lines++;
		ptr += 1;
	}

	if (lines == 1)
	{
		length = strlen(str);
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), length);
		strcpy(interpreterProxy->firstIndexableField(oop), str);
		buffer[lines] = oop;
		lines++;
	}

	oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), lines);

	memcpy(interpreterProxy->firstIndexableField(oop), buffer, sizeof(sqInt) * lines);

	free(del);
	free(buffer);

	if (free_orig)
		free(orig);

	if (free_delimiters)
		free(delimiters);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, oop);
	}

	return null;
}

/* Lots of code for an adaptive, stable, natural mergesort.  There are many
 * pieces to this algorithm; read listsort.txt for overviews and details.
 */

/* A sortslice contains a pointer to an array of keys and a pointer to
 * an array of corresponding values.  In other words, keys[i]
 * corresponds with values[i].  If values == NULL, then the keys are
 * also the values.
 *
 * Several convenience routines are provided here, so that keys and
 * values are always moved in sync.
 */

#define STATIC_LOCAL_INLINE(type) static inline type

#define SSIZE_T_MAX INTPTR_MAX

typedef intptr_t tim_ssize_t;

typedef struct tim_object_s
{
	sqInt oop;
	double value;
	int stIndex;
} tim_object_t;

typedef struct tim_listobject_s
{
	sqInt oop;
	tim_object_t **ob_item;
	sqInt ob_size;
} tim_listobject_t;

/* Reverse a slice of a list in place, from lo up to (exclusive) hi. */
static void
reverse_slice(tim_object_t **lo, tim_object_t **hi)
{
	assert(lo && hi);

	tim_object_t *t;
	
	--hi;
	while (lo < hi)
	{
		t = *lo;
		*lo = *hi;
		*hi = t;
		++lo;
		--hi;
	}
}

void *
tim_mem_malloc(size_t size)
{
	return (size > (size_t)SSIZE_T_MAX) ? NULL : malloc(size);
}

typedef struct
{
	tim_object_t **keys;
} sortslice;

STATIC_LOCAL_INLINE(void)
sortslice_copy(sortslice *s1, tim_ssize_t i, sortslice *s2, tim_ssize_t j)
{
	s1->keys[i] = s2->keys[j];
}

STATIC_LOCAL_INLINE(void)
sortslice_copy_incr(sortslice *dst, sortslice *src)
{
	*dst->keys++ = *src->keys++;
}

STATIC_LOCAL_INLINE(void)
sortslice_copy_decr(sortslice *dst, sortslice *src)
{
	*dst->keys-- = *src->keys--;
}

STATIC_LOCAL_INLINE(void)
sortslice_memcpy(sortslice *s1, tim_ssize_t i, sortslice *s2, tim_ssize_t j,
				 tim_ssize_t n)
{
	memcpy(&s1->keys[i], &s2->keys[j], sizeof(tim_object_t *) * n);
}

STATIC_LOCAL_INLINE(void)
sortslice_memmove(sortslice *s1, tim_ssize_t i, sortslice *s2, tim_ssize_t j,
				  tim_ssize_t n)
{
	memmove(&s1->keys[i], &s2->keys[j], sizeof(tim_object_t *) * n);
}

STATIC_LOCAL_INLINE(void)
sortslice_advance(sortslice *slice, tim_ssize_t n)
{
	slice->keys += n;
}

/* Comparison function: ms->key_compare, which is set at run-time in
 * listsort_impl to optimize for various special cases.
 * Returns -1 on error, 1 if x < y, 0 if x >= y.
 */

#define ISLT(X, Y) (*(ms->key_compare))(X, Y, ms)

/* Compare X to Y via "<".  Goto "fail" if the comparison raises an
   error.  Else "k" is set to true iff X<Y, and an "if (k)" block is
   started.  It makes more sense in context <wink>.  X and Y are tim_object_t*s.
*/
#define IFLT(X, Y)            \
	if ((k = ISLT(X, Y)) < 0) \
		goto fail;            \
	if (k)

/* The maximum number of entries in a MergeState's pending-runs stack.
 * For a list with n elements, this needs at most floor(log2(n)) + 1 entries
 * even if we didn't force runs to a minimal length.  So the number of bits
 * in a tim_ssize_t is plenty large enough for all cases.
 */
#define MAX_MERGE_PENDING (sizeof(size_t) * 8)

/* When we get into galloping mode, we stay there until both runs win less
 * often than MIN_GALLOP consecutive times.  See listsort.txt for more info.
 */
#define MIN_GALLOP 7

/* Avoid malloc for small temp arrays. */
#define MERGESTATE_TEMP_SIZE 256

/* One MergeState exists on the stack per invocation of mergesort.  It's just
 * a convenient way to pass state around among the helper functions.
 */
struct s_slice
{
	sortslice base;
	tim_ssize_t len; /* length of run */
	int power;		 /* node "level" for powersort merge strategy */
};

typedef struct s_MergeState MergeState;
struct s_MergeState
{
	/* This controls when we get *into* galloping mode.  It's initialized
	 * to MIN_GALLOP.  merge_lo and merge_hi tend to nudge it higher for
	 * random data, and lower for highly structured data.
	 */
	tim_ssize_t min_gallop;

	tim_ssize_t listlen;	 /* len(input_list) - read only */
	tim_object_t **basekeys; /* base address of keys array - read only */

	/* 'a' is temp storage to help with merges.  It contains room for
	 * alloced entries.
	 */
	sortslice a; /* may point to temparray below */
	tim_ssize_t alloced;

	/* A stack of n pending runs yet to be merged.  Run #i starts at
	 * address base[i] and extends for len[i] elements.  It's always
	 * true (so long as the indices are in bounds) that
	 *
	 *     pending[i].base + pending[i].len == pending[i+1].base
	 *
	 * so we could cut the storage for this, but it's a minor amount,
	 * and keeping all the info explicit simplifies the code.
	 */
	int n;
	struct s_slice pending[MAX_MERGE_PENDING];

	/* 'a' points to this when possible, rather than muck with malloc. */
	tim_object_t *temparray[MERGESTATE_TEMP_SIZE];

	/* This is the function we will use to compare two keys,
	 * even when none of our special cases apply and we have to use
	 * safe_object_compare. */
	int (*key_compare)(tim_object_t *, tim_object_t *, MergeState *);

	/* This function is used by unsafe_object_compare to optimize comparisons
	 * when we know our list is type-homogeneous but we can't assume anything else.
	 * In the pre-sort check it is set equal to Py_TYPE(key)->tp_richcompare */
	tim_object_t *(*key_richcompare)(tim_object_t *, tim_object_t *, int);

	/* This function is used by unsafe_tuple_compare to compare the first elements
	 * of tuples. It may be set to safe_object_compare, but the idea is that hopefully
	 * we can assume more, and use one of the special-case compares. */
	int (*tuple_elem_compare)(tim_object_t *, tim_object_t *, MergeState *);
};

/* binarysort is the best method for sorting small arrays: it does
   few compares, but can do data movement quadratic in the number of
   elements.
   [lo, hi) is a contiguous slice of a list, and is sorted via
   binary insertion.  This sort is stable.
   On entry, must have lo <= start <= hi, and that [lo, start) is already
   sorted (pass start == lo if you don't know!).
   If islt() complains return -1, else 0.
   Even in case of error, the output slice will be some permutation of
   the input (nothing is lost or duplicated).
*/
static int
binarysort(MergeState *ms, sortslice lo, tim_object_t **hi, tim_object_t **start)
{
	tim_ssize_t k;
	tim_object_t **l, **p, **r;
	tim_object_t *pivot;

	assert(lo.keys <= start && start <= hi);
	/* assert [lo, start) is sorted */
	if (lo.keys == start)
		++start;
	for (; start < hi; ++start)
	{
		/* set l to where *start belongs */
		l = lo.keys;
		r = start;
		pivot = *r;
		/* Invariants:
		 * pivot >= all in [lo, l).
		 * pivot  < all in [r, start).
		 * The second is vacuously true at the start.
		 */
		assert(l < r);
		do
		{
			p = l + ((r - l) >> 1);
			IFLT(pivot, *p)
			r = p;
			else l = p + 1;
		} while (l < r);
		assert(l == r);
		/* The invariants still hold, so pivot >= all in [lo, l) and
		   pivot < all in [l, start), so pivot belongs at l.  Note
		   that if there are elements equal to pivot, l points to the
		   first slot after them -- that's why this sort is stable.
		   Slide over to make room.
		   Caution: using memmove is much slower under MSVC 5;
		   we're not usually moving many slots. */
		for (p = start; p > l; --p)
			*p = *(p - 1);
		*l = pivot;
	}
	return 0;

fail:
	return -1;
}

/*
Return the length of the run beginning at lo, in the slice [lo, hi).  lo < hi
is required on entry.  "A run" is the longest ascending sequence, with

	lo[0] <= lo[1] <= lo[2] <= ...

or the longest descending sequence, with

	lo[0] > lo[1] > lo[2] > ...

Boolean *descending is set to 0 in the former case, or to 1 in the latter.
For its intended use in a stable mergesort, the strictness of the defn of
"descending" is needed so that the caller can safely reverse a descending
sequence without violating stability (strict > ensures there are no equal
elements to get out of order).

Returns -1 in case of error.
*/
static tim_ssize_t
count_run(MergeState *ms, tim_object_t **lo, tim_object_t **hi, int *descending)
{
	tim_ssize_t k;
	tim_ssize_t n;

	assert(lo < hi);
	*descending = 0;
	++lo;
	if (lo == hi)
		return 1;

	n = 2;
	IFLT(*lo, *(lo - 1))
	{
		*descending = 1;
		for (lo = lo + 1; lo < hi; ++lo, ++n)
		{
			IFLT(*lo, *(lo - 1));
			else break;
		}
	}
	else
	{
		for (lo = lo + 1; lo < hi; ++lo, ++n)
		{
			IFLT(*lo, *(lo - 1))
			break;
		}
	}

	return n;
fail:
	return -1;
}

/*
Locate the proper position of key in a sorted vector; if the vector contains
an element equal to key, return the position immediately to the left of
the leftmost equal element.  [gallop_right() does the same except returns
the position to the right of the rightmost equal element (if any).]

"a" is a sorted vector with n elements, starting at a[0].  n must be > 0.

"hint" is an index at which to begin the search, 0 <= hint < n.  The closer
hint is to the final result, the faster this runs.

The return value is the int k in 0..n such that

	a[k-1] < key <= a[k]

pretending that *(a-1) is minus infinity and a[n] is plus infinity.  IOW,
key belongs at index k; or, IOW, the first k elements of a should precede
key, and the last n-k should follow key.

Returns -1 on error.  See listsort.txt for info on the method.
*/
static tim_ssize_t
gallop_left(MergeState *ms, tim_object_t *key, tim_object_t **a, tim_ssize_t n, tim_ssize_t hint)
{
	tim_ssize_t ofs;
	tim_ssize_t lastofs;
	tim_ssize_t k;

	assert(key && a && n > 0 && hint >= 0 && hint < n);

	a += hint;
	lastofs = 0;
	ofs = 1;
	IFLT(*a, key)
	{
		/* a[hint] < key -- gallop right, until
		 * a[hint + lastofs] < key <= a[hint + ofs]
		 */
		const tim_ssize_t maxofs = n - hint; /* &a[n-1] is highest */
		while (ofs < maxofs)
		{
			IFLT(a[ofs], key)
			{
				lastofs = ofs;
				assert(ofs <= (SSIZE_T_MAX - 1) / 2);
				ofs = (ofs << 1) + 1;
			}
			else /* key <= a[hint + ofs] */
				break;
		}
		if (ofs > maxofs)
			ofs = maxofs;
		/* Translate back to offsets relative to &a[0]. */
		lastofs += hint;
		ofs += hint;
	}
	else
	{
		/* key <= a[hint] -- gallop left, until
		 * a[hint - ofs] < key <= a[hint - lastofs]
		 */
		const tim_ssize_t maxofs = hint + 1; /* &a[0] is lowest */
		while (ofs < maxofs)
		{
			IFLT(*(a - ofs), key)
			break;
			/* key <= a[hint - ofs] */
			lastofs = ofs;
			assert(ofs <= (SSIZE_T_MAX - 1) / 2);
			ofs = (ofs << 1) + 1;
		}
		if (ofs > maxofs)
			ofs = maxofs;
		/* Translate back to positive offsets relative to &a[0]. */
		k = lastofs;
		lastofs = hint - ofs;
		ofs = hint - k;
	}
	a -= hint;

	assert(-1 <= lastofs && lastofs < ofs && ofs <= n);
	/* Now a[lastofs] < key <= a[ofs], so key belongs somewhere to the
	 * right of lastofs but no farther right than ofs.  Do a binary
	 * search, with invariant a[lastofs-1] < key <= a[ofs].
	 */
	++lastofs;
	while (lastofs < ofs)
	{
		tim_ssize_t m = lastofs + ((ofs - lastofs) >> 1);

		IFLT(a[m], key)
		lastofs = m + 1; /* a[m] < key */
		else ofs = m;	 /* key <= a[m] */
	}
	assert(lastofs == ofs); /* so a[ofs-1] < key <= a[ofs] */
	return ofs;

fail:
	return -1;
}

/*
Exactly like gallop_left(), except that if key already exists in a[0:n],
finds the position immediately to the right of the rightmost equal value.

The return value is the int k in 0..n such that

	a[k-1] <= key < a[k]

or -1 if error.

The code duplication is massive, but this is enough different given that
we're sticking to "<" comparisons that it's much harder to follow if
written as one routine with yet another "left or right?" flag.
*/
static tim_ssize_t
gallop_right(MergeState *ms, tim_object_t *key, tim_object_t **a, tim_ssize_t n, tim_ssize_t hint)
{
	tim_ssize_t ofs;
	tim_ssize_t lastofs;
	tim_ssize_t k;

	assert(key && a && n > 0 && hint >= 0 && hint < n);

	a += hint;
	lastofs = 0;
	ofs = 1;
	IFLT(key, *a)
	{
		/* key < a[hint] -- gallop left, until
		 * a[hint - ofs] <= key < a[hint - lastofs]
		 */
		const tim_ssize_t maxofs = hint + 1; /* &a[0] is lowest */
		while (ofs < maxofs)
		{
			IFLT(key, *(a - ofs))
			{
				lastofs = ofs;
				assert(ofs <= (SSIZE_T_MAX - 1) / 2);
				ofs = (ofs << 1) + 1;
			}
			else /* a[hint - ofs] <= key */
				break;
		}
		if (ofs > maxofs)
			ofs = maxofs;
		/* Translate back to positive offsets relative to &a[0]. */
		k = lastofs;
		lastofs = hint - ofs;
		ofs = hint - k;
	}
	else
	{
		/* a[hint] <= key -- gallop right, until
		 * a[hint + lastofs] <= key < a[hint + ofs]
		 */
		const tim_ssize_t maxofs = n - hint; /* &a[n-1] is highest */
		while (ofs < maxofs)
		{
			IFLT(key, a[ofs])
			break;
			/* a[hint + ofs] <= key */
			lastofs = ofs;
			assert(ofs <= (SSIZE_T_MAX - 1) / 2);
			ofs = (ofs << 1) + 1;
		}
		if (ofs > maxofs)
			ofs = maxofs;
		/* Translate back to offsets relative to &a[0]. */
		lastofs += hint;
		ofs += hint;
	}
	a -= hint;

	assert(-1 <= lastofs && lastofs < ofs && ofs <= n);
	/* Now a[lastofs] <= key < a[ofs], so key belongs somewhere to the
	 * right of lastofs but no farther right than ofs.  Do a binary
	 * search, with invariant a[lastofs-1] <= key < a[ofs].
	 */
	++lastofs;
	while (lastofs < ofs)
	{
		tim_ssize_t m = lastofs + ((ofs - lastofs) >> 1);

		IFLT(key, a[m])
		ofs = m;			  /* key < a[m] */
		else lastofs = m + 1; /* a[m] <= key */
	}
	assert(lastofs == ofs); /* so a[ofs-1] <= key < a[ofs] */
	return ofs;

fail:
	return -1;
}

/* Heterogeneous compare: default, always safe to fall back on. */
static int
safe_object_compare(tim_object_t *v, tim_object_t *w, MergeState *ms)
{
	if (v == NULL || w == NULL)
	{
		printf("NULL pointers in safe compare.\n");
		return -1;
	}

	return v->value < w->value;
}

/* Conceptually a MergeState's constructor. */
static void
merge_init(MergeState *ms, tim_ssize_t list_size, sortslice *lo)
{
	assert(ms != NULL);

	ms->key_compare = safe_object_compare;
	ms->alloced = MERGESTATE_TEMP_SIZE;
	ms->a.keys = ms->temparray;
	ms->n = 0;
	ms->min_gallop = MIN_GALLOP;
	ms->listlen = list_size;
	ms->basekeys = lo->keys;
}

/* Free all the temp memory owned by the MergeState.  This must be called
 * when you're done with a MergeState, and may be called before then if
 * you want to free the temp memory early.
 */
static void
merge_freemem(MergeState *ms)
{
	assert(ms != NULL);
	if (ms->a.keys != ms->temparray)
	{
		free(ms->a.keys);
		ms->a.keys = NULL;
	}
}

/* Ensure enough temp memory for 'need' array slots is available.
 * Returns 0 on success and -1 if the memory can't be gotten.
 */
static int
merge_getmem(MergeState *ms, tim_ssize_t need)
{
	
	assert(ms != NULL);
	if (need <= ms->alloced)
		return 0;


	/* Don't realloc!  That can cost cycles to copy the old data, but
	 * we don't care what's in the block.
	 */
	merge_freemem(ms);
	if ((size_t)need > SSIZE_T_MAX / sizeof(tim_object_t *))
	{
		// PyErr_NoMemory();
		printf("No memory.\n");
		return -1;
	}
	ms->a.keys = (tim_object_t **)tim_mem_malloc(need * sizeof(tim_object_t *));
	if (ms->a.keys != NULL)
	{
		ms->alloced = need;
		return 0;
	}
	// PyErr_NoMemory();
	printf("No memory.\n");
	return -1;
}
#define MERGE_GETMEM(MS, NEED) ((NEED) <= (MS)->alloced ? 0 : merge_getmem(MS, NEED))

/* Merge the na elements starting at ssa with the nb elements starting at
 * ssb.keys = ssa.keys + na in a stable way, in-place.  na and nb must be > 0.
 * Must also have that ssa.keys[na-1] belongs at the end of the merge, and
 * should have na <= nb.  See listsort.txt for more info.  Return 0 if
 * successful, -1 if error.
 */
static tim_ssize_t
merge_lo(MergeState *ms, sortslice ssa, tim_ssize_t na,
		 sortslice ssb, tim_ssize_t nb)
{
	tim_ssize_t k;
	sortslice dest;
	int result = -1; /* guilty until proved innocent */
	tim_ssize_t min_gallop;

	assert(ms && ssa.keys && ssb.keys && na > 0 && nb > 0);
	assert(ssa.keys + na == ssb.keys);
	if (MERGE_GETMEM(ms, na) < 0)
		return -1;
	sortslice_memcpy(&ms->a, 0, &ssa, 0, na);
	dest = ssa;
	ssa = ms->a;

	sortslice_copy_incr(&dest, &ssb);
	--nb;
	if (nb == 0)
		goto Succeed;
	if (na == 1)
		goto CopyB;

	min_gallop = ms->min_gallop;
	for (;;)
	{
		tim_ssize_t acount = 0; /* # of times A won in a row */
		tim_ssize_t bcount = 0; /* # of times B won in a row */

		/* Do the straightforward thing until (if ever) one run
		 * appears to win consistently.
		 */
		for (;;)
		{
			assert(na > 1 && nb > 0);
			k = ISLT(ssb.keys[0], ssa.keys[0]);
			if (k)
			{
				if (k < 0)
					goto Fail;
				sortslice_copy_incr(&dest, &ssb);
				++bcount;
				acount = 0;
				--nb;
				if (nb == 0)
					goto Succeed;
				if (bcount >= min_gallop)
					break;
			}
			else
			{
				sortslice_copy_incr(&dest, &ssa);
				++acount;
				bcount = 0;
				--na;
				if (na == 1)
					goto CopyB;
				if (acount >= min_gallop)
					break;
			}
		}

		/* One run is winning so consistently that galloping may
		 * be a huge win.  So try that, and continue galloping until
		 * (if ever) neither run appears to be winning consistently
		 * anymore.
		 */
		++min_gallop;
		do
		{
			assert(na > 1 && nb > 0);
			min_gallop -= min_gallop > 1;
			ms->min_gallop = min_gallop;
			k = gallop_right(ms, ssb.keys[0], ssa.keys, na, 0);
			acount = k;
			if (k)
			{
				if (k < 0)
					goto Fail;
				sortslice_memcpy(&dest, 0, &ssa, 0, k);
				sortslice_advance(&dest, k);
				sortslice_advance(&ssa, k);
				na -= k;
				if (na == 1)
					goto CopyB;
				/* na==0 is impossible now if the comparison
				 * function is consistent, but we can't assume
				 * that it is.
				 */
				if (na == 0)
					goto Succeed;
			}
			sortslice_copy_incr(&dest, &ssb);
			--nb;
			if (nb == 0)
				goto Succeed;

			k = gallop_left(ms, ssa.keys[0], ssb.keys, nb, 0);
			bcount = k;
			if (k)
			{
				if (k < 0)
					goto Fail;
				sortslice_memmove(&dest, 0, &ssb, 0, k);
				sortslice_advance(&dest, k);
				sortslice_advance(&ssb, k);
				nb -= k;
				if (nb == 0)
					goto Succeed;
			}
			sortslice_copy_incr(&dest, &ssa);
			--na;
			if (na == 1)
				goto CopyB;
		} while (acount >= MIN_GALLOP || bcount >= MIN_GALLOP);
		++min_gallop; /* penalize it for leaving galloping mode */
		ms->min_gallop = min_gallop;
	}
Succeed:
	result = 0;
Fail:
	if (na)
		sortslice_memcpy(&dest, 0, &ssa, 0, na);
	return result;
CopyB:
	assert(na == 1 && nb > 0);
	/* The last element of ssa belongs at the end of the merge. */
	sortslice_memmove(&dest, 0, &ssb, 0, nb);
	sortslice_copy(&dest, nb, &ssa, 0);
	return 0;
}

/* Merge the na elements starting at pa with the nb elements starting at
 * ssb.keys = ssa.keys + na in a stable way, in-place.  na and nb must be > 0.
 * Must also have that ssa.keys[na-1] belongs at the end of the merge, and
 * should have na >= nb.  See listsort.txt for more info.  Return 0 if
 * successful, -1 if error.
 */
static tim_ssize_t
merge_hi(MergeState *ms, sortslice ssa, tim_ssize_t na,
		 sortslice ssb, tim_ssize_t nb)
{
	tim_ssize_t k;
	sortslice dest, basea, baseb;
	int result = -1; /* guilty until proved innocent */
	tim_ssize_t min_gallop;

	assert(ms && ssa.keys && ssb.keys && na > 0 && nb > 0);
	assert(ssa.keys + na == ssb.keys);
	if (MERGE_GETMEM(ms, nb) < 0)
		return -1;
	dest = ssb;
	sortslice_advance(&dest, nb - 1);
	sortslice_memcpy(&ms->a, 0, &ssb, 0, nb);
	basea = ssa;
	baseb = ms->a;
	ssb.keys = ms->a.keys + nb - 1;
	
	sortslice_advance(&ssa, na - 1);

	sortslice_copy_decr(&dest, &ssa);
	--na;
	if (na == 0)
		goto Succeed;
	if (nb == 1)
		goto CopyA;

	min_gallop = ms->min_gallop;
	for (;;)
	{
		tim_ssize_t acount = 0; /* # of times A won in a row */
		tim_ssize_t bcount = 0; /* # of times B won in a row */

		/* Do the straightforward thing until (if ever) one run
		 * appears to win consistently.
		 */
		for (;;)
		{
			assert(na > 0 && nb > 1);
			k = ISLT(ssb.keys[0], ssa.keys[0]);
			if (k)
			{
				if (k < 0)
					goto Fail;
				sortslice_copy_decr(&dest, &ssa);
				++acount;
				bcount = 0;
				--na;
				if (na == 0)
					goto Succeed;
				if (acount >= min_gallop)
					break;
			}
			else
			{
				sortslice_copy_decr(&dest, &ssb);
				++bcount;
				acount = 0;
				--nb;
				if (nb == 1)
					goto CopyA;
				if (bcount >= min_gallop)
					break;
			}
		}

		/* One run is winning so consistently that galloping may
		 * be a huge win.  So try that, and continue galloping until
		 * (if ever) neither run appears to be winning consistently
		 * anymore.
		 */
		++min_gallop;
		do
		{
			assert(na > 0 && nb > 1);
			min_gallop -= min_gallop > 1;
			ms->min_gallop = min_gallop;
			k = gallop_right(ms, ssb.keys[0], basea.keys, na, na - 1);
			if (k < 0)
				goto Fail;
			k = na - k;
			acount = k;
			if (k)
			{
				sortslice_advance(&dest, -k);
				sortslice_advance(&ssa, -k);
				sortslice_memmove(&dest, 1, &ssa, 1, k);
				na -= k;
				if (na == 0)
					goto Succeed;
			}
			sortslice_copy_decr(&dest, &ssb);
			--nb;
			if (nb == 1)
				goto CopyA;

			k = gallop_left(ms, ssa.keys[0], baseb.keys, nb, nb - 1);
			if (k < 0)
				goto Fail;
			k = nb - k;
			bcount = k;
			if (k)
			{
				sortslice_advance(&dest, -k);
				sortslice_advance(&ssb, -k);
				sortslice_memcpy(&dest, 1, &ssb, 1, k);
				nb -= k;
				if (nb == 1)
					goto CopyA;
				/* nb==0 is impossible now if the comparison
				 * function is consistent, but we can't assume
				 * that it is.
				 */
				if (nb == 0)
					goto Succeed;
			}
			sortslice_copy_decr(&dest, &ssa);
			--na;
			if (na == 0)
				goto Succeed;
		} while (acount >= MIN_GALLOP || bcount >= MIN_GALLOP);
		++min_gallop; /* penalize it for leaving galloping mode */
		ms->min_gallop = min_gallop;
	}
Succeed:
	result = 0;
Fail:
	if (nb)
		sortslice_memcpy(&dest, -(nb - 1), &baseb, 0, nb);
	return result;
CopyA:
	assert(nb == 1 && na > 0);
	/* The first element of ssb belongs at the front of the merge. */
	sortslice_memmove(&dest, 1 - na, &ssa, 1 - na, na);
	sortslice_advance(&dest, -na);
	sortslice_advance(&ssa, -na);
	sortslice_copy(&dest, 0, &ssb, 0);
	return 0;
}

/* Merge the two runs at stack indices i and i+1.
 * Returns 0 on success, -1 on error.
 */
static tim_ssize_t
merge_at(MergeState *ms, tim_ssize_t i)
{
	sortslice ssa, ssb;
	tim_ssize_t na, nb;
	tim_ssize_t k;

	assert(ms != NULL);
	assert(ms->n >= 2);
	assert(i >= 0);
	assert(i == ms->n - 2 || i == ms->n - 3);

	ssa = ms->pending[i].base;
	na = ms->pending[i].len;
	ssb = ms->pending[i + 1].base;
	nb = ms->pending[i + 1].len;
	assert(na > 0 && nb > 0);
	assert(ssa.keys + na == ssb.keys);

	/* Record the length of the combined runs; if i is the 3rd-last
	 * run now, also slide over the last run (which isn't involved
	 * in this merge).  The current run i+1 goes away in any case.
	 */
	ms->pending[i].len = na + nb;
	if (i == ms->n - 3)
		ms->pending[i + 1] = ms->pending[i + 2];
	--ms->n;

	/* Where does b start in a?  Elements in a before that can be
	 * ignored (already in place).
	 */
	k = gallop_right(ms, *ssb.keys, ssa.keys, na, 0);
	if (k < 0)
		return -1;
	sortslice_advance(&ssa, k);
	na -= k;
	if (na == 0)
		return 0;

	/* Where does a end in b?  Elements in b after that can be
	 * ignored (already in place).
	 */
	nb = gallop_left(ms, ssa.keys[na - 1], ssb.keys, nb, nb - 1);
	if (nb <= 0)
		return nb;

	/* Merge what remains of the runs, using a temp array with
	 * min(na, nb) elements.
	 */
	if (na <= nb)
		return merge_lo(ms, ssa, na, ssb, nb);
	else
		return merge_hi(ms, ssa, na, ssb, nb);
}

/* Two adjacent runs begin at index s1. The first run has length n1, and
 * the second run (starting at index s1+n1) has length n2. The list has total
 * length n.
 * Compute the "power" of the first run. See listsort.txt for details.
 */
static int
powerloop(tim_ssize_t s1, tim_ssize_t n1, tim_ssize_t n2, tim_ssize_t n)
{
	int result = 0;
	assert(s1 >= 0);
	assert(n1 > 0 && n2 > 0);
	assert(s1 + n1 + n2 <= n);
	/* midpoints a and b:
	 * a = s1 + n1/2
	 * b = s1 + n1 + n2/2 = a + (n1 + n2)/2
	 *
	 * Those may not be integers, though, because of the "/2". So we work with
	 * 2*a and 2*b instead, which are necessarily integers. It makes no
	 * difference to the outcome, since the bits in the expansion of (2*i)/n
	 * are merely shifted one position from those of i/n.
	 */
	tim_ssize_t a = 2 * s1 + n1; /* 2*a */
	tim_ssize_t b = a + n1 + n2; /* 2*b */
	/* Emulate a/n and b/n one bit a time, until bits differ. */
	for (;;)
	{
		++result;
		if (a >= n)
		{ /* both quotient bits are 1 */
			assert(b >= a);
			a -= n;
			b -= n;
		}
		else if (b >= n)
		{ /* a/n bit is 0, b/n bit is 1 */
			break;
		} /* else both quotient bits are 0 */
		assert(a < b && b < n);
		a <<= 1;
		b <<= 1;
	}
	return result;
}

/* The next run has been identified, of length n2.
 * If there's already a run on the stack, apply the "powersort" merge strategy:
 * compute the topmost run's "power" (depth in a conceptual binary merge tree)
 * and merge adjacent runs on the stack with greater power. See listsort.txt
 * for more info.
 *
 * It's the caller's responsibility to push the new run on the stack when this
 * returns.
 *
 * Returns 0 on success, -1 on error.
 */
static int
found_new_run(MergeState *ms, tim_ssize_t n2)
{
	assert(ms);
	if (ms->n)
	{
		assert(ms->n > 0);
		struct s_slice *p = ms->pending;
		tim_ssize_t s1 = p[ms->n - 1].base.keys - ms->basekeys; /* start index */
		tim_ssize_t n1 = p[ms->n - 1].len;
		int power = powerloop(s1, n1, n2, ms->listlen);
		while (ms->n > 1 && p[ms->n - 2].power > power)
		{
			if (merge_at(ms, ms->n - 2) < 0)
				return -1;
		}
		assert(ms->n < 2 || p[ms->n - 2].power < power);
		p[ms->n - 1].power = power;
	}
	return 0;
}

/* Regardless of invariants, merge all runs on the stack until only one
 * remains.  This is used at the end of the mergesort.
 *
 * Returns 0 on success, -1 on error.
 */
static int
merge_force_collapse(MergeState *ms)
{
	struct s_slice *p = ms->pending;

	assert(ms);
	while (ms->n > 1)
	{
		tim_ssize_t n = ms->n - 2;
		if (n > 0 && p[n - 1].len < p[n + 1].len)
			--n;
		if (merge_at(ms, n) < 0)
			return -1;
	}
	return 0;
}

/* Compute a good value for the minimum run length; natural runs shorter
 * than this are boosted artificially via binary insertion.
 *
 * If n < 64, return n (it's too small to bother with fancy stuff).
 * Else if n is an exact power of 2, return 32.
 * Else return an int k, 32 <= k <= 64, such that n/k is close to, but
 * strictly less than, an exact power of 2.
 *
 * See listsort.txt for more info.
 */
static tim_ssize_t
merge_compute_minrun(tim_ssize_t n)
{
	tim_ssize_t r = 0; /* becomes 1 if any 1 bits are shifted off */

	assert(n >= 0);
	while (n >= 64)
	{
		r |= n & 1;
		n >>= 1;
	}
	return n + r;
}

static void
reverse_sortslice(sortslice *s, tim_ssize_t n)
{
	reverse_slice(s->keys, &s->keys[n]);
}

/* An adaptive, stable, natural mergesort.  See listsort.txt.
 * Returns Py_None on success, NULL on error.  Even in case of error, the
 * list will be some permutation of its input state (nothing is lost or
 * duplicated).
 */

/*
Sort the list in ascending order and return None.

The sort is in-place (i.e. the list itself is modified) and stable (i.e. the
order of two equal elements is maintained).

If a key function is given, apply it once to each list item and sort them,
ascending or descending, according to their function values.

The reverse flag can be set to sort in descending order.
*/

static tim_object_t *list_sort_impl(tim_listobject_t *self, int reverse)
{
	MergeState ms;
	tim_ssize_t nremaining;
	tim_ssize_t minrun;
	sortslice lo;
	tim_ssize_t saved_ob_size, saved_allocated;
	tim_object_t **saved_ob_item;
	tim_object_t **final_ob_item;
	tim_object_t *result = NULL; /* guilty until proved innocent */

	/* The list is temporarily made empty, so that mutations performed
	 * by comparison functions can't affect the slice of memory we're
	 * sorting (allowing mutations during sorting is a core-dump
	 * factory, since ob_item may change).
	 */
	saved_ob_size = self->ob_size;
	saved_ob_item = self->ob_item;
	self->ob_size = 0;
	self->ob_item = NULL;

	lo.keys = saved_ob_item;

	merge_init(&ms, saved_ob_size, &lo);

	nremaining = saved_ob_size;
	if (nremaining < 2)
		goto succeed;

	/* Reverse sort stability achieved by initially reversing the list,
	applying a stable forward sort, then reversing the final result. */
	if (reverse)
	{
		reverse_slice(&saved_ob_item[0], &saved_ob_item[saved_ob_size]);
	}

	/* March over the array once, left to right, finding natural runs,
	 * and extending short natural runs to minrun elements.
	 */
	minrun = merge_compute_minrun(nremaining);
	do
	{
		int descending;
		tim_ssize_t n;

		/* Identify next run. */
		n = count_run(&ms, lo.keys, lo.keys + nremaining, &descending);
		if (n < 0)
			goto fail;
		if (descending)
			reverse_sortslice(&lo, n);
		/* If short, extend to min(minrun, nremaining). */
		if (n < minrun)
		{
			const tim_ssize_t force = nremaining <= minrun ? nremaining : minrun;
			if (binarysort(&ms, lo, lo.keys + force, lo.keys + n) < 0)
				goto fail;
			n = force;
		}
		/* Maybe merge pending runs. */
		assert(ms.n == 0 || ms.pending[ms.n - 1].base.keys + ms.pending[ms.n - 1].len == lo.keys);
		if (found_new_run(&ms, n) < 0)
			goto fail;
		/* Push new run on stack. */
		assert(ms.n < MAX_MERGE_PENDING);
		ms.pending[ms.n].base = lo;
		ms.pending[ms.n].len = n;
		++ms.n;
		/* Advance to find next run. */
		sortslice_advance(&lo, n);
		nremaining -= n;
	} while (nremaining);

	if (merge_force_collapse(&ms) < 0)
		goto fail;
	assert(ms.n == 1);
	assert(ms.pending[0].base.keys == saved_ob_item);
	assert(ms.pending[0].len == saved_ob_size);
	lo = ms.pending[0].base;

succeed:
	result = tim_mem_malloc(sizeof(tim_object_t));
	result->stIndex = 0;
	result->value = 0.0;
	result->oop = interpreterProxy->nilObject();
fail:

	if (reverse && saved_ob_size > 1)
		reverse_slice(saved_ob_item, saved_ob_item + saved_ob_size);

	merge_freemem(&ms);

	final_ob_item = self->ob_item;
	self->ob_size = saved_ob_size;
	self->ob_item = saved_ob_item;

	if (final_ob_item != NULL)
	{
		free(final_ob_item);
	}
	return result;
}
#undef IFLT
#undef ISLT

EXPORT(sqInt)
primitive_timsort(void)
{

	sqInt recv = interpreterProxy->stackValue(2); // the receiver.
	sqInt doubles = interpreterProxy->stackValue(1);
	int reverse = interpreterProxy->stackValue(0) == interpreterProxy->trueObject();

	sqInt size = interpreterProxy->stSizeOf(recv);

	assert(size == interpreterProxy->stSizeOf(doubles));

	tim_object_t **objs = tim_mem_malloc(sizeof(tim_object_t *) * size);

	tim_object_t *each;

	for (int i = 0; i < size; i++)
	{
		sqInt i_next = i + 1;

		each = objs[i] = tim_mem_malloc(sizeof(tim_object_t));
		each->oop = interpreterProxy->stObjectat(recv, i_next);
		each->value = interpreterProxy->floatValueOf(interpreterProxy->stObjectat(doubles, i_next));
		each->stIndex = i_next;
	}

	tim_listobject_t list;

	list.oop = recv;
	list.ob_size = size;
	list.ob_item = objs;

	tim_object_t *v = list_sort_impl(&list, reverse);

	if (!(interpreterProxy->failed()))
	{
		if (v == NULL)
		{
			interpreterProxy->popthenPush(3, interpreterProxy->nilObject());
		}
		else
		{
			sqInt j;
			for (sqInt i = 0; i < size; i++)
			{
				j = i + 1;
				each = objs[i];
				interpreterProxy->stObjectatput(recv, j, each->oop);
				interpreterProxy->stObjectatput(doubles, j, each->stIndex);

				free(each);
			}

			interpreterProxy->pop(2); // leave the receiver on the stack.
		}
	}

	free(v);
	free(objs);

	return null;
}

EXPORT(sqInt)
primitive_fma(void)
{
	double x = interpreterProxy->stackFloatValue(2); // the receiver, indeed.
	double y = interpreterProxy->stackFloatValue(1);
	double z = interpreterProxy->stackFloatValue(0);

	double res = fma(x, y, z);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->floatObjectOf(res));
	}

	return null;
}

#define KK 100										   /* the long lag */
#define LL 37										   /* the short lag */
#define mod_sum(x, y) (((x) + (y)) - (int)((x) + (y))) /* (x+y) mod 1.0 */

// double ran_u[KK]; /* the generator state */
void ranf_array(double aa[], int n, double ran_u[])
{
	register int i, j;
	for (j = 0; j < KK; j++)
		aa[j] = ran_u[j];
	for (; j < n; j++)
		aa[j] = mod_sum(aa[j - KK], aa[j - LL]);
	for (i = 0; i < LL; i++, j++)
		ran_u[i] = mod_sum(aa[j - KK], aa[j - LL]);
	for (; i < KK; i++, j++)
		ran_u[i] = mod_sum(aa[j - KK], ran_u[i - LL]);
}

/* the following routines are adapted from exercise 3.6--15 */
/* after calling ranf_start, get new randoms by, e.g., "x=ranf_arr_next()" */

// double ranf_arr_dummy = -1.0, ranf_arr_started = -1.0;
//  double *ranf_arr_ptr = &ranf_arr_dummy; /* the next random fraction, or -1 */

#define TT 70 /* guaranteed separation between streams */
#define is_odd(s) ((s) & 1)

void ranf_start(long seed, double ran_u[])
{
	register int t, s, j;
	double u[KK + KK - 1];
	double ulp = (1.0 / (1L << 30)) / (1L << 22); /* 2 to the -52 */
	double ss = 2.0 * ulp * ((seed & 0x3fffffff) + 2);

	for (j = 0; j < KK; j++)
	{
		u[j] = ss; /* bootstrap the buffer */
		ss += ss;
		if (ss >= 1.0)
			ss -= 1.0 - 2 * ulp; /* cyclic shift of 51 bits */
	}
	u[1] += ulp; /* make u[1] (and only u[1]) "odd" */
	for (s = seed & 0x3fffffff, t = TT - 1; t;)
	{
		for (j = KK - 1; j > 0; j--)
			u[j + j] = u[j], u[j + j - 1] = 0.0; /* "square" */
		for (j = KK + KK - 2; j >= KK; j--)
		{
			u[j - (KK - LL)] = mod_sum(u[j - (KK - LL)], u[j]);
			u[j - KK] = mod_sum(u[j - KK], u[j]);
		}
		if (is_odd(s))
		{ /* "multiply by z" */
			for (j = KK; j > 0; j--)
				u[j] = u[j - 1];
			u[0] = u[KK]; /* shift the buffer cyclically */
			u[LL] = mod_sum(u[LL], u[KK]);
		}
		if (s)
			s >>= 1;
		else
			t--;
	}
	for (j = 0; j < LL; j++)
		ran_u[j + KK - LL] = u[j];
	for (; j < KK; j++)
		ran_u[j - LL] = u[j];
	for (j = 0; j < 10; j++)
		ranf_array(u, KK + KK - 1, ran_u); /* warm things up */
}

double ranf_arr_cycle(double ran_u[], int QUALITY, double *ranf_arr_buf)
{
	ranf_array(ranf_arr_buf, QUALITY, ran_u);
	ranf_arr_buf[KK] = -1;
	return ranf_arr_buf[0];
}

EXPORT(sqInt)
primitive_randomknuth_start(void)
{

	sqInt oop = interpreterProxy->stackValue(0); // the receiver, indeed.
	sqInt stateArray = interpreterProxy->fetchPointerofObject(0, oop);

	long seed = interpreterProxy->fetchIntegerofObject(1, oop);

	double *ran_u = interpreterProxy->firstIndexableField(interpreterProxy->stObjectat(stateArray, 1));
	double *ranf_arr_buf = interpreterProxy->firstIndexableField(interpreterProxy->stObjectat(stateArray, 2));

	ranf_start(seed, ran_u);

	return null;
}

EXPORT(sqInt)
primitive_randomknuth_next(void)
{

	sqInt oop = interpreterProxy->stackValue(0); // the receiver, indeed.
	sqInt stateArray = interpreterProxy->fetchPointerofObject(0, oop);
	double *ran_u = interpreterProxy->firstIndexableField(interpreterProxy->stObjectat(stateArray, 1));
	double *ranf_arr_buf = interpreterProxy->firstIndexableField(interpreterProxy->stObjectat(stateArray, 2));
	sqInt cursor = interpreterProxy->integerValueOf(interpreterProxy->stObjectat(stateArray, 3));
	sqInt quality = interpreterProxy->fetchIntegerofObject(4, oop);

	double r = ranf_arr_buf[cursor - 1];

	if (r > 0.0)
	{
		interpreterProxy->stObjectatput(stateArray, 3, interpreterProxy->integerObjectOf(cursor + 1));
	}
	else
	{
		r = ranf_arr_cycle(ran_u, quality, ranf_arr_buf);
		interpreterProxy->stObjectatput(stateArray, 3, interpreterProxy->integerObjectOf(2));
	}

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, interpreterProxy->floatObjectOf(r));
	}

	return null;
}

/*	Note: This is coded so that it can be run in Squeak. */

/* InterpreterPlugin>>#setInterpreter: */
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	sqInt ok;

	interpreterProxy = anInterpreter;
	ok = ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR)) && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));

	return ok;
}

/* SmartSyntaxInterpreterPlugin>>#sqAssert: */
sqInt sqAssert(sqInt aBool)
{
	/* missing DebugCode */;
	return aBool;
}
