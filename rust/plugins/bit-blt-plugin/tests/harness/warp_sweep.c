#define NDEBUG 1
#include "BitBltPlugin.c"
#include <string.h>

/* Fake object memory: SmallIntegers are (v<<1)|1, nil is 2, the BitBlt
   "object" is oop 16 with slots in fakeSlots, the source map is oop 4. */
#define GUARD 16
#define NCASES 150
static sqInt fakeSlots[32];
static unsigned int mapWords[65536];
static sqInt fake_failed_flag;
static sqInt f_slotSizeOf(sqInt oop) { return oop == 16 ? 27 : 65536; }
static sqInt f_fetchPointerofObject(sqInt i, sqInt oop) { (void)oop; return fakeSlots[i]; }
static sqInt f_isIntegerObject(sqInt oop) { return oop & 1; }
static sqInt f_integerValueOf(sqInt oop) { return oop >> 1; }
static double f_floatValueOf(sqInt oop) { (void)oop; return 0.0; }
static sqInt f_nilObject(void) { return 2; }
static sqInt f_methodArgumentCount(void) { return 2; }
static sqInt smoothingArg; static sqInt mapArg;
static sqInt f_stackIntegerValue(sqInt off) { (void)off; return smoothingArg; }
static sqInt f_stackValue(sqInt off) { (void)off; return mapArg; }
static void *f_firstIndexableField(sqInt oop) { (void)oop; return mapWords; }
static sqInt f_primitiveFail(void) { fake_failed_flag = 1; return 0; }
static sqInt f_failed(void) { return fake_failed_flag; }

static unsigned int srcArena[GUARD + 64 * 8 + GUARD];
static unsigned int dstArena[GUARD + 64 * 8 + GUARD];
static unsigned int lookup[512];
static unsigned int seed;
static unsigned int nextr(void) { seed = seed * 1664525u + 1013904223u; return seed; }
static sqInt smallint(sqInt v) { return (v << 1) | 1; }

int main(void) {
  static const int depths[6] = {1, 2, 4, 8, 16, 32};
  int i, x, y;
  initBBOpTable();
  initDither8Lookup();
  slotSizeOf = f_slotSizeOf;
  fetchPointerofObject = f_fetchPointerofObject;
  isIntegerObject = f_isIntegerObject;
  integerValueOf = f_integerValueOf;
  floatValueOf = f_floatValueOf;
  nilObject = f_nilObject;
  methodArgumentCount = f_methodArgumentCount;
  stackIntegerValue = f_stackIntegerValue;
  stackValue = f_stackValue;
  firstIndexableField = f_firstIndexableField;
  primitiveFail = f_primitiveFail;
  failed = f_failed;
  for (i = 0; i < NCASES; i++) {
    int srcD, dstD, srcM, dstM, rule, useMap, smoothing;
    int wprS, wprD, ns;
    long q[8];
    unsigned int hash;
    seed = 0xABCD1234u ^ ((unsigned int)i * 2654435761u);
    srcD = depths[nextr() % 6];
    dstD = depths[nextr() % 6];
    srcM = (nextr() % 4) != 0;
    dstM = (nextr() % 4) != 0;
    rule = (nextr() % 2) ? 3 : 25; /* store and paint (25 interacts with smoothing) */
    useMap = (nextr() % 3) == 0;
    smoothing = 1 + (int)(nextr() % 2);
    destX = (sqInt)(nextr() % 8);
    destY = (sqInt)(nextr() % 4);
    width = 1 + (sqInt)(nextr() % 40);
    height = 1 + (sqInt)(nextr() % 6);
    clipX = (sqInt)(nextr() % 6);
    clipY = (sqInt)(nextr() % 3);
    clipWidth = 64 - clipX;
    clipHeight = 8 - clipY;
    /* fixed-point quad corners: anywhere from -1 to ~66 in source space */
    for (x = 0; x < 8; x++) q[x] = (long)(nextr() % (67 << 14)) - (1 << 14);
    for (x = 0; x < GUARD + ((64 * srcD) / 32) * 8 + GUARD; x++) srcArena[x] = nextr();
    for (x = 0; x < GUARD + ((64 * dstD) / 32) * 8 + GUARD; x++) dstArena[x] = nextr();
    for (x = 0; x < 512; x++) lookup[x] = nextr();
    for (x = 0; x < 65536; x++) mapWords[x] = nextr();
    wprS = (64 * srcD) / 32;
    wprD = (64 * dstD) / 32;

    /* BitBlt object slots: warp quad at BBWarpBase.. as p1x p1y z p2x p2y z p3x p3y z p4x p4y z */
    fakeSlots[BBWarpBase + 0] = smallint(q[0]);
    fakeSlots[BBWarpBase + 1] = smallint(q[1]);
    fakeSlots[BBWarpBase + 3] = smallint(q[2]);
    fakeSlots[BBWarpBase + 4] = smallint(q[3]);
    fakeSlots[BBWarpBase + 6] = smallint(q[4]);
    fakeSlots[BBWarpBase + 7] = smallint(q[5]);
    fakeSlots[BBWarpBase + 9] = smallint(q[6]);
    fakeSlots[BBWarpBase + 10] = smallint(q[7]);
    bitBltOop = 16;
    smoothingArg = smoothing; /* stackIntegerValue answers the decoded value */
    mapArg = 4; /* always a map object, big enough for any depth */
    fake_failed_flag = 0;

    combinationRule = rule;
    noSource = 0;
    sourceForm = 0x2000; destForm = 0x1000;
    sourceDepth = srcD; destDepth = dstD;
    sourceMSB = srcM; destMSB = dstM;
    sourcePPW = 32 / srcD; destPPW = 32 / dstD;
    sourcePitch = wprS * 4; destPitch = wprD * 4;
    sourceBits = (sqInt)(srcArena + GUARD);
    destBits = (sqInt)(dstArena + GUARD);
    sourceWidth = 64; sourceHeight = 8;
    destWidth = 64; destHeight = 8;
    endOfSource = (usqInt)(srcArena + GUARD + wprS * 8);
    endOfDestination = (usqInt)(dstArena + GUARD + wprD * 8);
    if (useMap) {
      cmFlags = ColorMapPresent | ColorMapIndexedPart;
      cmMask = 511; cmBitsPerColor = 3; cmLookupTable = lookup;
    } else { cmFlags = 0; cmMask = 0; cmBitsPerColor = 0; cmLookupTable = 0; }
    cmShiftTable = 0; cmMaskTable = 0;
    noHalftone = 1; halftoneBase = 0; halftoneHeight = 0;
    sourceX = 0; sourceY = 0;

    /* warpBits minus surfaces */
    ns = noSource; noSource = 1; clipRange(); noSource = ns;
    if (bbW > 0 && bbH > 0) {
      /* destMaskAndPointerInit */
      { sqInt endB, pixPerM1, startB;
        pixPerM1 = destPPW - 1;
        startB = destPPW - (dx & pixPerM1);
        endB = (((dx + bbW) - 1) & pixPerM1) + 1;
        if (destMSB) { mask1 = ((usqInt) AllOnes) >> (32 - (startB * destDepth)); mask2 = ((usqInt)(AllOnes) << (32 - (endB * destDepth))); }
        else { mask1 = ((usqInt)(AllOnes) << (32 - (startB * destDepth))); mask2 = ((usqInt) AllOnes) >> (32 - (endB * destDepth)); }
        if (bbW < startB) { mask1 = mask1 & mask2; mask2 = 0; nWords = 1; }
        else { nWords = (((bbW - startB) + pixPerM1) / destPPW) + 1; }
        hDir = (vDir = 1);
        destIndex = (destBits + (dy * destPitch)) + ((dx / destPPW) * 4);
        destDelta = (destPitch * vDir) - (4 * (nWords * hDir));
      }
      warpLoop();
    }

    hash = 2166136261u;
    for (y = 0; y < wprD * 8; y++) { hash ^= dstArena[GUARD + y]; hash *= 16777619u; }
    hash ^= (unsigned int)fake_failed_flag; hash *= 16777619u;
    printf("0x%08X, ", hash);
    if (i % 8 == 7) printf("\n");
  }
  printf("\n");
  return 0;
}
