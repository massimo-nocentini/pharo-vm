"Prepare the Pharo 12 image of the browser world.  cmake/emscripten/webimage.cmake
 runs it at build time with a native Pharo VM, on a copy of the stock image:

	pharo --headless <copy>.image --no-default-preferences --save --quit prepare-web-image.st <st dir> <out dir>

 It loads the OSWindow-Web package (Tonel) from <st dir>, switches to the bitmap fonts,
 because the web VM has no FFI and therefore no FreeType, and lets the session pick
 OSWebDriver whenever the page provides a display.  <out dir> receives Pharo12-web.image,
 Pharo12-web.changes and OSWindow-Web.st, the package filed out for web-bootstrap.st.
 The --save of the command line saves the image once more after this script, so the
 image starts in the browser with nothing of the preparation left to run."

| stDir outDir undeclared |
stDir := CommandLineArguments new commandLineArguments nextToLast asFileReference.
outDir := CommandLineArguments new commandLineArguments last asFileReference.
outDir ensureCreateDirectory.

(TonelReader on: stDir fileName: 'OSWindow-Web') version load.

"Loading only warns about an undeclared variable, and the world would break in the
 browser: fail before saving anything"
undeclared := 'OSWindow-Web' asPackage methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	Stdio stderr
		nextPutAll: 'prepare-web-image: undeclared variables in ';
		nextPutAll: (undeclared collect: [ :method | method printString ]) asArray printString;
		lf;
		flush.
	Smalltalk exitFailure ].

FreeTypeSystemSettings loadFt2Library: false.
StandardFonts setSmallBitmapFonts.

"The stock image saved OSSDL2Driver.  Let every session choose again: the startUp:
 hook of OSWebDriver takes over in the page, and native VMs keep SDL.  Loading ran the
 class initialization that registers the hook; running it again does no harm"
OSWindowDriver classPool
	at: #DriverClass put: nil;
	at: #Current put: nil.
(Smalltalk at: #OSWebDriver) initialize.

Smalltalk saveAs: (outDir / 'Pharo12-web') fullName.
"Package>>#fileOut writes next to the image, which is now in outDir.  It names every
 class it writes in a SystemNotification, which a headless image prints: keep the
 build log quiet"
[ 'OSWindow-Web' asPackage fileOut ]
	on: SystemNotification
	do: [ :notification | notification resume ].

Stdio stdout
	nextPutAll: 'prepare-web-image: saved ';
	nextPutAll: Smalltalk imagePath;
	lf;
	flush.
