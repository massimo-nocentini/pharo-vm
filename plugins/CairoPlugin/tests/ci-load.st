"Load the CairoPlugin of the VM, as the CI jobs do with the VMMaker image (Pharo 13.1, which has
Athens-Cairo and draws headless), which has no VM-Plugins-Primitives-Cairo package: this file
compiles its own primitive methods (cpCi...) into probe subclasses of the Athens classes
(CPCiCanvas, CPCiShapeCanvas, CPCiMatrix, CPCiBuilder, CPCiBasicRenderer, CPCiTextRenderer),
whose overrides of the Athens methods call the plugin's primitives and, unlike the package's
patches, never fall back to the FFI: a primitive that fails stops the probe.

1. Draws once with stock Athens (the first time a CI job calls cairo), else 'CairoPlugin FAILED
   cairo not loadable: <error>'.
   Then, independent of the plugin, toy text through the image's FFI (class CPCiToy): on an
   ARGB32 surface, cairo_select_font_face 'sans-serif' and cairo_show_text 'Ag' at 20 px; the
   status of the context must be SUCCESS and some pixels inked.  This exercises the text
   backend the bundle's cairo picks for toy fonts (fontconfig and FreeType on Linux, Quartz on
   macOS, DWrite on Windows, cairo's built-in twin without fonts).  Prints

	CairoToy ok family <family> type <font type> ink <pixels>

   (the family FreeType loaded, e.g. 'DejaVu Sans', for the type ft; 'twin' for the type user;
   else the requested 'sans-serif'), or 'CairoToy FAILED ...', and goes on either way: the CI
   jobs grep the line (on the web VMs the type is 'unknown' if the registry lacks
   cairo_scaled_font_get_type).
2. primitiveInfo: nil without the plugin ('CairoPlugin did not load'); else protocol 1,
   'CairoPlugin 1.0', unbound.
3. Computes the layout table of docs/CairoPlugin.md by name (the 29 slot indices and constants,
   then the classes ByteString ByteSymbol WideString WideSymbol), and binds the plugin to the
   libcairo of the image's FFI (CairoLibrary uniqueInstance libraryName) with the probe
   cairo_create of that library; checks primitiveInfo bound (the version, 'loader' natively and
   'static' on the web VMs, no missing function) and, natively, that one libcairo is loaded.
4. Draws a fixed 64x64 scene (filled and stroked rectangles, a dashed one, a Bezier path filled
   and stroked, a transformed rectangle, a ByteString and a WideString of text) three times on
   fresh surfaces: by stock Athens (ffi), with the one-for-one primitives (basic: the canvas, its
   path matrix, the path builder and the font renderer adopted by the probe classes), and with
   the shape and text primitives too (all).  The SHA-256 of the pixels must be the same, and every
   primitive of each pass must have been called (primitiveCounters).

Prints

	CairoPlugin <platform> <protocol> cairo <x.y.z> ok

(platform native or emscripten) and exits 0; else prints 'CairoPlugin FAILED ...' or
'CairoPlugin did not load' (which the jobs' grep of 'CairoPlugin native' does not match) and
exits 1.  On the web VMs (lane 74) it runs on the prepared image of the world (image/web), whose
OSWindow-Web lets a surface be made without the SurfacePlugin."

| lineEnd fail say probeClasses category header override overrides info platform protocol idx
  layout missingIvar lib probe bound version versionString names primNames counters scene font
  wide draw passes shaFfi used cairoModules toy |
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stdout
		nextPutAll: 'CairoPlugin FAILED ';
		nextPutAll: ((message copyReplaceAll: String cr with: ' ') copyReplaceAll: String lf with: ' ');
		nextPutAll: lineEnd;
		flush.
	Smalltalk exitFailure ].
say := [ :message | Stdio stdout nextPutAll: message; nextPutAll: lineEnd; flush ].

"1. cairo through stock Athens"
[ (AthensCairoSurface extent: 4 @ 4) drawDuring: [ :canvas |
	canvas setPaint: Color red; drawShape: (0 @ 0 extent: 2 @ 2) ] ]
		on: Error
		do: [ :error | fail value: 'cairo not loadable: ' , error description ].

"1b. toy text through the image's FFI, independent of the plugin"
toy := (Object << #CPCiToy) package: 'CairoPlugin-CI'; install.
toy class compile: 'ffiLibrary
	^ CairoLibrary' classified: 'cairo-plugin-ci'.
#( 'toyImageSurfaceCreate: format width: w height: h
	^ self ffiCall: #(void * cairo_image_surface_create (int format, int w, int h))'
   'toyCreate: surface
	^ self ffiCall: #(void * cairo_create (void * surface))'
   'toySelectFontFace: cr family: family slant: slant weight: weight
	^ self ffiCall: #(void cairo_select_font_face (void * cr, char * family, int slant, int weight))'
   'toySetFontSize: cr size: size
	^ self ffiCall: #(void cairo_set_font_size (void * cr, double size))'
   'toyMoveTo: cr x: x y: y
	^ self ffiCall: #(void cairo_move_to (void * cr, double x, double y))'
   'toySetSourceRGBA: cr r: r g: g b: b a: a
	^ self ffiCall: #(void cairo_set_source_rgba (void * cr, double r, double g, double b, double a))'
   'toyShowText: cr text: text
	^ self ffiCall: #(void cairo_show_text (void * cr, char * text))'
   'toyStatus: cr
	^ self ffiCall: #(int cairo_status (void * cr))'
   'toyGetScaledFont: cr
	^ self ffiCall: #(void * cairo_get_scaled_font (void * cr))'
   'toyScaledFontStatus: sf
	^ self ffiCall: #(int cairo_scaled_font_status (void * sf))'
   'toyScaledFontGetType: sf
	^ self ffiCall: #(int cairo_scaled_font_get_type (void * sf))'
   'toyLockFace: sf
	^ self ffiCall: #(void * cairo_ft_scaled_font_lock_face (void * sf))'
   'toyUnlockFace: sf
	^ self ffiCall: #(void cairo_ft_scaled_font_unlock_face (void * sf))'
   'toySurfaceFlush: surface
	^ self ffiCall: #(void cairo_surface_flush (void * surface))'
   'toySurfaceStatus: surface
	^ self ffiCall: #(int cairo_surface_status (void * surface))'
   'toyGetData: surface
	^ self ffiCall: #(void * cairo_image_surface_get_data (void * surface))'
   'toyGetStride: surface
	^ self ffiCall: #(int cairo_image_surface_get_stride (void * surface))'
   'toyDestroy: cr
	^ self ffiCall: #(void cairo_destroy (void * cr))'
   'toySurfaceDestroy: surface
	^ self ffiCall: #(void cairo_surface_destroy (void * surface))'
   'toyStatusString: st
	^ self ffiCall: #(char * cairo_status_to_string (int st))' ) do: [ :source |
	toy class compile: source classified: 'cairo-plugin-ci' ].
[ | lib has surface cr status sf type typeName family face familyPtr stride bytes ink |
	lib := CairoLibrary uniqueInstance libraryName.
	"(the Emscripten VMs' FFI registry has only the functions the images bind:
	cmake/emscripten/ffi/symbols/cairo.txt)"
	has := [ :name | [ (ExternalAddress loadSymbol: name module: lib) notNil ] on: Error do: [ :e | false ] ].
	surface := toy toyImageSurfaceCreate: 0 "ARGB32" width: 64 height: 32.
	cr := toy toyCreate: surface.
	toy toySelectFontFace: cr family: 'sans-serif' slant: 0 weight: 0.
	toy toySetFontSize: cr size: 20.
	toy toyMoveTo: cr x: 4 y: 24.
	toy toySetSourceRGBA: cr r: 0 g: 0 b: 0 a: 1.
	toy toyShowText: cr text: 'Ag'.
	toy toySurfaceFlush: surface.
	sf := toy toyGetScaledFont: cr.
	"(cairo_get_scaled_font of a context in error answers an object with its status)"
	status := (has value: 'cairo_status')
		ifTrue: [ toy toyStatus: cr ]
		ifFalse: [ toy toyScaledFontStatus: sf ].
	status = 0 ifFalse: [ status := status max: (toy toySurfaceStatus: surface) ].
	family := 'sans-serif'.
	type := (has value: 'cairo_scaled_font_get_type') ifTrue: [ toy toyScaledFontGetType: sf ] ifFalse: [ nil ].
	(status = 0 and: [ (type isNil or: [ type = 1 ]) and: [ has value: 'cairo_ft_scaled_font_lock_face' ] ]) ifTrue: [
		"FreeType: the family of the face fontconfig chose (FT_FaceRec's family_name,
		after five FT_Long)"
		face := toy toyLockFace: sf.
		face isNull ifFalse: [
			type := 1.
			familyPtr := face pointerAt: ((5 * (FFIExternalType sizeOf: #long)) alignedTo: Smalltalk wordSize) + 1.
			familyPtr isNull ifFalse: [
				family := (familyPtr respondsTo: #utf8StringFromCString)
					ifTrue: [ familyPtr utf8StringFromCString ]
					ifFalse: [ familyPtr readStringUTF8 ] ].
			toy toyUnlockFace: sf ] ].
	typeName := type
		ifNil: [ 'unknown' ]
		ifNotNil: [ #( 'toy' 'ft' 'win32' 'quartz' 'user' 'dwrite' ) at: type + 1 ifAbsent: [ type printString ] ].
	type = 4 ifTrue: [ family := 'twin' ].	"the user font of a toy face: cairo's built-in twin"
	stride := toy toyGetStride: surface.
	bytes := ByteArray new: stride * 32.
	status = 0 ifTrue: [ LibC memCopy: (toy toyGetData: surface) to: bytes size: bytes size ].
	ink := 0.
	1 to: bytes size by: 4 do: [ :i |
		((bytes at: i) + (bytes at: i + 1) + (bytes at: i + 2) + (bytes at: i + 3)) > 0 ifTrue: [ ink := ink + 1 ] ].
	toy toyDestroy: cr.
	toy toySurfaceDestroy: surface.
	(status = 0 and: [ ink > 0 ])
		ifTrue: [ say value: 'CairoToy ok family ' , family , ' type ' , typeName , ' ink ' , ink printString ]
		ifFalse: [
			say value: 'CairoToy FAILED status ' , status printString , ' ' , (toy toyStatusString: status) ,
				', family ' , family , ' type ' , typeName , ' ink ' , ink printString ] ]
	on: Error
	do: [ :error | say value: 'CairoToy FAILED ' , error description ].

"2. the plugin"
category := 'cairo-plugin-ci'.
probeClasses := Dictionary new.
probeClasses at: #CPCiAdmin put: ((Object << #CPCiAdmin) package: 'CairoPlugin-CI'; install).
{ #CPCiCanvas -> AthensCairoCanvas. #CPCiMatrix -> AthensCairoMatrix.
  #CPCiBuilder -> AthensCairoPathBuilder. #CPCiBasicRenderer -> CairoFreetypeFontRenderer } do: [ :each |
	probeClasses at: each key put: ((each value << each key) package: 'CairoPlugin-CI'; install) ].
probeClasses at: #CPCiShapeCanvas put: (((probeClasses at: #CPCiCanvas) << #CPCiShapeCanvas) package: 'CairoPlugin-CI'; install).
probeClasses at: #CPCiTextRenderer put: (((probeClasses at: #CPCiBasicRenderer) << #CPCiTextRenderer) package: 'CairoPlugin-CI'; install).
(probeClasses at: #CPCiAdmin) class
	compile: 'cpCiInfo <primitive: ''primitiveInfo'' module: ''CairoPlugin''> ^ nil' classified: category;
	compile: 'cpCiBind: path probe: anAddress layout: anArray <primitive: ''primitiveBind'' module: ''CairoPlugin'' error: ec> ^ ec' classified: category;
	compile: 'cpCiFunctionNames <primitive: ''primitiveFunctionNames'' module: ''CairoPlugin''> ^ nil' classified: category;
	compile: 'cpCiCounters <primitive: ''primitiveCounters'' module: ''CairoPlugin''> ^ nil' classified: category;
	compile: 'cpCiResetCounters <primitive: ''primitiveResetCounters'' module: ''CairoPlugin''> ^ nil' classified: category.
"(a failed primitive of the scene stops it: the patches of the package would answer through the FFI)"
probeClasses do: [ :each |
	each
		compile: 'cpCiFailed: aName code: aCode
	^ self error: ''cpCi '' , aName , '' failed: '' , aCode printString'
		classified: category ].

info := (probeClasses at: #CPCiAdmin) cpCiInfo.
info ifNil: [ say value: 'CairoPlugin did not load'. Smalltalk exitFailure ].
(info isArray and: [ info size = 7 and: [ info first = 1 and: [ (info at: 2) = 'CairoPlugin 1.0' ] ] ])
	ifFalse: [ fail value: 'primitiveInfo answers ' , info printString ].
platform := info at: 3.
protocol := info first.
(#( 'native' 'emscripten' ) includes: platform) ifFalse: [ fail value: 'primitiveInfo answers ' , info printString ].

"3. the layout table, by name, and the bind"
missingIvar := nil.
idx := [ :class :name |
	(class allInstVarNames includes: name)
		ifTrue: [ (class allInstVarNames indexOf: name) - 1 ]
		ifFalse: [ missingIvar ifNil: [ missingIvar := class name , '>>' , name ]. 0 ] ].
layout := {
	1.
	idx value: AthensCairoCanvas value: #handle.
	idx value: AthensCairoCanvas value: #pathTransform.
	idx value: AthensCairoCanvas value: #paint.
	idx value: AthensCairoCanvas value: #surface.
	idx value: AthensCairoSurface value: #handle.
	idx value: AthensCairoPathBuilder value: #context.
	idx value: ExternalObject value: #handle.
	idx value: AthensCairoPaintMode value: #canvas.
	idx value: AthensCairoStrokePaint value: #fillPaint.
	idx value: AthensCairoStrokePaint value: #width.
	idx value: AthensCairoStrokePaint value: #capStyle.
	idx value: AthensCairoStrokePaint value: #joinStyle.
	idx value: AthensCairoStrokePaint value: #dashLenghts.
	idx value: AthensCairoStrokePaint value: #dashOffset.
	idx value: CairoFreetypeFontRenderer value: #font.
	idx value: CairoFreetypeFontRenderer value: #canvas.
	idx value: CairoFreetypeFontRenderer value: #glyphExtents.
	idx value: Color value: #rgb.
	idx value: Color value: #alpha.
	idx value: Rectangle value: #origin.
	idx value: Rectangle value: #corner.
	idx value: Point value: #x.
	idx value: Point value: #y.
	Color classPool at: #RedShift ifAbsent: [ missingIvar := 'Color RedShift'. 0 ].
	Color classPool at: #GreenShift ifAbsent: [ missingIvar := 'Color GreenShift'. 0 ].
	Color classPool at: #ComponentMask ifAbsent: [ missingIvar := 'Color ComponentMask'. 0 ].
	255.
	idx value: AthensCairoSurface value: #currentCanvas.
	ByteString. ByteSymbol. WideString. WideSymbol }.
missingIvar ifNotNil: [ fail value: 'layout: no ' , missingIvar ].
(Color classPool at: #ComponentMax ifAbsent: [ nil ]) = (Color classPool at: #ComponentMask) asFloat
	ifFalse: [ fail value: 'layout: ComponentMax is not ComponentMask asFloat' ].
lib := CairoLibrary uniqueInstance libraryName.
probe := ExternalAddress loadSymbol: 'cairo_create' module: lib.
bound := (probeClasses at: #CPCiAdmin) cpCiBind: lib probe: probe layout: layout.
bound isInteger ifFalse: [ fail value: 'primitiveBind of ' , lib printString , ' failed: ' , bound printString ].
version := bound.
version >= 11600 ifFalse: [ fail value: 'cairo ' , version printString , ' is older than 1.16.0' ].
versionString := (version // 10000) printString , '.' , (version // 100 \\ 100) printString , '.' , (version \\ 100) printString.
info := (probeClasses at: #CPCiAdmin) cpCiInfo.
((info at: 4) = version
	and: [ (info at: 5) = (platform = 'native' ifTrue: [ 'loader' ] ifFalse: [ 'static' ])
	and: [ (info at: 6) isNil and: [ (info at: 7) isEmpty ] ] ])
		ifFalse: [ fail value: 'primitiveInfo answers ' , info printString , ' once bound' ].
platform = 'native' ifTrue: [
	"the plugin loads the very library of the FFI, through the same module cache"
	cairoModules := Smalltalk vm listLoadedModules select: [ :each | | file |
		"(the name of the file, e.g. libcairo.so.2, libcairo.2.dylib or libcairo-2.dll)"
		file := (each substrings: '/\') last asLowercase.
		(file includesSubstring: 'cairo') and: [ (file includesSubstring: 'cairoplugin') not ] ].
	cairoModules size = 1 ifFalse: [ fail value: 'loaded cairo modules: ' , cairoModules asArray printString ] ].

"4. the scene, by stock Athens and through the primitives"
names := (probeClasses at: #CPCiAdmin) cpCiFunctionNames.
primNames := names second.
header := [ :selector |
	selector numArgs = 0
		ifTrue: [ selector asString ]
		ifFalse: [
			String streamContents: [ :stream |
				selector keywords withIndexDo: [ :keyword :index |
					stream nextPutAll: keyword; nextPutAll: ' a'; nextPutAll: index printString; nextPutAll: ' ' ] ] ] ].
override := [ :class :selector :primitive |
	(primNames includes: primitive) ifFalse: [ fail value: 'the plugin has no ' , primitive ].
	class
		compile: (header value: selector) , '
	<primitive: ''' , primitive , ''' module: ''CairoPlugin'' error: ec>
	^ self cpCiFailed: ''' , primitive , ''' code: ec'
		classified: category ].
"{ probe class. Athens selector. primitive. pass that must call it }"
overrides := #(
	#( #CPCiCanvas #newPath #primitiveNewPath #basic )
	#( #CPCiCanvas #fill #primitiveFill #basic )
	#( #CPCiCanvas #stroke #primitiveStroke #basic )
	#( #CPCiCanvas #setPathMatrix #primitiveSetPathMatrix #basic )
	#( #CPCiCanvas #primSetSourceR:g:b:a: #primitiveSetSourceRGBA #basic )
	#( #CPCiCanvas #primResetDashes: #primitiveResetDash #basic )
	#( #CPCiCanvas #primSetDashesLengths:count:offset: #primitiveSetDash #basic )
	#( #CPCiCanvas #primRectangleX:y:width:height: #primitiveRectangle #basic )
	#( #CPCiCanvas #primSetLineWidth: #primitiveSetLineWidth #basic )
	#( #CPCiCanvas #primSetLineJoin: #primitiveSetLineJoin #basic )
	#( #CPCiCanvas #primSetLineCap: #primitiveSetLineCap #basic )
	#( #CPCiCanvas #loadPath: #primitiveAppendPath #basic )
	#( #CPCiCanvas #setScaledFont: #primitiveSetScaledFont #basic )
	#( #CPCiCanvas #primSetFontSize: #primitiveSetFontSize #basic )
	#( #CPCiCanvas #showGlyphs:size: #primitiveShowGlyphs #basic )
	#( #CPCiCanvas #cpCiGetScaledFontInto: #primitiveGetScaledFont #basic )
	#( #CPCiMatrix #primLoadIdentity #primitiveMatrixInitIdentity #both )
	#( #CPCiMatrix #initx:y:sx:sy:shx:shy: #primitiveMatrixInit #both )
	#( #CPCiMatrix #primTranslateX:Y: #primitiveMatrixTranslate #both )
	#( #CPCiMatrix #primScaleX:Y: #primitiveMatrixScale #both )
	#( #CPCiMatrix #primRotateByRadians: #primitiveMatrixRotate #both )
	#( #CPCiBuilder #newPath #primitiveBuilderNewPath #both )
	#( #CPCiBuilder #moveToX:Y: #primitiveBuilderMoveTo #both )
	#( #CPCiBuilder #lineToX:Y: #primitiveBuilderLineTo #both )
	#( #CPCiBuilder #primCurveViaX:Y:viaX:Y:toX:Y: #primitiveBuilderCurveTo #both )
	#( #CPCiBuilder #closePath #primitiveBuilderClosePath #both )
	#( #CPCiBuilder #cpCiCopyPathInto: #primitiveBuilderCopyPath #both )
	#( #CPCiBasicRenderer #convertString:len:ofFont:toGlyphs:numGlyphs:x:y: #primitiveTextToGlyphs #basic )
	#( #CPCiBasicRenderer #freeGlyphs: #primitiveGlyphFree #basic )
	#( #CPCiShapeCanvas #cpCiFillRectangle:color: #primitiveFillRectangleColor #all )
	#( #CPCiShapeCanvas #cpCiStrokeRectangle:paint:colorClass: #primitiveStrokeRectangle #all )
	#( #CPCiShapeCanvas #cpCiFillPath:color: #primitiveFillPathColor #all )
	#( #CPCiShapeCanvas #cpCiStrokePath:paint:colorClass: #primitiveStrokePath #all )
	#( #CPCiTextRenderer #cpCiShowString:from:to:pixelSize:colorClass: #primitiveShowString #all ) ).
overrides do: [ :row | override value: (probeClasses at: row first) value: row second value: row third ].
"the templates of the package's mirrors (docs/CairoPlugin.md), without their fallbacks"
(probeClasses at: #CPCiCanvas)
	compile: 'getScaledFont
	^ self cpCiGetScaledFontInto: CairoScaledFont basicNew'
	classified: category.
(probeClasses at: #CPCiBuilder)
	compile: 'copyPath
	^ self cpCiCopyPathInto: AthensCairoPath basicNew'
	classified: category.
(probeClasses at: #CPCiShapeCanvas)
	compile: 'draw
	| s p |
	s := shape.
	p := paint.
	s class == Rectangle ifTrue: [
		p class == Color ifTrue: [ ^ self cpCiFillRectangle: s color: p ].
		p class == AthensCairoStrokePaint ifTrue: [ ^ self cpCiStrokeRectangle: s paint: p colorClass: Color ] ].
	s class == AthensCairoPath ifTrue: [
		p class == Color ifTrue: [ ^ self cpCiFillPath: s color: p ].
		p class == AthensCairoStrokePaint ifTrue: [ ^ self cpCiStrokePath: s paint: p colorClass: Color ] ].
	^ self cpCiFailed: ''draw of a '' , s class name , '' with a '' , p class name code: nil'
	classified: category.
(probeClasses at: #CPCiTextRenderer)
	compile: 'renderCharacters: aString from: start to: end
	| ok |
	(originalFont face hasKerning not and: [ canvas paint class == Color ])
		ifFalse: [ ^ self cpCiFailed: ''renderCharacters:from:to: of a kerning font or a paint not a Color'' code: nil ].
	ok := self cpCiShowString: aString from: start to: end pixelSize: originalFont pixelSize colorClass: Color.
	ok == true ifTrue: [ ^ advance := glyphExtents width @ glyphExtents height ].
	ok == false ifTrue: [ ^ advance ].
	^ self cpCiFailed: ''primitiveShowString answering '' , ok printString code: nil'
	classified: category.

font := LogicalFont familyName: 'Source Sans Pro' pointSize: 12.
wide := WideString new: 3.
wide at: 1 put: $x; at: 2 put: (Character value: 16r3BB); at: 3 put: (Character value: 16rE9).
scene := [ :canvas | | stroke path |
	canvas setPaint: Color white; drawShape: (0 @ 0 extent: 64 @ 64).
	canvas setPaint: (Color r: 0.9 g: 0.2 b: 0.1 alpha: 0.8); drawShape: (4 @ 4 corner: 30 @ 26).
	stroke := canvas setStrokePaint: Color blue.
	stroke width: 3.
	canvas drawShape: (34.5 @ 4.5 extent: 25 @ 20).
	stroke := canvas setStrokePaint: Color green darker.
	stroke width: 2; dashes: #( 4 2 ) offset: 1.
	canvas drawShape: (4 @ 30 extent: 26 @ 12).
	path := canvas createPath: [ :builder |
		builder
			absolute;
			moveTo: 4 @ 60;
			curveVia: 14 @ 34 and: 30 @ 70 to: 40 @ 46;
			lineTo: 30 @ 60;
			close ].
	canvas setPaint: (Color orange alpha: 0.7); drawShape: path.
	(canvas setStrokePaint: Color black) width: 1.5.
	canvas drawShape: path.
	canvas pathTransform restoreAfter: [
		canvas pathTransform translateBy: 48 @ 48; rotateByDegrees: 30; scaleBy: 1.5.
		canvas setPaint: Color magenta; drawShape: (-6 @ -6 extent: 12 @ 12) ].
	canvas setFont: font; setPaint: Color black.
	canvas pathTransform restoreAfter: [
		canvas pathTransform translateBy: 34 @ 40.
		canvas drawString: 'Caf' , (String with: (Character value: 16rE9)) ].
	canvas pathTransform restoreAfter: [
		canvas pathTransform translateBy: 34 @ 58.
		canvas drawString: wide ] ].
"draw: the SHA-256 of the pixels of the scene on a fresh surface, the probe classes adopting
its objects first (pass basic and all)"
draw := [ :pass | | surface canvas bytes size |
	surface := AthensCairoSurface extent: 64 @ 64.
	pass = #ffi ifFalse: [
		canvas := surface instVarNamed: #context.
		(probeClasses at: (pass = #all ifTrue: [ #CPCiShapeCanvas ] ifFalse: [ #CPCiCanvas ])) adoptInstance: canvas.
		(probeClasses at: #CPCiMatrix) adoptInstance: canvas pathTransform.
		(probeClasses at: #CPCiBuilder) adoptInstance: (surface instVarNamed: #builder).
		(probeClasses at: (pass = #all ifTrue: [ #CPCiTextRenderer ] ifFalse: [ #CPCiBasicRenderer ]))
			adoptInstance: (surface instVarNamed: #ftFontRenderer) ].
	(probeClasses at: #CPCiAdmin) cpCiResetCounters.
	[ surface drawDuring: scene ]
		on: Error
		do: [ :error | fail value: pass , ': ' , error description ].
	counters := (probeClasses at: #CPCiAdmin) cpCiCounters.
	size := surface stride * surface privateHeight.
	bytes := ByteArray new: size.
	LibC memCopy: surface getDataPtr to: bytes size: size.
	(SHA256 hashMessage: bytes) hex ].
shaFfi := draw value: #ffi.
#( #basic #all ) do: [ :pass | | sha |
	sha := draw value: pass.
	used := 0.
	overrides do: [ :row |
		(row fourth = pass or: [ row fourth = #both ]) ifTrue: [
			(counters second at: (primNames indexOf: row third)) > 0
				ifTrue: [ used := used + 1 ]
				ifFalse: [ fail value: pass , ': ' , row third , ' was not called' ] ] ].
	sha = shaFfi ifFalse: [ fail value: pass , ': the pixels differ from those of the FFI (' , (sha first: 16) , ' and ' , (shaFfi first: 16) , ')' ].
	say value: 'CairoPlugin scene ' , pass , ' ' , (sha first: 16) , ', ' , used printString , ' primitives, ' ,
		(counters first inject: 0 into: [ :sum :each | sum + each ]) printString , ' cairo calls' ].

Stdio stdout
	nextPutAll: 'CairoPlugin '; nextPutAll: platform;
	nextPutAll: ' '; nextPutAll: protocol printString;
	nextPutAll: ' cairo '; nextPutAll: versionString;
	nextPutAll: ' ok'; nextPutAll: lineEnd; flush.
Smalltalk snapshot: false andQuit: true.
