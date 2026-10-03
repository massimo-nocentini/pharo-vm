"Prepare a stock Pharo 12 or Pharo 15 image, uploaded to the page, for the browser
 world, as prepare-web-image.st does at build time.  The page stages this script next to
 OSWindow-Web.st, the package filed out by prepare-web-image.st, in /pharo/st.

 A stock image cannot open the world page yet: its world starts through
 OSSDL2Driver, which needs the FFI.  So run this script in the Console page, for
 instance with

	CodeImporter evaluateFileNamed: '/pharo/st/web-bootstrap.st'

 then Save: the world page boots the saved image through OSWebDriver.  In a
 world that runs on another driver (an image that has the package but was saved
 elsewhere), it also restarts the world renderer on OSWebDriver.

 It files in st/OSWindow-Web.st of the image directory, unless the package is
 already there: 'st OSWindow-Web.st web-bootstrap.st' files it in first.  It does
 not save the image."

| package driver renderer |
(Smalltalk hasClassNamed: #OSWebDriver) ifFalse: [
	package := FileLocator imageDirectory / 'st' / 'OSWindow-Web.st'.
	package exists ifFalse: [ self error: 'web-bootstrap.st: no ' , package fullName ].
	"(the code importer of Pharo 15 fails on an empty file, with a MessageNotUnderstood)"
	package size > 0 ifTrue: [ CodeImporter evaluateFileNamed: package fullName ].
	(Smalltalk hasClassNamed: #OSWebDriver) ifFalse: [
		self error: 'web-bootstrap.st: ' , package fullName , ' defines no OSWebDriver' ] ].
driver := Smalltalk at: #OSWebDriver.

FreeTypeSystemSettings loadFt2Library: false.
StandardFonts setSmallBitmapFonts.

"The startUp: hook of OSWebDriver, registered by the package, picks it in the next
 sessions.  In this one, restart a world that runs on another driver; the restart
 shuts that driver down, so leave it in place.  But stop its event loop first:
 OSSDL2Driver>>#shutDown: forgets the windows before it stops that loop, which may
 meanwhile dispatch an event of the window that the restart destroys"
renderer := World worldState worldRenderer.
(driver isSuitable and: [
	 (renderer isKindOf: OSWorldRenderer) and: [ (renderer driver isKindOf: driver) not ] ])
	ifTrue: [
		renderer driver ifNotNil: [ :old | old shutDown: false ].
		OSWindowDriver classPool at: #DriverClass put: driver.
		OSWorldRenderer restart ]
