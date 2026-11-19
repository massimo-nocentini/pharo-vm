/*
 * The text and PDF probe of the cairo stack (scripts/cairo-stack/check.sh,
 * docs/CairoStack.md): Pango with PangoCairo, and Poppler's glib wrapper.
 * Like probe.c it loads the stack's libraries by absolute path with dlopen,
 * as the image's FFI does, so no header and no link to the stack is needed.
 *
 *     cc -std=c99 -D_GNU_SOURCE -o probe-text probe-text.c -ldl     (Linux)
 *     cc -std=c99 -D_DARWIN_C_SOURCE -o probe-text probe-text.c     (macOS)
 *     probe-text <dir of the libraries> <scratch dir> [pango] [poppler]
 *
 * pango: a layout of "Ag" and a Hebrew word in "Sans 20", drawn through
 * PangoCairo on an ARGB32 surface.  poppler: a 2-page PDF that cairo's PDF
 * surface writes into <scratch dir> (page 1 toy text; page 2 a green image
 * surface carrying the JPEG data of a red square, so that only Poppler's DCT
 * decoder makes the page red), opened from a GBytes; then a hand-written PDF
 * whose Helvetica is not embedded (a base-14 font: Poppler substitutes a
 * system font through fontconfig).  It prints
 *
 *     pango <version> font-map <type> family <family> ink <n>
 *     poppler <version> pages <n> size <w>x<h> text "<page 1>" ink <n> jpeg <r>,<g>,<b>
 *     base14 text "<page 1>" ink <n>
 *     instances ok          (or: instances FAIL <what>)
 *
 * where ink is the number of pixels drawn, and "instances" says that each of
 * libglib, libgobject, libcairo, libfreetype, libharfbuzz, libpango and
 * libfontconfig that these parts load is loaded once, from <dir>.  It exits 0
 * when every part draws ink, the texts extract, the JPEG pixel is red and the
 * instances are right.  check.sh compares the versions with the pins and
 * asserts that stderr stays empty (no Poppler or fontconfig message).
 */
#include <dlfcn.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#define SO(linux_name, macos_name) macos_name
#else
#include <link.h>
#define SO(linux_name, macos_name) linux_name
#endif

typedef void *ptr;

static const char *dir;
static char dir_real[PATH_MAX];

static void *open_lib(const char *name)
{
    char path[PATH_MAX];
    snprintf(path, sizeof path, "%s/%s", dir, name);
    void *h = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!h) {
        fprintf(stderr, "probe-text: %s\n", dlerror());
        exit(2);
    }
    return h;
}

static void *sym(void *lib, const char *name)
{
    void *p = dlsym(lib, name);
    if (!p) {
        fprintf(stderr, "probe-text: %s: %s\n", name, dlerror());
        exit(2);
    }
    return p;
}

/* POSIX's way to assign dlsym's result to a function pointer */
#define LOAD(lib, var, name) (*(void **)&(var) = sym((lib), (name)))

/* The fixed 32x32 baseline JPEG of solid #C00000 (libjpeg-turbo 3.2.1,
 * quality 90, standard Huffman tables) */
static const unsigned char red_jpeg[646] = {
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xff, 0xdb, 0x00, 0x43, 0x00, 0x03, 0x02, 0x02, 0x03, 0x02, 0x02, 0x03,
    0x03, 0x03, 0x03, 0x04, 0x03, 0x03, 0x04, 0x05, 0x08, 0x05, 0x05, 0x04, 0x04, 0x05, 0x0a, 0x07,
    0x07, 0x06, 0x08, 0x0c, 0x0a, 0x0c, 0x0c, 0x0b, 0x0a, 0x0b, 0x0b, 0x0d, 0x0e, 0x12, 0x10, 0x0d,
    0x0e, 0x11, 0x0e, 0x0b, 0x0b, 0x10, 0x16, 0x10, 0x11, 0x13, 0x14, 0x15, 0x15, 0x15, 0x0c, 0x0f,
    0x17, 0x18, 0x16, 0x14, 0x18, 0x12, 0x14, 0x15, 0x14, 0xff, 0xdb, 0x00, 0x43, 0x01, 0x03, 0x04,
    0x04, 0x05, 0x04, 0x05, 0x09, 0x05, 0x05, 0x09, 0x14, 0x0d, 0x0b, 0x0d, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14,
    0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0x14, 0xff, 0xc0,
    0x00, 0x11, 0x08, 0x00, 0x20, 0x00, 0x20, 0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11,
    0x01, 0xff, 0xc4, 0x00, 0x1f, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
    0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05,
    0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7d, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
    0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23,
    0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17,
    0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
    0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a,
    0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a,
    0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
    0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7,
    0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5,
    0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1,
    0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xff, 0xc4, 0x00, 0x1f, 0x01, 0x00, 0x03,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x11, 0x00,
    0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77, 0x00,
    0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13,
    0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15,
    0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27,
    0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6,
    0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4,
    0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9,
    0xfa, 0xff, 0xda, 0x00, 0x0c, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00, 0xf9,
    0x0a, 0x8a, 0x28, 0xaf, 0xca, 0x8f, 0xf4, 0x00, 0x28, 0xa2, 0x8a, 0x00, 0x28, 0xa2, 0x8a, 0x00,
    0x28, 0xa2, 0x8a, 0x00, 0xff, 0xd9,
};

/* A one-page PDF (200x50) showing "Base14 Pharo" in a base-14 Helvetica that
 * is not embedded; the same literal as tests/cairo-stack/ci-text.st's */
static const char base14_pdf[] =
    "%PDF-1.4\n"
    "1 0 obj\n"
    "<< /Type /Catalog /Pages 2 0 R >>\n"
    "endobj\n"
    "2 0 obj\n"
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n"
    "endobj\n"
    "3 0 obj\n"
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 50] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\n"
    "endobj\n"
    "4 0 obj\n"
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n"
    "endobj\n"
    "5 0 obj\n"
    "<< /Length 42 >>\n"
    "stream\n"
    "BT /F1 24 Tf 10 16 Td (Base14 Pharo) Tj ET\n"
    "endstream\n"
    "endobj\n"
    "xref\n"
    "0 6\n"
    "0000000000 65535 f \n"
    "0000000009 00000 n \n"
    "0000000058 00000 n \n"
    "0000000115 00000 n \n"
    "0000000240 00000 n \n"
    "0000000310 00000 n \n"
    "trailer\n"
    "<< /Size 6 /Root 1 0 R >>\n"
    "startxref\n"
    "402\n"
    "%%EOF\n";

enum { ARGB32 = 0, RGB24 = 1 };

/* cairo */
static ptr (*image_surface_create)(int, int, int);
static ptr (*create)(ptr);
static void (*destroy)(ptr);
static void (*surface_destroy)(ptr);
static void (*surface_flush)(ptr);
static void (*surface_finish)(ptr);
static int (*surface_status)(ptr);
static unsigned char *(*get_data)(ptr);
static int (*get_stride)(ptr);
static void (*set_source_rgb)(ptr, double, double, double);
static void (*paint)(ptr);
static void (*move_to)(ptr, double, double);
static void (*save)(ptr);
static void (*restore)(ptr);
static void (*scale)(ptr, double, double);
static void (*set_source_surface)(ptr, ptr, double, double);
static int (*surface_set_mime_data)(ptr, const char *, const unsigned char *, unsigned long, void (*)(void *), void *);
static ptr (*pdf_surface_create)(const char *, double, double);
static void (*select_font_face)(ptr, const char *, int, int);
static void (*set_font_size)(ptr, double);
static void (*show_text)(ptr, const char *);
static void (*show_page)(ptr);
/* GLib, GObject */
static ptr (*g_bytes_new)(const void *, size_t);
static void (*g_bytes_unref)(ptr);
static void (*g_free)(ptr);
static void (*g_error_free)(ptr);
static void (*g_object_unref)(ptr);
static const char *(*g_type_name_from_instance)(ptr);

/* GError: domain, code, message */
struct gerror { uint32_t domain; int code; char *message; };

static void load_core(void)
{
    void *cairo = open_lib(SO("libcairo.so.2", "libcairo.2.dylib"));
    void *glib = open_lib(SO("libglib-2.0.so.0", "libglib-2.0.0.dylib"));
    void *gobject = open_lib(SO("libgobject-2.0.so.0", "libgobject-2.0.0.dylib"));
    LOAD(cairo, image_surface_create, "cairo_image_surface_create");
    LOAD(cairo, create, "cairo_create");
    LOAD(cairo, destroy, "cairo_destroy");
    LOAD(cairo, surface_destroy, "cairo_surface_destroy");
    LOAD(cairo, surface_flush, "cairo_surface_flush");
    LOAD(cairo, surface_finish, "cairo_surface_finish");
    LOAD(cairo, surface_status, "cairo_surface_status");
    LOAD(cairo, get_data, "cairo_image_surface_get_data");
    LOAD(cairo, get_stride, "cairo_image_surface_get_stride");
    LOAD(cairo, set_source_rgb, "cairo_set_source_rgb");
    LOAD(cairo, paint, "cairo_paint");
    LOAD(cairo, move_to, "cairo_move_to");
    LOAD(cairo, save, "cairo_save");
    LOAD(cairo, restore, "cairo_restore");
    LOAD(cairo, scale, "cairo_scale");
    LOAD(cairo, set_source_surface, "cairo_set_source_surface");
    LOAD(cairo, surface_set_mime_data, "cairo_surface_set_mime_data");
    LOAD(cairo, pdf_surface_create, "cairo_pdf_surface_create");
    LOAD(cairo, select_font_face, "cairo_select_font_face");
    LOAD(cairo, set_font_size, "cairo_set_font_size");
    LOAD(cairo, show_text, "cairo_show_text");
    LOAD(cairo, show_page, "cairo_show_page");
    LOAD(glib, g_bytes_new, "g_bytes_new");
    LOAD(glib, g_bytes_unref, "g_bytes_unref");
    LOAD(glib, g_free, "g_free");
    LOAD(glib, g_error_free, "g_error_free");
    LOAD(gobject, g_object_unref, "g_object_unref");
    LOAD(gobject, g_type_name_from_instance, "g_type_name_from_instance");
}

static uint32_t pixel(ptr surface, int x, int y)
{
    uint32_t p;
    memcpy(&p, get_data(surface) + y * get_stride(surface) + 4 * x, 4);
    return p; /* 0xAARRGGBB in native endianness */
}

/* The pixels drawn: alpha > 0 on a transparent surface, else not white */
static int ink_of(ptr surface, int w, int h, int on_white)
{
    int ink = 0;
    surface_flush(surface);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            uint32_t p = pixel(surface, x, y);
            if (on_white ? (p & 0xffffff) != 0xffffff : (p >> 24) != 0)
                ink++;
        }
    return ink;
}

/* "text" with newlines and quotes escaped */
static void print_quoted(const char *s)
{
    putchar('"');
    for (; s && *s; s++) {
        if (*s == '\n') fputs("\\n", stdout);
        else if (*s == '"' || *s == '\\') { putchar('\\'); putchar(*s); }
        else putchar(*s);
    }
    putchar('"');
}

static int run_pango(void)
{
    void *pango = open_lib(SO("libpango-1.0.so.0", "libpango-1.0.0.dylib"));
    void *pangocairo = open_lib(SO("libpangocairo-1.0.so.0", "libpangocairo-1.0.0.dylib"));
    const char *(*version_string)(void);
    ptr (*font_map_create_context)(ptr);
    ptr (*font_description_from_string)(const char *);
    void (*font_description_free)(ptr);
    ptr (*context_load_font)(ptr, ptr);
    ptr (*font_describe)(ptr);
    const char *(*font_description_get_family)(ptr);
    void (*layout_set_font_description)(ptr, ptr);
    void (*layout_set_text)(ptr, const char *, int);
    ptr (*font_map_get_default)(void);
    ptr (*create_layout)(ptr);
    void (*show_layout)(ptr, ptr);
    LOAD(pango, version_string, "pango_version_string");
    LOAD(pango, font_map_create_context, "pango_font_map_create_context");
    LOAD(pango, font_description_from_string, "pango_font_description_from_string");
    LOAD(pango, font_description_free, "pango_font_description_free");
    LOAD(pango, context_load_font, "pango_context_load_font");
    LOAD(pango, font_describe, "pango_font_describe");
    LOAD(pango, font_description_get_family, "pango_font_description_get_family");
    LOAD(pango, layout_set_font_description, "pango_layout_set_font_description");
    LOAD(pango, layout_set_text, "pango_layout_set_text");
    LOAD(pangocairo, font_map_get_default, "pango_cairo_font_map_get_default");
    LOAD(pangocairo, create_layout, "pango_cairo_create_layout");
    LOAD(pangocairo, show_layout, "pango_cairo_show_layout");

    ptr font_map = font_map_get_default(); /* owned by Pango */
    const char *type = g_type_name_from_instance(font_map);
    ptr desc = font_description_from_string("Sans 20");
    char family[256] = "?";
    ptr context = font_map_create_context(font_map);
    ptr font = context_load_font(context, desc);
    if (font) {
        ptr d = font_describe(font);
        const char *f = font_description_get_family(d);
        snprintf(family, sizeof family, "%s", f ? f : "?");
        font_description_free(d);
        g_object_unref(font);
    }
    g_object_unref(context);

    enum { W = 160, H = 40 };
    ptr surface = image_surface_create(ARGB32, W, H);
    ptr cr = create(surface);
    set_source_rgb(cr, 0, 0, 0);
    ptr layout = create_layout(cr);
    layout_set_font_description(layout, desc);
    layout_set_text(layout, "Ag \xd7\xa9\xd7\x9c\xd7\x95\xd7\x9d", -1);
    move_to(cr, 4, 4);
    show_layout(cr, layout);
    int ink = ink_of(surface, W, H, 0);
    g_object_unref(layout);
    font_description_free(desc);
    destroy(cr);
    surface_destroy(surface);
    printf("pango %s font-map %s family %s ink %d\n", version_string(), type ? type : "?", family, ink);
    return ink > 0 && font != NULL;
}

static void *poppler;
static ptr (*document_new_from_bytes)(ptr, const char *, struct gerror **);
static int (*document_get_n_pages)(ptr);
static ptr (*document_get_page)(ptr, int);
static void (*page_get_size)(ptr, double *, double *);
static char *(*page_get_text)(ptr);
static void (*page_render)(ptr, ptr);

/* Page <index> rendered on a white w x h surface (the caller destroys it) */
static ptr render(ptr doc, int index, int w, int h)
{
    ptr page = document_get_page(doc, index);
    ptr surface = image_surface_create(ARGB32, w, h);
    ptr cr = create(surface);
    set_source_rgb(cr, 1, 1, 1);
    paint(cr);
    if (page) {
        page_render(page, cr);
        g_object_unref(page);
    }
    destroy(cr);
    surface_flush(surface);
    return surface;
}

static char *text_of(ptr doc, int index)
{
    ptr page = document_get_page(doc, index);
    char *text = page ? page_get_text(page) : NULL;
    if (page) g_object_unref(page);
    return text; /* g_free */
}

static ptr open_bytes(const void *data, size_t length, const char *what)
{
    ptr bytes = g_bytes_new(data, length);
    struct gerror *error = NULL;
    ptr doc = document_new_from_bytes(bytes, NULL, &error);
    g_bytes_unref(bytes); /* the document holds its own reference */
    if (!doc) {
        printf("%s error %s\n", what, error && error->message ? error->message : "?");
        if (error) g_error_free(error);
    }
    return doc;
}

static int run_poppler(const char *scratch)
{
    poppler = open_lib(SO("libpoppler-glib.so.8", "libpoppler-glib.8.dylib"));
    const char *(*get_version)(void);
    LOAD(poppler, get_version, "poppler_get_version");
    LOAD(poppler, document_new_from_bytes, "poppler_document_new_from_bytes");
    LOAD(poppler, document_get_n_pages, "poppler_document_get_n_pages");
    LOAD(poppler, document_get_page, "poppler_document_get_page");
    LOAD(poppler, page_get_size, "poppler_page_get_size");
    LOAD(poppler, page_get_text, "poppler_page_get_text");
    LOAD(poppler, page_render, "poppler_page_render");
    int ok = 1;

    /* The 2-page PDF, written by cairo */
    char path[PATH_MAX];
    snprintf(path, sizeof path, "%s/probe-text.pdf", scratch);
    ptr pdf = pdf_surface_create(path, 300, 200);
    ptr cr = create(pdf);
    set_source_rgb(cr, 0, 0, 0);
    select_font_face(cr, "sans-serif", 0, 0);
    set_font_size(cr, 20);
    move_to(cr, 20, 100);
    show_text(cr, "Hello Poppler \xc3\xa9 \xe2\x82\xac");
    show_page(cr);
    ptr image = image_surface_create(RGB24, 32, 32);
    ptr ic = create(image);
    set_source_rgb(ic, 0, 1, 0);
    paint(ic);
    destroy(ic);
    surface_set_mime_data(image, "image/jpeg", red_jpeg, sizeof red_jpeg, NULL, NULL);
    save(cr);
    scale(cr, 300 / 32.0, 200 / 32.0);
    set_source_surface(cr, image, 0, 0);
    paint(cr);
    restore(cr);
    show_page(cr);
    destroy(cr);
    surface_finish(pdf);
    int status = surface_status(pdf);
    surface_destroy(pdf);
    surface_destroy(image);
    if (status != 0) {
        printf("poppler %s error cairo PDF surface status %d\n", get_version(), status);
        return 0;
    }

    FILE *f = fopen(path, "rb");
    if (!f) { printf("poppler %s error cannot read %s\n", get_version(), path); return 0; }
    fseek(f, 0, SEEK_END);
    long length = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *data = malloc(length > 0 ? length : 1);
    size_t got = fread(data, 1, length > 0 ? length : 0, f);
    fclose(f);
    ptr doc = open_bytes(data, got, "poppler");
    free(data);
    if (!doc) return 0;
    double w = 0, h = 0;
    ptr page = document_get_page(doc, 0);
    if (page) { page_get_size(page, &w, &h); g_object_unref(page); }
    int pages = document_get_n_pages(doc);
    char *text = text_of(doc, 0);
    ptr s1 = render(doc, 0, 300, 200);
    int ink = ink_of(s1, 300, 200, 1);
    surface_destroy(s1);
    ptr s2 = render(doc, 1, 300, 200);
    uint32_t p = pixel(s2, 150, 100);
    surface_destroy(s2);
    int r = (p >> 16) & 255, g = (p >> 8) & 255, b = p & 255;
    printf("poppler %s pages %d size %.0fx%.0f text ", get_version(), pages, w, h);
    print_quoted(text);
    printf(" ink %d jpeg %d,%d,%d\n", ink, r, g, b);
    ok = pages == 2 && w == 300 && h == 200 && text && strstr(text, "Hello Poppler") && ink > 0
        && r > 150 && g < 60 && b < 60;
    g_free(text);
    g_object_unref(doc);

    /* The base-14 PDF */
    doc = open_bytes(base14_pdf, sizeof base14_pdf - 1, "base14");
    if (!doc) return 0;
    text = text_of(doc, 0);
    ptr s3 = render(doc, 0, 200, 50);
    ink = ink_of(s3, 200, 50, 1);
    surface_destroy(s3);
    printf("base14 text ");
    print_quoted(text);
    printf(" ink %d\n", ink);
    ok = ok && text && strstr(text, "Base14 Pharo") && ink > 0;
    g_free(text);
    g_object_unref(doc);
    return ok;
}

/* The libraries that must be loaded once, from <dir> */
static struct instance {
    const char *prefix;
    int expected, count, outside;
} instances[] = {
    { SO("libglib-2.0.so", "libglib-2.0."), 1, 0, 0 },
    { SO("libgobject-2.0.so", "libgobject-2.0."), 1, 0, 0 },
    { SO("libcairo.so", "libcairo."), 1, 0, 0 },
    { SO("libfreetype.so", "libfreetype."), 1, 0, 0 },
    { SO("libharfbuzz.so", "libharfbuzz."), 0, 0, 0 },
    { SO("libpango-1.0.so", "libpango-1.0."), 0, 0, 0 },
    { SO("libfontconfig.so", "libfontconfig."), 0, 0, 0 },
};
enum { N_INSTANCES = sizeof instances / sizeof instances[0], HARFBUZZ = 4, PANGO = 5, FONTCONFIG = 6 };

static void count_image(const char *name)
{
    if (!name || !*name) return;
    const char *base = strrchr(name, '/');
    base = base ? base + 1 : name;
    for (int i = 0; i < N_INSTANCES; i++) {
        if (strncmp(base, instances[i].prefix, strlen(instances[i].prefix)) != 0) continue;
        instances[i].count++;
        char d[PATH_MAX], real[PATH_MAX];
        snprintf(d, sizeof d, "%.*s", (int)(base - name), name);
        if (!*d || !realpath(d, real) || strcmp(real, dir_real) != 0)
            instances[i].outside++;
    }
}

#ifdef __APPLE__
static void count_images(void)
{
    for (uint32_t i = 0; i < _dyld_image_count(); i++)
        count_image(_dyld_get_image_name(i));
}
#else
static int count_phdr(struct dl_phdr_info *info, size_t size, void *data)
{
    (void)size;
    (void)data;
    count_image(info->dlpi_name);
    return 0;
}
static void count_images(void) { dl_iterate_phdr(count_phdr, NULL); }
#endif

static int check_instances(void)
{
    char what[1024] = "";
    count_images();
    for (int i = 0; i < N_INSTANCES; i++) {
        struct instance *in = &instances[i];
        char one[160];
        if (in->count > 1 || (in->expected && in->count == 0) || in->outside) {
            snprintf(one, sizeof one, "%s%s* loaded %d times, %d outside the directory",
                     *what ? "; " : "", in->prefix, in->count, in->outside);
            strncat(what, one, sizeof what - strlen(what) - 1);
        }
    }
    printf("instances %s%s\n", *what ? "FAIL " : "ok", what);
    return !*what;
}

int main(int argc, char **argv)
{
    int pango = 0, poppler_part = 0, ok = 1;
    if (argc < 3) {
        fprintf(stderr, "usage: probe-text <dir of the libraries> <scratch dir> [pango] [poppler]\n");
        return 2;
    }
    dir = argv[1];
    if (!realpath(dir, dir_real)) {
        fprintf(stderr, "probe-text: no directory %s\n", dir);
        return 2;
    }
    for (int i = 3; i < argc; i++) {
        if (strcmp(argv[i], "pango") == 0) pango = 1;
        else if (strcmp(argv[i], "poppler") == 0) poppler_part = 1;
        else { fprintf(stderr, "probe-text: unknown part %s\n", argv[i]); return 2; }
    }
    instances[HARFBUZZ].expected = instances[PANGO].expected = pango;
#ifdef __APPLE__
    instances[FONTCONFIG].expected = poppler_part; /* cairo and Pango use CoreText */
#else
    instances[FONTCONFIG].expected = 1; /* cairo's font backend */
#endif
    load_core();
    if (pango) ok = run_pango() && ok;
    if (poppler_part) ok = run_poppler(argv[2]) && ok;
    ok = check_instances() && ok;
    fflush(stdout);
    return ok ? 0 : 1;
}
