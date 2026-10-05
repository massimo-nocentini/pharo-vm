"Prepare the image of the browser world, from a stock Pharo 12 or Pharo 15 image.
 cmake/emscripten/webimage.cmake runs it at build time with a native Pharo VM, on a copy
 of the stock image:

	PHARO_WEB_ST_DIR=<st dir> PHARO_WEB_IMAGE_DIR=<out dir> PHARO_WEB_FONTS=<fonts> \
	pharo --headless <copy>.image --no-default-preferences st --save --quit prepare-web-image.st

 The settings come in the environment: the st command of Pharo 15 takes nothing but its
 file.  It loads the OSWindow-Web package (Tonel) from <st dir>, sets the image up for
 the web VM (OSWebDriver class>>#setUpImage), with its fonts: freetype keeps the
 FreeType fonts of the stock image, bitmap switches to the bitmap fonts, for a web VM
 built without FreeType.  It also lets the session pick OSWebDriver whenever the page
 provides a display.  <out dir> receives
 Pharo-web.image, Pharo-web.changes and OSWindow-Web.st, the package filed out for
 web-bootstrap.st.  The --save of st saves the image once more after this script, so
 the image starts in the browser with nothing of the preparation left to run."

| environment stDir outDir undeclared package |
environment := OSEnvironment current.
stDir := (environment
	at: 'PHARO_WEB_ST_DIR'
	ifAbsent: [ self error: 'prepare-web-image: PHARO_WEB_ST_DIR is not set' ]) asFileReference.
outDir := (environment
	at: 'PHARO_WEB_IMAGE_DIR'
	ifAbsent: [ self error: 'prepare-web-image: PHARO_WEB_IMAGE_DIR is not set' ]) asFileReference.
outDir ensureCreateDirectory.
"The fonts of the web VM, which the FreeType of this VM tells nothing about (see
 OSWebDriver class>>#setUpFonts)"
environment
	at: 'PHARO_WEB_FONTS'
	ifAbsent: [ self error: 'prepare-web-image: PHARO_WEB_FONTS is not set' ].

"Install the snapshot rather than load the version: a version is named with a UUID,
 which Pharo 15 makes only with the UUIDPlugin of a VM 10.3.9 or later"
(TonelReader on: stDir fileName: 'OSWindow-Web') snapshot install.
package := 'OSWindow-Web' asPackage.

"Loading only warns about an undeclared variable, and the world would break in the
 browser: fail before saving anything"
undeclared := package methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	Stdio stderr
		nextPutAll: 'prepare-web-image: undeclared variables in ';
		nextPutAll: (undeclared collect: [ :method | method printString ]) asArray printString;
		lf;
		flush.
	Smalltalk exitFailure ].

(Smalltalk at: #OSWebDriver) setUpImage.

"The stock image saved OSSDL2Driver.  Let every session choose again: the startUp:
 hook of OSWebDriver takes over in the page, and native VMs keep SDL.  Loading ran the
 class initialization that registers the hook; running it again does no harm"
OSWindowDriver classPool
	at: #DriverClass put: nil;
	at: #Current put: nil.
(Smalltalk at: #OSWebDriver) initialize.

"File the package out for web-bootstrap.st, in the chunk format that the code importers
 of both Pharo 12 and Pharo 15 read: every class first, since Pharo 15 compiles no method
 that refers to a class not defined yet; fluid class definitions, which both evaluate; a
 methodsFor: section for each method, since Pharo 15 reads one method of a section; then
 the initialization of the classes that define one on their class side.  The file outs of
 the images would not do: Package>>#fileOut of Pharo 12 writes a class after methods that
 refer to it, and Pharo 15 has a Fileouter instead, which initializes every class that
 defines #initialize on its instance side"
(outDir / 'OSWindow-Web.st') ensureDelete; writeStreamDo: [ :stream |
	| bang chunk classes methods |
	"A chunk ends with an exclamation mark, and one in it is doubled.  One before a chunk
	 makes it the header of the chunks that follow, a class comment or a methodsFor:
	 section, which an empty chunk ends.  (This script is in chunk format too, hence
	 Character value: 33)"
	bang := Character value: 33.
	chunk := [ :string |
		stream
			nextPutAll: (string copyReplaceAll: (String with: bang) with: (String with: bang with: bang));
			nextPut: bang;
			nextPut: Character cr ].
	classes := package definedClasses asArray sort: [ :a :b |
		a allSuperclasses size < b allSuperclasses size or: [
			a allSuperclasses size = b allSuperclasses size and: [ a name <= b name ] ] ].
	classes do: [ :class |
		(class hasTraitComposition or: [
			 class classSide instVarNames notEmpty or: [ class packageTag isRoot not ] ]) ifTrue: [
			self error: 'prepare-web-image: cannot file out ' , class name
				, ', which has traits, class-side variables or a tag' ].
		chunk value: '(' , class superclass name , ' << #' , class name
			, ' slots: ' , class instVarNames asArray printString
			, '; sharedVariables: ' , class classVarNames asArray sorted printString
			, '; package: ' , package name asString printString , ') install' ].
	classes do: [ :class |
		class hasComment ifTrue: [
			stream nextPut: bang.
			chunk
				value: class name , ' commentStamp: '''' prior: 0';
				value: class comment ] ].
	methods := package methods asArray sort: [ :a :b |
		a methodClass instanceSide name < b methodClass instanceSide name or: [
			a methodClass instanceSide == b methodClass instanceSide and: [
				a methodClass == b methodClass
					ifTrue: [ a selector <= b selector ]
					ifFalse: [ a methodClass isMeta not ] ] ] ].
	methods do: [ :method |
		stream nextPut: bang.
		chunk
			value: method methodClass name , ' methodsFor: ' , method protocolName asString printString;
			value: method sourceCode;
			value: ' ' ].
	classes do: [ :class |
		(class classSide includesSelector: #initialize) ifTrue: [
			chunk value: class name , ' initialize' ] ] ].

Smalltalk saveAs: (outDir / 'Pharo-web') fullName.
Stdio stdout
	nextPutAll: 'prepare-web-image: saved ';
	nextPutAll: Smalltalk imagePath;
	lf;
	flush.
