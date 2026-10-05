"The functions an image binds through the FFI, for the registry of the Emscripten VM
 (cmake/emscripten/ffiRegistry.cmake): one line for each function a method binds,

	<library> TAB <function> TAB <class>>><selector>

 sorted and without duplicates, where <library> is the name the registry knows, the
 one src/externalPrimitives.c reduces the image's library name to (libc.so.6 is c,
 libSDL2-2.0.so.0 is SDL2).  Run it on a stock image, which it does not save:

	pharo --headless <image> --no-default-preferences st ffi-symbols.st

 natively (WASM_HOST_PHARO) on Pharo 12 and Pharo 15, whose union the committed lists
 cmake/emscripten/ffi/symbols/<library>.txt hold, or with the node VM, where lane 59
 (tests/wasm/lanes/59-ffi-clean.sh) checks that the registry has every function of a
 registered library.  The lines go to the file that PHARO_FFI_SYMBOLS names in the
 environment, or else to stdout; a summary goes to stderr, and the image quits.

 Every method of every class and metaclass is read, the test packages included (a
 trait's methods are read in the classes that use it).  The function specs are the
 literals of the first argument of an ffiCall* or nbCall* message, both branches of a
 conditional spec (LibC>>pipe:mode: binds popen, or _popen on Windows) included, and
 they are parsed with FFIFunctionParser, as the callout parses them: a prefix list, or
 the raw literals of the methods, miss names (giterr_last, git_libgit2_init).  The
 symbols of ExternalAddress class>>loadSymbol:from: and loadSymbol:module: (environ)
 are listed too.

 The library is the library:, module: or from: argument of the message, or else the
 ffiLibrary of its receiver, self or self class; where that fails (SDL2 class>>
 ffiLibraryName looks for the file of SDL2, which the node VM does not find), the one
 library class of the map below that the receiver's ffiLibraryName and ffiLibrary
 methods name.  A library class gives its registry name from the map, which keeps the
 library finders out: they look for the files of the host.  Any other library gives its
 Unix file name, reduced, so DynamicLoader (libdl.so) is c too, as an alias of c is:
 those of cmake/emscripten/ffiLibraries.cmake (LibC is one: UnixPlatform>>
 getErrorMessage: binds strerror from the module #LibC).  A library that cannot be told
 without running code (an instance variable, as in TFUFFITestCase, or a library without
 a Unix name, as ObjCLibrary) is '?'."

| registryNames cAliases reduce registryNameOf receiverOf staticNameOf libraryOf functionOf specsOf isCallout isLoadSymbol rows unknown failures text output |
registryNames := Dictionary new.
registryNames
	at: #LibC put: 'c';
	at: #FT2FFILibrary put: 'freetype';
	at: #CairoLibrary put: 'cairo';
	at: #LGitLibrary put: 'git2';
	at: #SDL2 put: 'SDL2';
	at: #SDL2Library put: 'SDL2'.
cAliases := #('m' 'dl' 'pthread' 'rt' 'LibC').

"reduceLibraryName() of src/externalPrimitives.c: the base name, without a leading
 'lib', nor anything from its first '.' on, nor a version after a '-' (the first '-'
 followed by a digit); then the aliases of c"
reduce := [ :fileName | | base dash |
	base := fileName asString copyAfterLast: $/.
	base ifEmpty: [ base := fileName asString ].
	((base beginsWith: 'lib') and: [ base size > 3 and: [ (base at: 4) ~= $. ] ])
		ifTrue: [ base := base allButFirst: 3 ].
	base := base copyUpTo: $..
	dash := (1 to: base size - 1)
		detect: [ :index | (base at: index) = $- and: [ (base at: index + 1) isDigit ] ]
		ifNone: [ 0 ].
	dash > 0 ifTrue: [ base := base first: dash - 1 ].
	(cAliases includes: base) ifTrue: [ 'c' ] ifFalse: [ base ] ].

"A library as ffiLibrary answers it, or as a library: argument gives it: a file name, a
 library class or its unique instance (asFFILibrary makes the one from the other), or
 an FFIUnknownLibrary, which carries a file name"
registryNameOf := [ :library | | class ffiLibrary |
	library isString
		ifTrue: [ reduce value: library ]
		ifFalse: [
			class := library isClass ifTrue: [ library ] ifFalse: [ library class ].
			(library notNil and: [ registryNames includesKey: class name ])
				ifTrue: [ registryNames at: class name ]
				ifFalse: [
					ffiLibrary := [ library asFFILibrary ] on: Error do: [ nil ].
					ffiLibrary
						ifNil: [ '?' ]
						ifNotNil: [
							[ reduce value: ((ffiLibrary isKindOf: (Smalltalk at: #FFIUnknownLibrary))
									ifTrue: [ ffiLibrary libraryName ]
									ifFalse: [ ffiLibrary unix64LibraryName ]) ]
								on: Error
								do: [ '?' ] ] ] ] ].

"What self is in a method: the class in a class-side method, the unique instance of a
 library, or else an instance that no initialize ran in"
receiverOf := [ :method | | class |
	class := method methodClass.
	class isMeta
		ifTrue: [ class instanceSide ]
		ifFalse: [
			(class includesBehavior: (Smalltalk at: #FFILibrary))
				ifTrue: [ [ class uniqueInstance ] on: Error do: [ nil ] ]
				ifFalse: [ [ class basicNew ] on: Error do: [ nil ] ] ] ].

"The library of a receiver whose ffiLibrary fails, as SDL2's does where SDL2 is not found
 (SDL2 class>>ffiLibraryName looks for the file): the receiver is a class of the map, or
 its ffiLibraryName and ffiLibrary methods, on its side or its class's, name one class of
 the map (SDL2Handle class>>ffiLibraryName answers SDL2 ffiLibraryName)"
staticNameOf := [ :receiver | | classes names |
	(receiver isClass and: [ registryNames includesKey: receiver name ])
		ifTrue: [ registryNames at: receiver name ]
		ifFalse: [
			classes := receiver isClass
				ifTrue: [ { receiver class } ]
				ifFalse: [ { receiver class. receiver class class } ].
			names := Set new.
			classes do: [ :class |
				#(#ffiLibraryName #ffiLibrary) do: [ :selector |
					(class lookupSelector: selector) ifNotNil: [ :found |
						[ found ast allChildren do: [ :child |
							(child isVariable and: [ child isGlobalVariable
								and: [ registryNames includesKey: child name asSymbol ] ])
								ifTrue: [ names add: (registryNames at: child name asSymbol) ] ] ]
							on: Error
							do: [ ] ] ] ].
			names size = 1 ifTrue: [ names anyOne ] ifFalse: [ nil ] ] ].

libraryOf := [ :method :node | | index argument receiver |
	index := node selector keywords findFirst: [ :keyword |
		#('library:' 'module:' 'from:') includes: keyword ].
	index > 0
		ifTrue: [
			argument := node arguments at: index.
			argument isLiteralNode
				ifTrue: [ registryNameOf value: argument value ]
				ifFalse: [
					(argument isVariable and: [ argument isGlobalVariable ])
						ifTrue: [ registryNameOf value: (Smalltalk globals at: argument name ifAbsent: [ nil ]) ]
						ifFalse: [ '?' ] ] ]
		ifFalse: [
			receiver := (node receiver isSelfVariable or: [ node receiver isSuperVariable ])
				ifTrue: [ receiverOf value: method ]
				ifFalse: [
					(node receiver isMessage
						and: [ node receiver selector = #class
						and: [ node receiver receiver isSelfVariable ] ])
						ifTrue: [ (receiverOf value: method) ifNotNil: [ :object | object class ] ]
						ifFalse: [ nil ] ].
			receiver
				ifNil: [ '?' ]
				ifNotNil: [
					| name |
					name := registryNameOf value: ([ receiver ffiLibrary ] on: Error do: [ nil ]).
					name = '?'
						ifTrue: [ (staticNameOf value: receiver) ifNil: [ '?' ] ]
						ifFalse: [ name ] ] ] ].

"The function specs of a first argument: the literal, or the literals in its expression
 (not those inside a literal array)"
specsOf := [ :argument |
	argument isLiteralNode
		ifTrue: [ { argument value } ]
		ifFalse: [
			(argument allChildren select: [ :child |
				child isLiteralNode
					and: [ child parent isLiteralNode not
					and: [ child value isArray or: [ child value isString ] ] ] ])
				collect: [ :child | child value ] ] ].
"The function of a spec; '' for the spec of a function pointer, which has no name (the
 callouts of the callbacks of LGit), and nil for a spec that does not parse"
functionOf := [ :spec |
	[ (FFIFunctionParser new parseNamedFunction: spec) functionName asString ]
		on: Error
		do: [
			[ FFIFunctionParser new parseAnonymousFunction: spec. '' ]
				on: Error
				do: [ nil ] ] ].

isCallout := [ :selector | (selector beginsWith: 'ffiCall') or: [ selector beginsWith: 'nbCall' ] ].
isLoadSymbol := [ :selector | selector = #loadSymbol:from: or: [ selector = #loadSymbol:module: ] ].

rows := Set new.
unknown := 0.
failures := OrderedCollection new.
SystemNavigation default allBehaviorsDo: [ :behavior |
	behavior isTrait ifFalse: [
		behavior methodsDo: [ :method |
			"A callout that ran has been compiled again, and its method sends no ffiCall:
			 (LGitLibrary>>libgit2_init, at the startup): the original says what it sends"
			((method propertyAt: #ffiNonCompiledMethod ifAbsent: [ method ]) messages anySatisfy: [ :selector |
				(isCallout value: selector) or: [ isLoadSymbol value: selector ] ])
				ifTrue: [
					[ method ast allChildren do: [ :node |
						(node isMessage and: [ node arguments notEmpty ]) ifTrue: [
							| names library |
							names := OrderedCollection new.
							((isCallout value: node selector)
								and: [ "the translators pass their spec on, in a variable"
									node arguments first isVariable not ])
								ifTrue: [
									(specsOf value: node arguments first) do: [ :spec |
										(functionOf value: spec)
											ifNil: [ failures add: method printString , ': ' , spec printString ]
											ifNotNil: [ :name | name ifNotEmpty: [ names add: name ] ] ] ].
							((isLoadSymbol value: node selector)
								and: [ node arguments first isLiteralNode
								and: [ node arguments first value isString ] ])
								ifTrue: [ names add: node arguments first value asString ].
							names ifNotEmpty: [
								library := libraryOf value: method value: node.
								library = '?' ifTrue: [ unknown := unknown + names size ].
								names do: [ :name |
									rows add: library , String tab , name , String tab
										, method methodClass name , '>>' , method selector ] ] ] ] ]
						on: Error
						do: [ :error | failures add: method printString , ': ' , error messageText asString ] ] ] ] ].

text := String streamContents: [ :stream |
	rows asSortedCollection do: [ :row | stream nextPutAll: row; nextPut: Character lf ] ].
output := OSEnvironment current at: 'PHARO_FFI_SYMBOLS' ifAbsent: [ '' ].
output isEmpty
	ifTrue: [ Stdio stdout nextPutAll: text; flush ]
	ifFalse: [
		output asFileReference
			ensureDelete;
			writeStreamDo: [ :stream | stream nextPutAll: text ] ].
Stdio stderr
	nextPutAll: 'ffi-symbols: ';
	print: rows size;
	nextPutAll: ' rows, ';
	print: (rows collect: [ :row | (row substrings: String tab) first: 2 ]) size;
	nextPutAll: ' functions of ';
	print: (rows collect: [ :row | (row substrings: String tab) first ]) size;
	nextPutAll: ' libraries, ';
	print: unknown;
	nextPutAll: ' of an unknown library, ';
	print: failures size;
	nextPutAll: ' specs not parsed';
	lf.
failures do: [ :failure |
	Stdio stderr nextPutAll: 'ffi-symbols: no function in '; nextPutAll: failure; lf ].
Stdio stderr flush.
Smalltalk snapshot: false andQuit: true
