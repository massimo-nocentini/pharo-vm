#define NDEBUG 1
#include "BitBltPlugin.c"
#include <string.h>

/* 16 guard words on each side, filled from the same LCG stream as the
   bitmap, so a reversed blit's word-before-the-row read is deterministic
   and reproducible by the Rust test. */
#define GUARD 16
static unsigned int arena[GUARD + 64 * 8 + GUARD];
static int WPR;
struct kase { int depth, sx, sy, dx, dy, w, h; };
static struct kase cases[] = {
  {8, 0, 0, 3, 0, 40, 4},   /* right, skewed: reversed loop */
  {1, 3, 0, 8, 0, 29, 4},   /* right at 1bpp, skewed */
  {8, 0, 0, 4, 0, 40, 4},   /* right, word-aligned: skew -32 */
  {8, 1, 0, 5, 0, 17, 3},   /* right, same in-word phase: skew 0 */
  {16, 2, 0, 5, 0, 22, 4},  /* right at 16bpp, skewed */
  {8, 3, 0, 0, 0, 40, 4},   /* left: forward copy */
  {8, 0, 0, 0, 2, 40, 4},   /* down: vDir -1 */
  {8, 0, 2, 0, 0, 40, 4},   /* up: forward */
  {8, 1, 1, 4, 3, 40, 4},   /* down-right */
  {4, 2, 0, 7, 0, 21, 3},   /* right at 4bpp, skewed */
};
int main(void) {
  unsigned int seed;
  int c, x, n;
  initBBOpTable();
  printf("// (depth, sx, sy, dx, dy, w, h, expected words)\n");
  for (c = 0; c < (int)(sizeof cases / sizeof cases[0]); c++) {
    struct kase k = cases[c];
    WPR = (64 * k.depth) / 32;
    n = GUARD + WPR * 8 + GUARD;
    seed = 0xC0FFEE ^ (k.depth * 977 + k.sx * 31 + k.dx * 7 + k.w);
    for (x = 0; x < n; x++) { seed = seed * 1664525u + 1013904223u; arena[x] = seed; }
    combinationRule = 3;
    noSource = 0; noHalftone = 1; cmFlags = 0;
    sourceForm = destForm = 0x1000;
    destDepth = sourceDepth = k.depth;
    destMSB = sourceMSB = 1;
    destPPW = sourcePPW = 32 / k.depth;
    destPitch = sourcePitch = WPR * 4;
    destBits = sourceBits = (sqInt)(arena + GUARD);
    endOfSource = endOfDestination = (usqInt)(arena + GUARD + WPR * 8);
    sx = k.sx; sy = k.sy; dx = k.dx; dy = k.dy; bbW = k.w; bbH = k.h;
    performCopyLoop();
    printf("(%d, %d, %d, %d, %d, %d, %d, vec![", k.depth, k.sx, k.sy, k.dx, k.dy, k.w, k.h);
    for (x = 0; x < WPR * 8; x++) printf("%s0x%08X", x ? ", " : "", arena[GUARD + x]);
    printf("]),\n");
  }
  return 0;
}
