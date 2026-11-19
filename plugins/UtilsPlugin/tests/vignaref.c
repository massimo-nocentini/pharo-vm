/* vignaref.c -- the reference of the C tests for SplitMix64: Vigna's
 * splitmix64.c as he wrote it (../src/vigna, included unchanged, with its
 * own names: the static x and next()), and a setter of its state, which the
 * file keeps static.  run-c-tests.sh compiles it on its own, next to the
 * engine, which renames the same file's names (upEngine.c). */

#include "../src/vigna/splitmix64.c"

void
vigna_ref_seed(uint64_t seed)
{
	x = seed;
}

uint64_t
vigna_ref_next(void)
{
	return next();
}
