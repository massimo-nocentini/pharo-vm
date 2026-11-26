"Template used by regen.sh: @SRC@ @PLUGIN@ @OUT@ are substituted.
 1. loads the Slang/VMMaker packages of @SRC@ (a smalltalksrc dir) with Monticello,
    replacing what the VMMaker image has (MC removes methods absent from @SRC@);
 2. translates each external plugin of @PLUGIN@ (a literal array of class names) the way PharoVMMaker does for
    CoInterpreter + Spur64BitCoMemoryManager, into @OUT@/plugins/src/<moduleName>/."
| out repo loader t vmmaker failed |
out := [:s | Stdio stdout nextPutAll: s; lf; flush].
failed := false.
t := Time millisecondClockValue.
repo := TonelRepository new directory: '@SRC@' asFileReference.
loader := MCVersionLoader new.
#('Printf' 'VMMakerLoadingDependencies' 'CAST' 'Slang' 'Melchor' 'VMMaker' 'FileAttributesPlugin') do: [:p |
	loader addVersion: (repo versionFromFileNamed: p)].
[[loader load]
	on: Warning
	do: [:w |
		"Undeclared-variable warnings are expected (test/tool packages are not reloaded)"
		(w messageText includesSubstring: 'Undeclared') ifFalse: [out value: 'REGEN-LOAD-WARNING: ', w messageText].
		w resume]]
	on: Error
	do: [:e | out value: 'REGEN-LOAD-ERROR: ', e class name, ': ', e messageText.
		[(e respondsTo: #methodClass) ifTrue: [out value: '  in class: ', e methodClass printString].
		 ((e respondsTo: #sourceCode) and: [e respondsTo: #location]) ifTrue: [ | src loc |
			src := e sourceCode. loc := e location.
			out value: '  source: ', (src copyFrom: 1 to: (src indexOf: Character cr ifAbsent: [src size min: 80]) - 1).
			out value: '  near: ', (src copyFrom: (loc - 80 max: 1) to: (loc - 1 min: src size)), ' <<HERE>> ',
				(src copyFrom: (loc min: src size + 1) to: (loc + 40 min: src size))]] on: Error do: [:ignored | ].
		failed := true].
failed ifTrue: [Smalltalk exitFailure].
out value: 'REGEN: loaded @SRC@ in ', (Time millisecondClockValue - t) printString, ' ms'.
@LENIENT@ ifTrue: [
	"Lenient mode (regen.sh default): a <var:type:> pragma naming a variable that is
	 not a local/argument of the method (BitBltSimulation>>alphaBlendConst:with:paintMode:
	 types the instance variable sourceAlpha) is a TranslationError in this Slang.
	 Log it and DROP that declaration (what the old Squeak Slang that produced the
	 checked-in BitBltPlugin.c effectively did: sourceAlpha stays a global sqInt).
	 Calls that pass the strict check behave exactly as before."
	TMethod compile: 'declarationAt: aVariableName put: aDeclaration
	((aVariableName beginsWithAnyOf: #(#self #cascade #std)) not
		and: [(self declaresVariableWithName: aVariableName) not]) ifTrue: [
		Stdio stdout nextPutAll: ''REGEN-LENIENT: '', definingClass name, ''>>'', selector, '' types non-local '', aVariableName, '' (declaration dropped)''; lf; flush.
		^ aDeclaration].
	^ declarations at: aVariableName put: aDeclaration'
		classified: 'regen-lenient'.
	"Slang's dead-code elimination can strip BOTH arms of a conditional and leave
	 'cond ifTrue:'/'cond ifFalse:' with no argument (BitBltSimulation>>copyLoopPixMap,
	 after inlining pickSourcePixels:...), which then crashes CAST generation
	 (SubscriptOutOfBounds in generateCASTIfFalse:). Emit an empty statement instead."
	#(#generateCASTIfFalse: #generateCASTIfTrue:) do: [:sel | | src i |
		src := (CCodeGenerator >> sel) sourceCode.
		i := src indexOf: Character cr.
		CCodeGenerator
			compile: (src first: i),
				'	tast arguments isEmpty ifTrue: [Stdio stdout nextPutAll: ''REGEN-LENIENT: argument-less '', tast selector, '' on '', tast receiver printString, '' emitted as an empty statement''; lf; flush. ^ CEmptyStatementNode new].', (String with: Character cr),
				(src allButFirst: i)
			classified: 'regen-lenient']].
(@PLUGIN@ reject: [:n | Smalltalk globals includesKey: n]) ifNotEmpty: [:missing |
	out value: 'REGEN-ERROR: no class named ', missing printString. Smalltalk exitFailure].
@PLUGIN@ do: [:pluginName |
t := Time millisecondClockValue.
[[VMMakerConfiguration initializeForPharo.
  vmmaker := VMMaker
	makerFor: CoInterpreter
	and: StackToRegisterMappingCogit
	with: { #ObjectMemory. #Spur64BitCoMemoryManager.
		#bytecodeTableInitializer. #initializeBytecodeTableForSistaV1.
		#ImageReader. #SpurImageReader. #ImageWriter. #SpurImageWriter }
	to: '@OUT@' asFileReference
	platformDir: '@OUT@' asFileReference
	including: #()
	configuration: VMMakerConfiguration.
  vmmaker stopOnErrors: true; internal: #() external: { pluginName }.
  vmmaker vmmakerConfiguration interpreterPluginClass initialize.
  vmmaker generateExternalPlugin: pluginName ] valueSupplyingAnswer: true]
	on: Error
	do: [:e | out value: 'REGEN-TRANSLATE-ERROR: ', pluginName, ': ', e class name, ': ', e messageText.
		(e signalerContext ifNil: [#()] ifNotNil: [:c | c stack first: (c stack size min: 12)])
			do: [:c | out value: '    ', (c printString truncateTo: 160)].
		(e signalerContext ifNil: [#()] ifNotNil: [:c | c stack]) do: [:c |
			((c receiver isKindOf: TMethod) or: [c receiver isKindOf: CCodeGenerator]) ifTrue: [
				(c receiver isKindOf: TMethod) ifTrue: [out value: '  while translating method: ', c receiver definingClass name, '>>', c receiver selector]]].
		failed := true].
failed ifFalse: [
	out value: 'REGEN: translated ', pluginName, ' (module ', (Smalltalk globals at: pluginName) moduleName, ') in ', (Time millisecondClockValue - t) printString, ' ms']].
failed ifTrue: [Smalltalk exitFailure].
