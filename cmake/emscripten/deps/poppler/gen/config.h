/* config.h.  Generated from config.h.cmake by cmake.  */

#include "poppler-config.h"

/* Build against libcurl. */
#define ENABLE_LIBCURL 0

/* Build with libopenjpeg support. */
#define ENABLE_LIBOPENJPEG 0

/* Build with brotli support. */
#define ENABLE_BROTLI 0

/* Do not hardcode the library location */
#define ENABLE_RELOCATABLE 0

/* Build against libnss3 for digital signature validation */
#define ENABLE_NSS3 0

/* Build against libgpgme for digital signature validation */
#define ENABLE_GPGME 0

/* Enable pgp signatures in GPG backend by default */
#define ENABLE_PGP_SIGNATURES 0

/* Signatures enabled */
#define ENABLE_SIGNATURES 0

/* Default signature backend */
#define DEFAULT_SIGNATURE_BACKEND "None"

/* Define to 1 if you have the `fseek64' function. */
#define HAVE_FSEEK64 0

/* Define to 1 if fseeko (and presumably ftello) exists and is declared. */
#define HAVE_FSEEKO 1

/* Define to 1 if you have the `pread64' function. */
#define HAVE_PREAD64 0

/* Define to 1 if you have the `lseek64' function. */
#define HAVE_LSEEK64 0

/* Defines if gmtime_r is available on your system */
#define HAVE_GMTIME_R 1

/* Defines if timegm is available on your system */
#define HAVE_TIMEGM 1

/* Defines if localtime_r is available on your system */
#define HAVE_LOCALTIME_R 1

/* Defines if strtok_r is available on your system */
#define HAVE_STRTOK_R 1

/* Defines if tcgetattr is available on your system */
#define HAVE_TCGETATTR 1

/* Define to 1 if you have a big endian machine */
#define WORDS_BIGENDIAN 0

/* Define as const if the declaration of iconv() needs const. */
#define ICONV_CONST 

/* Name of package */
#define PACKAGE "poppler"

/* Define to the address where bug reports for this package should be sent. */
#define PACKAGE_BUGREPORT "https://bugs.freedesktop.org/enter_bug.cgi?product=poppler"

/* Define to the full name of this package. */
#define PACKAGE_NAME "poppler"

/* Define to the full name and version of this package. */
#define PACKAGE_STRING "poppler 26.11.0"

/* Define to the one symbol short name of this package. */
#define PACKAGE_TARNAME "poppler"

/* Define to the home page for this package. */
#define PACKAGE_URL ""

/* Define to the version of this package. */
#define PACKAGE_VERSION "26.11.0"

/* Poppler data dir */
#define POPPLER_DATADIR "/usr/share/poppler"

/* Version number of package */
#define VERSION "26.11.0"

/* Use fontconfig font configuration backend */
#define WITH_FONTCONFIGURATION_FONTCONFIG 1

/* Use win32 font configuration backend */
#define WITH_FONTCONFIGURATION_WIN32 0

/* Use android font configuration backend */
#define WITH_FONTCONFIGURATION_ANDROID 0

/* MS defined snprintf as deprecated but then added it in Visual Studio 2015. */
#if defined(_MSC_VER) && _MSC_VER < 1900
#define snprintf _snprintf
#endif

//------------------------------------------------------------------------
// popen
//------------------------------------------------------------------------
#if defined(_MSC_VER) || defined(__BORLANDC__)
#define popen _popen
#define pclose _pclose
#define strncasecmp _strnicmp
#define strcasecmp _stricmp
#endif

/* Number of bits in a file offset, on hosts where this is settable. */
/* #undef _FILE_OFFSET_BITS */

/* Define to 1 to make fseeko visible on some hosts (e.g. glibc 2.2). */
/* TODO This is wrong, port if needed #undef _LARGEFILE_SOURCE */

/* Define for large files, on AIX-style hosts. */
/* TODO This is wrong, port if needed #undef _LARGE_FILES */

/* Build with support for font subsetting (uses Harfbuzz) */
#define ENABLE_HARFBUZZ 0
