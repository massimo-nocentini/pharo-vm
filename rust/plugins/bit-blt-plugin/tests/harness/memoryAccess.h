#ifndef MEMORY_ACCESS_H
#define MEMORY_ACCESS_H
#define long32At(a) (*((unsigned int *)(a)))
#define long32Atput(a, v) (*((unsigned int *)(a)) = (v))
#define byteAtPointer(a) (*((unsigned char *)(a)))
#define oopForPointer(p) ((sqInt)(p))
#endif
