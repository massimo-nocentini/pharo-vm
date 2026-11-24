/* The types of windows.h (minwindef.h, winnt.h, windef.h) whose names are
 * likely names of constants.  sqPlatformSpecific.h includes windows.h on
 * Windows only; glue.sh compiles the glue once more after this file, so that a
 * clash (enum { ..., FLOAT, ... }) fails everywhere. */
typedef int BOOL;
typedef unsigned char BOOLEAN;
typedef unsigned char BYTE;
typedef char CHAR;
typedef unsigned int DWORD;
typedef float FLOAT;
typedef void *HANDLE;
typedef int INT;
typedef int LONG;
typedef long long LONGLONG;
typedef short SHORT;
typedef char TCHAR;
typedef unsigned char UCHAR;
typedef unsigned int UINT;
typedef unsigned int ULONG;
typedef unsigned short USHORT;
typedef unsigned short WCHAR;
typedef unsigned short WORD;
#undef VOID
#define VOID void
#define CONST const
#define ERROR 0
#define IN
#define OUT
#define OPTIONAL
#define NO_ERROR 0L
