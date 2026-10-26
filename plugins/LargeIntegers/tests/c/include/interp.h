/* Stand-in for the interp.h that Slang generates for the 64-bit Spur VM
   (generated/64/vm/include/interp.h): the definitions LargeIntegers.c and the
   headers it includes use.  The tests are 64-bit only (sqInt is 8 bytes). */
#ifndef INTERP_H__
#define INTERP_H__
#define VM_PROXY_MAJOR 1
#define VM_PROXY_MINOR 15
#define SQ_VI_BYTES_PER_WORD 8
#define BaseHeaderSize 8
#define BytesPerOop 8
#define BytesPerWord 8
#define PrimErrGenericFailure 1
#define PrimErrBadReceiver 2
#define PrimErrBadArgument 3
#define PrimErrBadIndex 4
#define PrimErrBadNumArgs 5
#define PrimErrNoMemory 9
#define MinSmallInteger -1152921504606846976
#define MaxSmallInteger 1152921504606846975
#define NumSmallIntegerTagBits 3
#endif
