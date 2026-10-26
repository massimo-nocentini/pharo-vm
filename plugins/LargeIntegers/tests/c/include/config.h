/* Stand-in for the config.h that CMake generates (build/include/pharovm/config.h),
   so that the C tests need no configured build: the type widths and the byte order
   of the host, which the VM headers included by the plugin read. */
#ifndef PHAROVM_CONFIG_H
#define PHAROVM_CONFIG_H
#define SIZEOF_INT 4
#define SIZEOF_LONG __SIZEOF_LONG__
#define SIZEOF_LONG_LONG 8
#define SIZEOF_VOID_P __SIZEOF_POINTER__
#if !defined(LSB_FIRST)
# define LSB_FIRST (__BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__)
#endif
#define VM_LABEL(foo) 0
#endif
