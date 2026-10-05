/*
 * The FreeType modules of the Emscripten VM
 *
 * cmake/emscripten/deps/freetype.cmake compiles FreeType with
 * FT_CONFIG_MODULES_H naming this file, in place of the archive's
 * include/freetype/config/ftmodule.h, which lists every module; it compiles
 * the sources of these modules only.  src/base/ftinit.c includes it twice,
 * so it has no include guard.
 *
 * The fonts of Pharo 12 and Pharo 15 (Source Sans Pro and Source Code Pro,
 * embedded in the image) are TrueType: tt_driver_class, sfnt for the tables,
 * and the autofitter, for the light hinting the image asks for.  CFF, with
 * psaux and pshinter, is there for OpenType/CFF fonts put into the Fonts
 * directory.  Both rasterisers: smooth renders the 8-bit Forms, raster the
 * 1-bit ones.  psnames gives the glyph names (without the Adobe Glyph List,
 * which ftoption.h leaves out).
 */
FT_USE_MODULE( FT_Module_Class, autofit_module_class )
FT_USE_MODULE( FT_Driver_ClassRec, tt_driver_class )
FT_USE_MODULE( FT_Driver_ClassRec, cff_driver_class )
FT_USE_MODULE( FT_Module_Class, psaux_module_class )
FT_USE_MODULE( FT_Module_Class, psnames_module_class )
FT_USE_MODULE( FT_Module_Class, pshinter_module_class )
FT_USE_MODULE( FT_Module_Class, sfnt_module_class )
FT_USE_MODULE( FT_Renderer_Class, ft_smooth_renderer_class )
FT_USE_MODULE( FT_Renderer_Class, ft_raster1_renderer_class )
