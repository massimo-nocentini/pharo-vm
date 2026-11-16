/* suFFI.h -- the C interface of libSimdUTFFFI, which the package SimdUTF-FFI
 * (the submodule plugins/SimdUTFPlugin/simdutf.st) calls through the FFI of the
 * image when the VM has no SimdUTFPlugin.  It runs the engine of the plugin
 * (suEngine.cpp) unchanged.  No VM header, no object pointer, no callback, no
 * state between calls (su_init's choice aside): every argument is a number or
 * the address of the first unit of a ByteArray, ByteString or WideString that
 * the image pinned for the call.  Results go into a destination the image
 * allocated after a sizing call (su_ffi_measure, or su_ffi_base64_encoded_size).
 *
 * Every int64 answer is what the primitive of the same name answers: >= 0 on
 * success (units written, a measure, 0 for valid), else the status word
 * -(status + 4*code + 4096*payload) of docs/SimdUTF.md with positions 1-based
 * and absolute in the source object; status 0 words are the failures the
 * primitive would have: SU_FFI_BAD_ARGUMENT, SU_FFI_BAD_INDEX.  OVER_BUDGET
 * (status 1) is never answered: the image allocates. */
#ifndef SU_FFI_H
#define SU_FFI_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32)
# define SU_FFI_EXPORT __declspec(dllexport)
#else
# define SU_FFI_EXPORT __attribute__((visibility("default")))
#endif

#define SU_FFI_PROTOCOL 1
#define SU_FFI_BAD_ARGUMENT (-4)	/* -(0 + 4*1): the primitive fails with #'bad argument' */
#define SU_FFI_BAD_INDEX    (-8)	/* -(0 + 4*2): the primitive fails with #'bad index' */

SU_FFI_EXPORT int64_t su_ffi_protocol(void);		/* SU_FFI_PROTOCOL, or 0 when no usable kernel */
SU_FFI_EXPORT const char *su_ffi_simdutf_version(void);	/* "9.4.0" */
SU_FFI_EXPORT const char *su_ffi_implementation(void);	/* the kernel, after the guard */
SU_FFI_EXPORT const char *su_ffi_forced(void);		/* "", the honoured name, or "refused <value>", cut to 63 bytes */

/* source: the object's first unit; words: 0 for a ByteString (Latin-1), 1 for a
   WideString; units: the object's size; [start0, stop0): the range.  dst: the
   destination object's first unit, dstUnits its size, at0 the 0-based index.
   flags as primitiveEncode. */
SU_FFI_EXPORT int64_t su_ffi_encode(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags,
		void *dst, int64_t dstUnits, int64_t at0);
/* source: bytes; flags as primitiveDecode; dstWords: 0, a ByteString destination
   (Latin-1), 1 a WideString destination (UTF-32) */
SU_FFI_EXPORT int64_t su_ffi_decode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags,
		void *dst, int64_t dstWords, int64_t dstUnits, int64_t at0);
/* flags as primitiveMeasure; DECODED answers chars*2 + wide */
SU_FFI_EXPORT int64_t su_ffi_measure(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags);
/* flags as primitiveValidate; 0 when valid */
SU_FFI_EXPORT int64_t su_ffi_validate(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags);
SU_FFI_EXPORT int64_t su_ffi_base64_encode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t options, int64_t lineLength, int64_t lineEnd,
		void *dst, int64_t dstUnits, int64_t at0);
SU_FFI_EXPORT int64_t su_ffi_base64_decode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t options, int64_t lastChunk,
		void *dst, int64_t dstUnits, int64_t at0);
SU_FFI_EXPORT int64_t su_ffi_base64_encoded_size(int64_t n, int64_t options,
		int64_t lineLength, int64_t lineEnd);	/* SU_FFI_BAD_ARGUMENT for bad options, n < 0,
							   or a result > INT64_MAX (checked arithmetic) */
/* Every function: source == NULL with units > 0, or dst == NULL (into calls
   always need a destination; at0 = 0 and dstUnits = 0 included), answers
   SU_FFI_BAD_ARGUMENT; negative units/dstUnits are SU_FFI_BAD_INDEX. */

#ifdef __cplusplus
}
#endif
#endif
