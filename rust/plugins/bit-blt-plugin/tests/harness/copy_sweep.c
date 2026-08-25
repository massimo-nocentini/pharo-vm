#define NDEBUG 1
#include "BitBltPlugin.c"
#include <string.h>

#define GUARD 16
#define NCASES 400
static unsigned int srcArena[GUARD + 64 * 8 + GUARD];
static unsigned int dstArena[GUARD + 64 * 8 + GUARD];
static unsigned int lookup[512];
static unsigned int halftone[4];
static unsigned int seed;
static unsigned int nextr(void) { seed = seed * 1664525u + 1013904223u; return seed; }

int main(void) {
  static const int depths[6] = {1, 2, 4, 8, 16, 32};
  static const int rules[35] = {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,
                                18,19,20,21,24,25,26,27,28,29,30,31,32,34,37,38,39,40,41};
  int i, x, y;
  initBBOpTable();
  initDither8Lookup();
  for (i = 0; i < NCASES; i++) {
    int srcD, dstD, srcM, dstM, rule, sx0, sy0, dx0, dy0, w0, h0, useMap, useHt, noSrc;
    int wprS, wprD, maxx;
    unsigned int hash;
    seed = 0x9E3779B9u ^ ((unsigned int)i * 2654435761u);
    srcD = depths[nextr() % 6];
    dstD = depths[nextr() % 6];
    srcM = (nextr() % 4) != 0;
    dstM = (nextr() % 4) != 0;
    rule = rules[nextr() % 35];
    sx0 = nextr() % 24; sy0 = nextr() % 4;
    dx0 = nextr() % 24; dy0 = nextr() % 4;
    w0 = 1 + nextr() % 40; h0 = 1 + nextr() % 4;
    useMap = (nextr() % 3) == 0;
    useHt = (nextr() % 4) == 0;
    noSrc = (nextr() % 8) == 0;
    sourceAlpha = nextr() % 256;
    componentAlphaModeAlpha = nextr() % 256;
    componentAlphaModeColor = nextr() & 0xFFFFFF;
    maxx = sx0 > dx0 ? sx0 : dx0;
    if (w0 > 64 - maxx) w0 = 64 - maxx;
    if (h0 > 8 - (sy0 > dy0 ? sy0 : dy0)) h0 = 8 - (sy0 > dy0 ? sy0 : dy0);
    wprS = (64 * srcD) / 32;
    wprD = (64 * dstD) / 32;
    for (x = 0; x < GUARD + wprS * 8 + GUARD; x++) srcArena[x] = nextr();
    for (x = 0; x < GUARD + wprD * 8 + GUARD; x++) dstArena[x] = nextr();
    for (x = 0; x < 512; x++) lookup[x] = nextr();
    for (x = 0; x < 4; x++) halftone[x] = nextr();

    combinationRule = rule;
    noSource = noSrc;
    sourceForm = 0x2000; destForm = 0x1000;
    sourceDepth = srcD; destDepth = dstD;
    sourceMSB = srcM; destMSB = dstM;
    sourcePPW = 32 / srcD; destPPW = 32 / dstD;
    sourcePitch = wprS * 4; destPitch = wprD * 4;
    sourceBits = (sqInt)(srcArena + GUARD);
    destBits = (sqInt)(dstArena + GUARD);
    endOfSource = (usqInt)(srcArena + GUARD + wprS * 8);
    endOfDestination = (usqInt)(dstArena + GUARD + wprD * 8);
    if (useMap) {
      cmFlags = ColorMapPresent | ColorMapIndexedPart;
      cmMask = 511;
      cmBitsPerColor = 3;
      cmLookupTable = lookup;
      cmShiftTable = 0; cmMaskTable = 0;
    } else {
      cmFlags = 0; cmMask = 0; cmBitsPerColor = 0;
      cmLookupTable = 0; cmShiftTable = 0; cmMaskTable = 0;
    }
    if (useHt) { noHalftone = 0; halftoneBase = (sqInt)halftone; halftoneHeight = 1 + (i % 4); }
    else { noHalftone = 1; halftoneBase = 0; halftoneHeight = 0; }
    gammaLookupTable = 0; ungammaLookupTable = 0;
    sx = sx0; sy = sy0; dx = dx0; dy = dy0; bbW = w0; bbH = h0;
    /* copyBitsDispatch: quick path or the general loop */
    if (!tryCopyingBitsQuickly()) { bitCount = 0; performCopyLoop(); }

    hash = 2166136261u;
    for (y = 0; y < wprD * 8; y++) { hash ^= dstArena[GUARD + y]; hash *= 16777619u; }
    hash ^= (unsigned int)bitCount; hash *= 16777619u;
    printf("0x%08X, ", hash);
    if (i % 8 == 7) printf("\n");
  }
  printf("\n");
  return 0;
}
