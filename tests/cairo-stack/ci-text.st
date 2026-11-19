"Draw text with the Pango and render PDFs with the Poppler of the VM's cairo stack (docs/CairoStack.md),
as the CI jobs do with the VMMaker image (Pharo 13.1, whose Athens-Cairo draws headless).  The file is
self-contained: it never loads libpangocairo.st or libpoppler.st, and compiles its own uFFI callouts
(ffiCall:module:, one module per library, the library where the function is defined) into the class
side of a probe class PCSCiText.

Each library is resolved by the stock finder of the platform, by the names the images use
(FFIUnix64LibraryFinder, FFIMacLibraryFinder or FFIWindowsLibraryFinder findAnyLibrary:), and must lie
inside the VM bundle: the directory of the VM on Linux and Windows, Contents/MacOS/Plugins on macOS.
Before the first Pango (resp. Poppler) call it checks, in a minimal form of the GLibInstanceCheck of
libpangocairo.st, that the library runs with the VM's GLib: on Linux and macOS glib_check_version (2,
88, 0) looked up through the library's module answers NULL and g_free (and g_object_unref) through the
module are those of the VM's libglib (libgobject), and Pango's HarfBuzz is at least 11.0.0; on Windows
GLib is at least 2.88 and the loaded libglib-2.0-0.dll, libgobject-2.0-0.dll and libcairo-2.dll (and
libharfbuzz-0.dll for Pango) lie in the VM directory.

1. Pango: pango_cairo_font_map_get_default, pango_font_map_create_context, pango_layout_new,
   pango_layout_set_text 'Ag', pango_layout_get_pixel_size (both > 0), the family of the font that
   'Sans 20' loads (pango_context_load_font, pango_font_describe,
   pango_font_description_get_family); then pango_cairo_create_layout on an Athens canvas (64@32)
   and pango_cairo_show_layout, whose pixels must have ink.
2. Poppler: on macOS first the bundled fontconfig file as LibPoppler does (unless FONTCONFIG_FILE
   is set: FcConfigCreate, FcConfigParseAndLoad of Contents/Resources/fontconfig/fonts.conf,
   FcConfigSetCurrent of the libfontconfig next to libpoppler-glib).  A PDF that cairo's PDF surface
   writes to a temporary file (cairo_show_text 'Pharo'), read back and opened from a GBytes
   (poppler_document_new_from_bytes): 1 page, its text contains 'Pharo', rendered on an Athens canvas
   with ink.  Then the hand-written base-14 PDF of scripts/cairo-stack/probe-text.c (the same 585
   bytes: a Helvetica that is not embedded, which Poppler substitutes through fontconfig): its text
   contains 'Base14', its rendering has ink.
Every object is unref'd or freed explicitly.  Prints exactly one line for each part

	PangoCairo ok pango <x.y.z> family <family> ink <n>
	Poppler ok poppler <x.y.z> pages 1 text yes ink <n> base14 text yes ink <n>

or 'PangoCairo FAILED <reason>' / 'Poppler FAILED <reason>' (a library that does not load, a symbol
missing, a failed instance check, no ink...), or 'PangoCairo not bundled' / 'Poppler not bundled'
when the VM directory has no libpangocairo (resp. libpoppler-glib), and exits 0: the CI jobs grep the
lines.  It exits 1 only on an internal error of this script."

| lineEnd say internal platform vmDir libDir isWindows isMac finder names libFile resolve inside quote
  probe libs compile pdfPart pangoPart cString utf8Z ink checkUnix checkWindows utf16Z utf16Decode
  dirOf normalize oneLine base14 fontconfig checkVersion |
lineEnd := OSPlatform current lineEnding.
say := [ :message | Stdio stdout nextPutAll: message; nextPutAll: lineEnd; flush ].
oneLine := [ :message | (message asString copyReplaceAll: String cr with: ' ') copyReplaceAll: String lf with: ' ' ].
internal := [ :message |
	say value: 'ci-text internal error: ' , (oneLine value: message).
	Smalltalk exitFailure ].

[
isWindows := OSPlatform current isWindows.
isMac := OSPlatform current isMacOS.
platform := isWindows ifTrue: [ #win ] ifFalse: [ isMac ifTrue: [ #mac ] ifFalse: [ #unix ] ].
vmDir := Smalltalk vm directory.
libDir := isMac ifTrue: [ (vmDir asFileReference / 'Plugins') fullName ] ifFalse: [ vmDir asFileReference fullName ].

"the names of each library (the classes of libpangocairo.st and libpoppler.st use the same)"
names := Dictionary new.
names
	at: #glib put: { #unix -> #( 'libglib-2.0.so.0' 'libglib-2.0.so' ). #mac -> #( 'libglib-2.0.0.dylib' 'libglib-2.0.dylib' ). #win -> #( 'libglib-2.0-0.dll' ) } asDictionary;
	at: #gobject put: { #unix -> #( 'libgobject-2.0.so.0' 'libgobject-2.0.so' ). #mac -> #( 'libgobject-2.0.0.dylib' 'libgobject-2.0.dylib' ). #win -> #( 'libgobject-2.0-0.dll' ) } asDictionary;
	at: #pango put: { #unix -> #( 'libpango-1.0.so.0' 'libpango-1.0.so' ). #mac -> #( 'libpango-1.0.0.dylib' 'libpango-1.0.dylib' ). #win -> #( 'libpango-1.0-0.dll' ) } asDictionary;
	at: #pangocairo put: { #unix -> #( 'libpangocairo-1.0.so.0' 'libpangocairo-1.0.so' ). #mac -> #( 'libpangocairo-1.0.0.dylib' 'libpangocairo-1.0.dylib' ). #win -> #( 'libpangocairo-1.0-0.dll' ) } asDictionary;
	at: #poppler put: { #unix -> #( 'libpoppler-glib.so.8' 'libpoppler-glib.so' ). #mac -> #( 'libpoppler-glib.8.dylib' 'libpoppler-glib.dylib' ). #win -> #( 'libpoppler-glib-8.dll' ) } asDictionary.
finder := isWindows
	ifTrue: [ FFIWindowsLibraryFinder new ]
	ifFalse: [ isMac ifTrue: [ FFIMacLibraryFinder new ] ifFalse: [ FFIUnix64LibraryFinder new ] ].

"a directory or file name to compare (Windows: no \\?\ prefix, any case)"
normalize := [ :path | | p |
	p := path asString.
	(p beginsWith: '\\?\') ifTrue: [ p := p allButFirst: 4 ].
	p := p asFileReference fullName.
	isWindows ifTrue: [ p := (p copyReplaceAll: '/' with: '\') asLowercase ].
	p ].
dirOf := [ :path | | p i |
	p := path asString.
	i := (p lastIndexOf: $/) max: (isWindows ifTrue: [ p lastIndexOf: $\ ] ifFalse: [ 0 ]).
	i = 0 ifTrue: [ '' ] ifFalse: [ p first: i - 1 ] ].
"the file of the library in the bundle, if any (the first name: the soname, install name or DLL)"
libFile := [ :key | libDir , (isWindows ifTrue: [ '\' ] ifFalse: [ '/' ]) , ((names at: key) at: platform) first ].
"the path the finder resolves; an error when it is not inside the bundle"
resolve := [ :key | | path |
	path := (finder respondsTo: #findAnyLibrary:)
		ifTrue: [ finder findAnyLibrary: ((names at: key) at: platform) ]
		ifFalse: [ | found |
			found := nil.
			((names at: key) at: platform) do: [ :each |
				found ifNil: [ found := [ finder findLibrary: each ] on: Error do: [ :e | e return: nil ] ] ].
			found ifNil: [ self error: 'cannot locate ' , ((names at: key) at: platform) first ] ].
	inside value: path value: ((names at: key) at: platform) first.
	path ].
inside := [ :path :name |
	(normalize value: (dirOf value: path)) = (normalize value: libDir)
		ifFalse: [ self error: name , ' resolves to ' , path asString , ', not inside the VM bundle (' , libDir , ')' ] ].

"strings to and from C"
quote := [ :aString | aString asString copyReplaceAll: '''' with: '''''' ].
utf8Z := [ :aString | aString utf8Encoded , #[ 0 ] ].
cString := [ :address |
	(address isNil or: [ address isNull ])
		ifTrue: [ nil ]
		ifFalse: [
			(address respondsTo: #utf8StringFromCString)
				ifTrue: [ address utf8StringFromCString ]
				ifFalse: [ address readStringUTF8 ] ] ].
utf16Z := [ :aString | | bytes |
	bytes := ByteArray new: aString size + 1 * 2.
	aString doWithIndex: [ :c :i | bytes at: i * 2 - 1 put: c asInteger \\ 256; at: i * 2 put: c asInteger // 256 ].
	bytes ].
utf16Decode := [ :bytes :count |
	String streamContents: [ :out | | i high |
		i := 1.
		high := nil.
		[ i <= count ] whileTrue: [ | unit |
			unit := (bytes at: 2 * i - 1) + ((bytes at: 2 * i) bitShift: 8).
			(unit between: 16rD800 and: 16rDBFF)
				ifTrue: [ high := unit ]
				ifFalse: [
					((unit between: 16rDC00 and: 16rDFFF) and: [ high notNil ])
						ifTrue: [ out nextPut: (Character value: 16r10000 + ((high - 16rD800) bitShift: 10) + (unit - 16rDC00)) ]
						ifFalse: [ out nextPut: (Character value: unit) ].
					high := nil ].
			i := i + 1 ] ] ].
"pixels with ink on an Athens surface"
ink := [ :surface | | form n |
	form := surface asForm.
	n := 0.
	0 to: form height - 1 do: [ :y |
		0 to: form width - 1 do: [ :x | (form pixelValueAt: x @ y) = 0 ifFalse: [ n := n + 1 ] ] ].
	n ].

"the probe class; compile: the callouts of { header. signature } into the module at aPath"
probe := (Object << #PCSCiText) package: 'PangoPoppler-CI'; install.
compile := [ :pairs :aPath |
	pairs do: [ :pair |
		probe class
			compile: pair first , '
	^ self ffiCall: #(' , pair second , ') module: ''' , (quote value: aPath) , ''''
			classified: 'ci-text' ] ].
libs := Dictionary new.
libs at: #cairo put: CairoLibrary uniqueInstance libraryName.
compile
	value: #(
		#( 'cairoCreate: surface' 'void * cairo_create (void * surface)' )
		#( 'cairoDestroy: cr' 'void cairo_destroy (void * cr)' )
		#( 'cairoSave: cr' 'void cairo_save (void * cr)' )
		#( 'cairoRestore: cr' 'void cairo_restore (void * cr)' )
		#( 'cairoMoveTo: cr x: x y: y' 'void cairo_move_to (void * cr, double x, double y)' )
		#( 'cairoSetSourceRGBA: cr r: r g: g b: b a: a' 'void cairo_set_source_rgba (void * cr, double r, double g, double b, double a)' )
		#( 'cairoSelectFontFace: cr family: family slant: slant weight: weight' 'void cairo_select_font_face (void * cr, void * family, int slant, int weight)' )
		#( 'cairoSetFontSize: cr size: size' 'void cairo_set_font_size (void * cr, double size)' )
		#( 'cairoShowText: cr text: text' 'void cairo_show_text (void * cr, void * text)' )
		#( 'cairoShowPage: cr' 'void cairo_show_page (void * cr)' )
		#( 'cairoPdfSurfaceCreate: file width: w height: h' 'void * cairo_pdf_surface_create (void * file, double w, double h)' )
		#( 'cairoSurfaceFinish: surface' 'void cairo_surface_finish (void * surface)' )
		#( 'cairoSurfaceStatus: surface' 'int cairo_surface_status (void * surface)' )
		#( 'cairoSurfaceDestroy: surface' 'void cairo_surface_destroy (void * surface)' ) )
	value: (libs at: #cairo).
isWindows ifTrue: [
	compile
		value: #(
			#( 'getModuleHandle: name' 'void * GetModuleHandleW (void * name)' )
			#( 'getModuleFileName: module buffer: buffer size: size' 'uint32 GetModuleFileNameW (void * module, void * buffer, uint32 size)' ) )
		value: 'kernel32.dll' ].
"glib_check_version through the module of aKey (a library resolved into libs)"
checkVersion := [ :key |
	compile
		value: { { 'checkGLibMajor: a minor: b micro: c'. 'void * glib_check_version (uint32 a, uint32 b, uint32 c)' } }
		value: (libs at: key).
	cString value: (probe checkGLibMajor: 2 minor: 88 micro: 0) ].

"the minimal instance checks: an error naming what is wrong"
checkUnix := [ :key :harfBuzz | | path message address |
	path := libs at: key.
	message := checkVersion value: key.
	message ifNotNil: [
		self error: ((names at: key) at: platform) first , ' binds a GLib older than 2.88 (' , message ,
			'): another libglib-2.0 was loaded first' ].
	#( #( 'g_free' #glib 'libglib-2.0' ) #( 'g_object_unref' #gobject 'libgobject-2.0' ) ) do: [ :each |
		address := [ ExternalAddress loadSymbol: each first module: path ]
			on: Error
			do: [ :e | self error: ((names at: key) at: platform) first , ' does not reach ' , each first ].
		address asInteger = (ExternalAddress loadSymbol: each first module: (libs at: each second)) asInteger
			ifFalse: [
				self error: ((names at: key) at: platform) first , ' binds another ' , each third , ' than the one of the VM (' ,
					(libs at: each second) , '): another one was loaded first' ] ].
	harfBuzz ifTrue: [
		compile
			value: #( #( 'harfBuzzAtLeastMajor: a minor: b micro: c' 'int hb_version_atleast (uint32 a, uint32 b, uint32 c)' ) )
			value: path.
		(probe harfBuzzAtLeastMajor: 11 minor: 0 micro: 0) = 0
			ifTrue: [ self error: ((names at: key) at: platform) first , ' binds a HarfBuzz older than 11.0.0' ] ] ].
checkWindows := [ :key :dlls | | message |
	"(loading the library loads its imports)"
	[ ExternalAddress loadSymbol: 'glib_check_version' module: (libs at: key) ] on: Error do: [ :e | e return: nil ].
	message := checkVersion value: #glib.
	message ifNotNil: [ self error: 'the GLib of the VM is older than 2.88 (' , message , ')' ].
	dlls do: [ :dll | | handle buffer count file |
		handle := probe getModuleHandle: (utf16Z value: dll).
		handle isNull ifTrue: [ self error: dll , ' is not loaded' ].
		buffer := ByteArray new: 2 * 4096.
		count := probe getModuleFileName: handle buffer: buffer size: 4096.
		file := utf16Decode value: buffer value: count.
		(normalize value: (dirOf value: file)) = (normalize value: libDir)
			ifFalse: [ self error: 'the loaded ' , dll , ' is ' , file , ', not the one in the VM directory (a stale DLL next to the image?)' ] ] ].

"1. Pango"
pangoPart := [
	(libFile value: #pangocairo) asFileReference exists
		ifFalse: [ 'PangoCairo not bundled' ]
		ifTrue: [
			[ | fontMap context layout pixel desc font described family version surface count |
				#( #glib #gobject #pango #pangocairo ) do: [ :key | libs at: key put: (resolve value: key) ].
				inside value: (libs at: #cairo) value: 'CairoLibrary'.
				isWindows
					ifTrue: [ checkWindows value: #pangocairo value: #( 'libglib-2.0-0.dll' 'libgobject-2.0-0.dll' 'libcairo-2.dll' 'libharfbuzz-0.dll' ) ]
					ifFalse: [
						checkUnix value: #pangocairo value: false.
						checkUnix value: #pango value: true ].
				compile
					value: #(
						#( 'gFree: p' 'void g_free (void * p)' ) )
					value: (libs at: #glib).
				compile
					value: #(
						#( 'gObjectUnref: o' 'void g_object_unref (void * o)' ) )
					value: (libs at: #gobject).
				compile
					value: #(
						#( 'pangoVersionString' 'void * pango_version_string ()' )
						#( 'pangoFontMapCreateContext: map' 'void * pango_font_map_create_context (void * map)' )
						#( 'pangoLayoutNew: context' 'void * pango_layout_new (void * context)' )
						#( 'pangoLayoutSetText: layout text: text length: length' 'void pango_layout_set_text (void * layout, void * text, int length)' )
						#( 'pangoLayoutSetFontDescription: layout description: desc' 'void pango_layout_set_font_description (void * layout, void * desc)' )
						#( 'pangoLayoutGetPixelSize: layout width: w height: h' 'void pango_layout_get_pixel_size (void * layout, void * w, void * h)' )
						#( 'pangoFontDescriptionFromString: text' 'void * pango_font_description_from_string (void * text)' )
						#( 'pangoFontDescriptionFree: desc' 'void pango_font_description_free (void * desc)' )
						#( 'pangoFontDescriptionGetFamily: desc' 'void * pango_font_description_get_family (void * desc)' )
						#( 'pangoContextLoadFont: context description: desc' 'void * pango_context_load_font (void * context, void * desc)' )
						#( 'pangoFontDescribe: font' 'void * pango_font_describe (void * font)' ) )
					value: (libs at: #pango).
				compile
					value: #(
						#( 'pangoCairoFontMapGetDefault' 'void * pango_cairo_font_map_get_default ()' )
						#( 'pangoCairoCreateLayout: cr' 'void * pango_cairo_create_layout (void * cr)' )
						#( 'pangoCairoShowLayout: cr layout: layout' 'void pango_cairo_show_layout (void * cr, void * layout)' ) )
					value: (libs at: #pangocairo).
				version := cString value: probe pangoVersionString.
				desc := probe pangoFontDescriptionFromString: (utf8Z value: 'Sans 20').
				fontMap := probe pangoCairoFontMapGetDefault.
				fontMap isNull ifTrue: [ self error: 'pango_cairo_font_map_get_default answered NULL' ].
				context := probe pangoFontMapCreateContext: fontMap.
				layout := probe pangoLayoutNew: context.
				probe pangoLayoutSetText: layout text: (utf8Z value: 'Ag') length: -1.
				probe pangoLayoutSetFontDescription: layout description: desc.
				pixel := { ByteArray new: 4. ByteArray new: 4 }.
				probe pangoLayoutGetPixelSize: layout width: pixel first height: pixel second.
				probe gObjectUnref: layout.
				font := probe pangoContextLoadFont: context description: desc.
				family := nil.
				font isNull ifFalse: [
					described := probe pangoFontDescribe: font.
					family := cString value: (probe pangoFontDescriptionGetFamily: described).
					probe pangoFontDescriptionFree: described.
					probe gObjectUnref: font ].
				probe gObjectUnref: context.
				((pixel first signedLongAt: 1) > 0 and: [ (pixel second signedLongAt: 1) > 0 ])
					ifFalse: [
						probe pangoFontDescriptionFree: desc.
						self error: 'pango_layout_get_pixel_size answered ' , (pixel first signedLongAt: 1) printString , 'x' ,
							(pixel second signedLongAt: 1) printString ].
				family ifNil: [
					probe pangoFontDescriptionFree: desc.
					self error: 'pango_context_load_font answered NULL for Sans 20' ].
				surface := AthensCairoSurface extent: 64 @ 32.
				surface drawDuring: [ :canvas | | cr drawn |
					cr := canvas getHandle.
					probe cairoSave: cr.
					drawn := probe pangoCairoCreateLayout: cr.
					probe pangoLayoutSetText: drawn text: (utf8Z value: 'Ag') length: -1.
					probe pangoLayoutSetFontDescription: drawn description: desc.
					probe cairoSetSourceRGBA: cr r: 0 g: 0 b: 0 a: 1.
					probe cairoMoveTo: cr x: 2 y: 0.
					probe pangoCairoShowLayout: cr layout: drawn.
					probe gObjectUnref: drawn.
					probe cairoRestore: cr ].
				probe pangoFontDescriptionFree: desc.
				count := ink value: surface.
				count > 0 ifFalse: [ self error: 'pango ' , version , ' family ' , family , ': no ink' ].
				'PangoCairo ok pango ' , version , ' family ' , family , ' ink ' , count printString ]
					on: Error
					do: [ :error | 'PangoCairo FAILED ' , (oneLine value: error description) ] ] ].
say value: pangoPart value.

"2. Poppler"
"the hand-written base-14 PDF: byte for byte the base14_pdf of scripts/cairo-stack/probe-text.c"
base14 := (String lf join: #(
	'%PDF-1.4'
	'1 0 obj'
	'<< /Type /Catalog /Pages 2 0 R >>'
	'endobj'
	'2 0 obj'
	'<< /Type /Pages /Kids [3 0 R] /Count 1 >>'
	'endobj'
	'3 0 obj'
	'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 50] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>'
	'endobj'
	'4 0 obj'
	'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'
	'endobj'
	'5 0 obj'
	'<< /Length 42 >>'
	'stream'
	'BT /F1 24 Tf 10 16 Td (Base14 Pharo) Tj ET'
	'endstream'
	'endobj'
	'xref'
	'0 6'
	'0000000000 65535 f '
	'0000000009 00000 n '
	'0000000058 00000 n '
	'0000000115 00000 n '
	'0000000240 00000 n '
	'0000000310 00000 n '
	'trailer'
	'<< /Size 6 /Root 1 0 R >>'
	'startxref'
	'402'
	'%%EOF' )) , String lf.
base14 := base14 asByteArray.
base14 size = 585 ifFalse: [ internal value: 'the base-14 PDF has ' , base14 size printString , ' bytes, not 585' ].
"macOS (LibPoppler's PopplerFontconfig): the bundled fonts.conf, through the libfontconfig of libpoppler-glib"
fontconfig := [ | dir conf config |
	(isMac and: [ (OSEnvironment current at: 'FONTCONFIG_FILE' ifAbsent: [ '' ]) isEmpty ]) ifTrue: [
		dir := dirOf value: (libs at: #poppler).
		conf := dir , '/../../Resources/fontconfig/fonts.conf'.
		conf asFileReference exists ifFalse: [ self error: 'no ' , conf ].
		compile
			value: #(
				#( 'fcConfigCreate' 'void * FcConfigCreate ()' )
				#( 'fcConfigParseAndLoad: config file: file complain: complain' 'int FcConfigParseAndLoad (void * config, void * file, int complain)' )
				#( 'fcConfigSetCurrent: config' 'int FcConfigSetCurrent (void * config)' )
				#( 'fcConfigDestroy: config' 'void FcConfigDestroy (void * config)' ) )
			value: dir , '/libfontconfig.1.dylib'.
		config := probe fcConfigCreate.
		(probe fcConfigParseAndLoad: config file: (utf8Z value: conf) complain: 1) = 0 ifTrue: [
			probe fcConfigDestroy: config.
			self error: 'FcConfigParseAndLoad failed on ' , conf ].
		(probe fcConfigSetCurrent: config) = 0 ifTrue: [
			probe fcConfigDestroy: config.
			self error: 'FcConfigSetCurrent failed' ].
		probe fcConfigDestroy: config ] ].
"aBlock value: the PopplerDocument opened from the bytes (unref'd after)"
pdfPart := [ :bytes :aBlock | | gbytes slot document |
	gbytes := probe gBytesNew: bytes size: bytes size.
	slot := ByteArray new: 8.
	document := probe popplerDocumentNewFromBytes: gbytes password: ExternalAddress null error: slot.
	probe gBytesUnref: gbytes.
	document isNull ifTrue: [ | error message |
		error := slot pointerAt: 1.
		message := error isNull
			ifTrue: [ 'no GError' ]
			ifFalse: [ | text |
				text := cString value: (error pointerAt: 9).
				probe gErrorFree: error.
				text ].
		self error: 'poppler_document_new_from_bytes: ' , message ].
	[ aBlock value: document ] ensure: [ probe gObjectUnref: document ] ].
say value: [
	(libFile value: #poppler) asFileReference exists
		ifFalse: [ 'Poppler not bundled' ]
		ifTrue: [
			[ | version file bytes pages text1 ink1 text2 ink2 page render |
				#( #glib #gobject #poppler ) do: [ :key | libs at: key put: (resolve value: key) ].
				inside value: (libs at: #cairo) value: 'CairoLibrary'.
				isWindows
					ifTrue: [ checkWindows value: #poppler value: #( 'libglib-2.0-0.dll' 'libgobject-2.0-0.dll' 'libcairo-2.dll' ) ]
					ifFalse: [ checkUnix value: #poppler value: false ].
				compile
					value: #(
						#( 'gFree: p' 'void g_free (void * p)' )
						#( 'gBytesNew: data size: size' 'void * g_bytes_new (void * data, size_t size)' )
						#( 'gBytesUnref: bytes' 'void g_bytes_unref (void * bytes)' )
						#( 'gErrorFree: error' 'void g_error_free (void * error)' ) )
					value: (libs at: #glib).
				compile
					value: #(
						#( 'gObjectUnref: o' 'void g_object_unref (void * o)' ) )
					value: (libs at: #gobject).
				compile
					value: #(
						#( 'popplerGetVersion' 'void * poppler_get_version ()' )
						#( 'popplerDocumentNewFromBytes: bytes password: password error: error' 'void * poppler_document_new_from_bytes (void * bytes, void * password, void * error)' )
						#( 'popplerDocumentGetNPages: document' 'int poppler_document_get_n_pages (void * document)' )
						#( 'popplerDocumentGetPage: document index: index' 'void * poppler_document_get_page (void * document, int index)' )
						#( 'popplerPageGetText: page' 'void * poppler_page_get_text (void * page)' )
						#( 'popplerPageRender: page cr: cr' 'void poppler_page_render (void * page, void * cr)' ) )
					value: (libs at: #poppler).
				fontconfig value.
				version := cString value: probe popplerGetVersion.
				"the text of page 1 (g_free) and the pixels with ink of its rendering on an Athens canvas"
				page := [ :document :extent | | p address text surface |
					p := probe popplerDocumentGetPage: document index: 0.
					p isNull ifTrue: [ self error: 'poppler_document_get_page 0 answered NULL' ].
					address := probe popplerPageGetText: p.
					text := cString value: address.
					address isNull ifFalse: [ probe gFree: address ].
					surface := AthensCairoSurface extent: extent.
					surface drawDuring: [ :canvas | | cr |
						cr := canvas getHandle.
						probe cairoSave: cr.
						probe popplerPageRender: p cr: cr.
						probe cairoRestore: cr ].
					probe gObjectUnref: p.
					{ text ifNil: [ '' ]. ink value: surface } ].
				"a PDF of cairo's PDF surface"
				file := FileLocator temp asFileReference / ('pcs-ci-text-' , Time microsecondClockValue printString , '.pdf').
				[ | surface cr status |
					surface := probe cairoPdfSurfaceCreate: (utf8Z value: file fullName) width: 200 height: 100.
					cr := probe cairoCreate: surface.
					probe cairoSetSourceRGBA: cr r: 0 g: 0 b: 0 a: 1.
					probe cairoSelectFontFace: cr family: (utf8Z value: 'sans-serif') slant: 0 weight: 0.
					probe cairoSetFontSize: cr size: 32.
					probe cairoMoveTo: cr x: 20 y: 60.
					probe cairoShowText: cr text: (utf8Z value: 'Pharo').
					probe cairoShowPage: cr.
					probe cairoDestroy: cr.
					probe cairoSurfaceFinish: surface.
					status := probe cairoSurfaceStatus: surface.
					probe cairoSurfaceDestroy: surface.
					status = 0 ifFalse: [ self error: 'cairo PDF surface status ' , status printString ].
					bytes := file binaryReadStreamDo: [ :in | in upToEnd ] ]
						ensure: [ file ensureDelete ].
				pdfPart value: bytes value: [ :document | | result |
					pages := probe popplerDocumentGetNPages: document.
					pages = 1 ifFalse: [ self error: 'poppler ' , version , ': ' , pages printString , ' pages, not 1' ].
					result := page value: document value: 200 @ 100.
					text1 := result first.
					ink1 := result second ].
				(text1 includesSubstring: 'Pharo')
					ifFalse: [ self error: 'poppler ' , version , ': the text of the page is ' , text1 printString ].
				ink1 > 0 ifFalse: [ self error: 'poppler ' , version , ': no ink' ].
				"the base-14 PDF"
				pdfPart value: base14 value: [ :document | | result |
					result := page value: document value: 200 @ 50.
					text2 := result first.
					ink2 := result second ].
				(text2 includesSubstring: 'Base14')
					ifFalse: [ self error: 'poppler ' , version , ': the text of the base-14 page is ' , text2 printString ].
				ink2 > 0 ifFalse: [ self error: 'poppler ' , version , ': no ink on the base-14 page' ].
				'Poppler ok poppler ' , version , ' pages ' , pages printString , ' text yes ink ' , ink1 printString ,
					' base14 text yes ink ' , ink2 printString ]
					on: Error
					do: [ :error | 'Poppler FAILED ' , (oneLine value: error description) ] ] ] value
] on: Error do: [ :error | internal value: error description ].

Smalltalk snapshot: false andQuit: true.
