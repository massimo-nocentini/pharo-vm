/*
 * The toy-text probe of the cairo stack (scripts/cairo-stack/check.sh,
 * docs/CairoStack.md): loads <dir>/libcairo.so.2 (or the file given) with
 * dlopen, as the image's FFI does, so no header and no link to the stack is
 * needed, and draws "Ag" with cairo's toy font API on an ARGB32 surface.
 *
 *     cc -o probe probe.c -ldl        (-ldl: glibc < 2.34 only)
 *     probe <dir or libcairo path> [family]
 *
 * It prints
 *
 *     probe cairo <cairo_version_string()>
 *     probe toy <status> family <family> type <ft|user|...> ink <n> first-call-ms <t>
 *
 * where family is the family of the face cairo picked (FreeType's family
 * name; "twin" when cairo fell back to its built-in stroke font, type user),
 * ink the number of pixels with alpha > 0, and first-call-ms the time of the
 * first cairo_show_text (fontconfig's initialisation and font scan included).
 * It exits 0 when cairo's status is success and ink > 0.
 */
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

typedef void *ptr;

static void *lib;

static void *sym(const char *name)
{
    void *p = dlsym(lib, name);
    if (!p) {
        fprintf(stderr, "probe: %s: %s\n", name, dlerror());
        exit(2);
    }
    return p;
}

static double now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv)
{
    char path[4096];
    const char *family = argc > 2 ? argv[2] : "sans-serif";

    if (argc < 2) {
        fprintf(stderr, "usage: probe <dir with libcairo.so.2 | libcairo path> [family]\n");
        return 2;
    }
    struct stat st_arg;
    if (stat(argv[1], &st_arg) == 0 && S_ISDIR(st_arg.st_mode))
        snprintf(path, sizeof path, "%s/libcairo.so.2", argv[1]);
    else
        snprintf(path, sizeof path, "%s", argv[1]);
    lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "probe: %s\n", dlerror());
        return 2;
    }

    const char *(*version_string)(void) = (const char *(*)(void))sym("cairo_version_string");
    ptr (*surface_create)(int, int, int) = (ptr (*)(int, int, int))sym("cairo_image_surface_create");
    ptr (*create)(ptr) = (ptr (*)(ptr))sym("cairo_create");
    void (*select_font_face)(ptr, const char *, int, int) = (void (*)(ptr, const char *, int, int))sym("cairo_select_font_face");
    void (*set_font_size)(ptr, double) = (void (*)(ptr, double))sym("cairo_set_font_size");
    void (*set_source_rgb)(ptr, double, double, double) = (void (*)(ptr, double, double, double))sym("cairo_set_source_rgb");
    void (*move_to)(ptr, double, double) = (void (*)(ptr, double, double))sym("cairo_move_to");
    void (*show_text)(ptr, const char *) = (void (*)(ptr, const char *))sym("cairo_show_text");
    int (*status)(ptr) = (int (*)(ptr))sym("cairo_status");
    const char *(*status_to_string)(int) = (const char *(*)(int))sym("cairo_status_to_string");
    ptr (*get_scaled_font)(ptr) = (ptr (*)(ptr))sym("cairo_get_scaled_font");
    int (*scaled_font_get_type)(ptr) = (int (*)(ptr))sym("cairo_scaled_font_get_type");
    void (*surface_flush)(ptr) = (void (*)(ptr))sym("cairo_surface_flush");
    unsigned char *(*get_data)(ptr) = (unsigned char *(*)(ptr))sym("cairo_image_surface_get_data");
    int (*get_stride)(ptr) = (int (*)(ptr))sym("cairo_image_surface_get_stride");
    void (*destroy)(ptr) = (void (*)(ptr))sym("cairo_destroy");
    void (*surface_destroy)(ptr) = (void (*)(ptr))sym("cairo_surface_destroy");
    /* cairo-ft, present when cairo has the FreeType backend */
    ptr (*ft_lock_face)(ptr) = (ptr (*)(ptr))dlsym(lib, "cairo_ft_scaled_font_lock_face");
    void (*ft_unlock_face)(ptr) = (void (*)(ptr))dlsym(lib, "cairo_ft_scaled_font_unlock_face");

    printf("probe cairo %s\n", version_string());

    enum { W = 64, H = 32, ARGB32 = 0 };
    ptr surface = surface_create(ARGB32, W, H);
    ptr cr = create(surface);
    set_source_rgb(cr, 0, 0, 0);
    select_font_face(cr, family, 0, 0);
    set_font_size(cr, 20);
    move_to(cr, 4, 24);
    double t0 = now_ms();
    show_text(cr, "Ag");
    double t1 = now_ms();
    int st = status(cr);

    const char *type = "?";
    char face_family[256] = "?";
    ptr sf = get_scaled_font(cr);
    switch (scaled_font_get_type(sf)) {
    case 0: type = "toy"; break;
    case 1: type = "ft"; break;
    case 2: type = "win32"; break;
    case 3: type = "quartz"; break;
    case 4: type = "user"; snprintf(face_family, sizeof face_family, "twin"); break;
    case 5: type = "dwrite"; break;
    }
    if (scaled_font_get_type(sf) == 1 && ft_lock_face && ft_unlock_face) {
        /* FT_FaceRec: num_faces, face_index, face_flags, style_flags,
         * num_glyphs (FT_Long each), then family_name: stable since FreeType 2.0 */
        char *face = ft_lock_face(sf);
        if (face) {
            const char *name = *(const char **)(face + 5 * sizeof(long));
            snprintf(face_family, sizeof face_family, "%s", name ? name : "?");
            ft_unlock_face(sf);
        }
    }

    surface_flush(surface);
    unsigned char *data = get_data(surface);
    int stride = get_stride(surface), ink = 0;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++)
            if (data[y * stride + 4 * x + 3] != 0) /* little-endian ARGB32: alpha last */
                ink++;
    printf("probe toy %s family %s type %s ink %d first-call-ms %.1f\n",
           st == 0 ? "ok" : status_to_string(st), face_family, type, ink, t1 - t0);
    destroy(cr);
    surface_destroy(surface);
    return st == 0 && ink > 0 ? 0 : 1;
}
