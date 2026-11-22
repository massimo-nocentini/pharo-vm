/* cmark-gfm_export.h -- our replacement of the header that upstream's CMake
 * writes with generate_export_header(BASE_NAME cmark-gfm), for a static,
 * private copy of cmark-gfm inside the CMarkPlugin: every export macro is
 * empty, so that with hidden visibility (cmake/cmark.cmake) nothing of
 * cmark-gfm leaves the plugin.  (Upstream selects the same empty macros with
 * CMARK_GFM_STATIC_DEFINE.)  0.29.0.gfm.14 has no separate extensions export
 * header: extensions/cmark-gfm-core-extensions.h includes this one.
 */
#ifndef CMARK_GFM_EXPORT_H
#define CMARK_GFM_EXPORT_H

#define CMARK_GFM_EXPORT
#define CMARK_GFM_NO_EXPORT
#define CMARK_GFM_DEPRECATED
#define CMARK_GFM_DEPRECATED_EXPORT
#define CMARK_GFM_DEPRECATED_NO_EXPORT

#endif
