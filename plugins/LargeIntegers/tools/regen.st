"regen.st - run by sync-c.sh on a throw-away copy of the VMMaker image.
 Loads the Slang packages of the smalltalksrc directory named by LISYNC_SRC
 with Monticello (replacing the image's code: Monticello also removes the
 methods the directory no longer has) and translates LargeIntegersPlugin as
 PharoVMMaker does for the 64-bit Spur CoInterpreter, into LISYNC_OUT/plugins/src/LargeIntegers/.
 Prints LISYNC-ERROR: lines and exits with a failure status on any error."
| env out srcDir outDir repo loader failed vmmaker |
out := [:s | Stdio stdout nextPutAll: s; lf; flush].
env := Smalltalk os environment.
srcDir := env at: 'LISYNC_SRC' ifAbsent: [nil].
outDir := env at: 'LISYNC_OUT' ifAbsent: [nil].
(srcDir isNil or: [outDir isNil]) ifTrue: [
	out value: 'LISYNC-ERROR: LISYNC_SRC and LISYNC_OUT must be set'.
	Smalltalk exitFailure].
failed := false.
repo := TonelRepository new directory: srcDir asFileReference.
loader := MCVersionLoader new.
#('Printf' 'VMMakerLoadingDependencies' 'CAST' 'Slang' 'Melchor' 'VMMaker' 'FileAttributesPlugin') do: [:p |
	loader addVersion: (repo versionFromFileNamed: p)].
[[loader load]
	on: Warning
	do: [:w |
		"undeclared-variable warnings are expected: the test and tool packages are not loaded"
		(w messageText includesSubstring: 'Undeclared') ifFalse: [out value: 'LISYNC-WARNING: ', w messageText].
		w resume]]
	on: Error
	do: [:e |
		out value: 'LISYNC-ERROR: loading ', srcDir, ': ', e class name, ': ', e messageText.
		[((e respondsTo: #sourceCode) and: [e respondsTo: #location]) ifTrue: [ | src loc |
			src := e sourceCode.
			loc := e location.
			out value: '  near: ', (src copyFrom: (loc - 80 max: 1) to: (loc - 1 min: src size)), ' <<HERE>> ',
				(src copyFrom: (loc min: src size + 1) to: (loc + 40 min: src size))]] on: Error do: [:ignored | ].
		failed := true].
failed ifTrue: [Smalltalk exitFailure].
[[VMMakerConfiguration initializeForPharo.
  vmmaker := VMMaker
	makerFor: CoInterpreter
	and: StackToRegisterMappingCogit
	with: { #ObjectMemory. #Spur64BitCoMemoryManager.
		#bytecodeTableInitializer. #initializeBytecodeTableForSistaV1.
		#ImageReader. #SpurImageReader. #ImageWriter. #SpurImageWriter }
	to: outDir asFileReference
	platformDir: outDir asFileReference
	including: #()
	configuration: VMMakerConfiguration.
  vmmaker stopOnErrors: true; internal: #() external: #(LargeIntegersPlugin).
  vmmaker vmmakerConfiguration interpreterPluginClass initialize.
  vmmaker generateExternalPlugin: #LargeIntegersPlugin] valueSupplyingAnswer: true]
	on: Error
	do: [:e |
		out value: 'LISYNC-ERROR: translating LargeIntegersPlugin: ', e class name, ': ', e messageText.
		(e signalerContext ifNil: [#()] ifNotNil: [:c | c stack])
			detect: [:c | c receiver isKindOf: TMethod]
			ifFound: [:c | out value: '  while translating ', c receiver definingClass name, '>>', c receiver selector]
			ifNone: [].
		failed := true].
failed ifTrue: [Smalltalk exitFailure].
out value: 'LISYNC: translated LargeIntegersPlugin'.
