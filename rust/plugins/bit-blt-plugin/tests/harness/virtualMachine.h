#ifndef VIRTUAL_MACHINE_H
#define VIRTUAL_MACHINE_H
#include <stdint.h>
typedef intptr_t sqInt;
typedef uintptr_t usqInt;
typedef intptr_t sqIntptr_t;
typedef uintptr_t usqIntptr_t;
typedef long long sqLong;
typedef unsigned long long usqLong;
#define VM_PROXY_MAJOR 1
#define VM_PROXY_MINOR 15
#define BytesPerOop 8
struct VirtualMachine {
  sqInt (*majorVersion)(void);
  sqInt (*minorVersion)(void);
  sqInt (*byteSizeOf)(sqInt);
  sqInt (*failed)(void);
  sqInt (*fetchIntegerofObject)(sqInt, sqInt);
  sqInt (*fetchLong32ofObject)(sqInt, sqInt);
  sqInt (*fetchPointerofObject)(sqInt, sqInt);
  void *(*firstIndexableField)(sqInt);
  double (*floatValueOf)(sqInt);
  sqInt (*integerObjectOf)(sqInt);
  sqInt (*integerValueOf)(sqInt);
  void *(*ioLoadFunctionFrom)(char *, char *);
  sqInt (*isArray)(sqInt);
  sqInt (*isBytes)(sqInt);
  sqInt (*isIntegerObject)(sqInt);
  sqInt (*isPointers)(sqInt);
  sqInt (*isPositiveMachineIntegerObject)(sqInt);
  sqInt (*isWords)(sqInt);
  sqInt (*isWordsOrBytes)(sqInt);
  sqInt (*methodArgumentCount)(void);
  sqInt (*methodReturnInteger)(sqInt);
  sqInt (*methodReturnReceiver)(void);
  sqInt (*nilObject)(void);
  sqInt (*pop)(sqInt);
  void (*popthenPush)(sqInt, sqInt);
  sqInt (*positive32BitIntegerFor)(unsigned int);
  unsigned int (*positive32BitValueOf)(sqInt);
  usqLong (*positive64BitValueOf)(sqInt);
  sqInt (*primitiveFail)(void);
  sqInt (*primitiveFailFor)(sqInt);
  sqInt (*slotSizeOf)(sqInt);
  sqInt (*stackIntegerValue)(sqInt);
  sqInt (*stackObjectValue)(sqInt);
  sqInt (*stackValue)(sqInt);
  sqInt (*statNumGCs)(void);
  sqInt (*storeIntegerofObjectwithValue)(sqInt, sqInt, sqInt);
};
#endif
