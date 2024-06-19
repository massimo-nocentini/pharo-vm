
static char __buildInfo[] = "UtilsPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <timsort.h>

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
#define EXPORT(returnType) static returnType
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
static sqInt sqAssert(sqInt aBool);

struct VirtualMachine *interpreterProxy;
static const char *moduleName =
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

int timsort_comparer(int i, int j, mergestate_t *state)
{
	sqInt array = *((sqInt *)state->userdata);

	return interpreterProxy->floatValueOf(interpreterProxy->stObjectat(array, i)) <
		   interpreterProxy->floatValueOf(interpreterProxy->stObjectat(array, j));
}

EXPORT(sqInt)
primitive_timsort(void)
{
	sqInt recv = interpreterProxy->stackValue(2);
	sqInt collected = interpreterProxy->stackValue(1);
	sqInt reverse = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	int length = interpreterProxy->stSizeOf(recv);

	sortslice_t perm = (sortslice_t)malloc(sizeof(item_t) * length);
	item_t item;

	for (int i = 0; i < length;)
	{
		item = i;
		perm[item] = ++i; // the identity permutation according to Smalltalk's one-based indexing.
	}
	sortslice_t sorting_perm = timsort(perm, length, reverse, &timsort_comparer, &collected);

	// sqInt sorted = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), length);

	if (!(interpreterProxy->failed()))
	{
		for (int i = 0; i < length;)
		{
			item = perm[i];
			interpreterProxy->stObjectatput(collected, ++i, interpreterProxy->stObjectat(recv, item));
		}

		interpreterProxy->popthenPush(3, collected);
	}

	free(perm);

	return null;
}

EXPORT(sqInt)
primitive_fma(void)
{
	sqInt x = interpreterProxy->stackFloatValue(2); // the receiver, indeed.
	sqInt y = interpreterProxy->stackFloatValue(1);
	sqInt z = interpreterProxy->stackFloatValue(0);

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
static sqInt
sqAssert(sqInt aBool)
{
	/* missing DebugCode */;
	return aBool;
}
