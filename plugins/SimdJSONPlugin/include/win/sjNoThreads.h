/* sjNoThreads.h -- included first (-include) in simdjson.cpp and
 * sjEngine.cpp on Windows, by cmake/simdjson.cmake.
 *
 * simdjson.h turns its threads on when _REENTRANT or _MT is defined.  The
 * VM's COMMON_FLAGS define _MT, and the mingw-w64 _mingw.h, which every
 * system header includes, defines it again whenever it is missing, so -U_MT
 * on the command line does not last.  _mingw.h has an include guard: once it
 * has been read and _MT is gone, no header defines it again (none of the
 * mingw-w64 or libc++ headers tests it), and simdjson builds without threads,
 * as on the other platforms. */

#include <_mingw.h>
#undef _MT
