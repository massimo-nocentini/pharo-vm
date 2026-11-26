"
I am a buffer of a space of Bloc in a Morphic host (`BlMorphicWindowHost`, `BlMorphicHost`) on a VM without the manual surfaces of SurfacePlugin: the WebAssembly VM, whose world has no SDL2.

Bloc's own buffer, `BlExternalForm`, is a manual surface over memory outside the image, where Alexandrie's cairo draws and from which BitBlt copies through SurfacePlugin. I am a plain 32-bit Form instead, whose bits are pinned in memory: cairo draws into them, through the `void *` of `cairo_image_surface_create_for_data` (my `pointer` is my bits), and BitBlt copies from them as from any Form. The pixels are those of a `BlExternalForm`: cairo's ARGB32, native-endian, premultiplied, which BitBlt takes as depth 32.

`BlAbstractMorphicHostSpace>>#newExternalForm` (as this package has it) makes me when `isNeeded`, and Bloc's form otherwise: so a native VM, whose SurfacePlugin is there, goes on as before.
"
Class {
	#name : 'PharoBlocMorphicForm',
	#superclass : 'Form',
	#classVars : [
		'Needed',
		'NeededSession'
	],
	#category : 'PharoBloc-Web',
	#package : 'PharoBloc-Web'
}

{ #category : 'testing' }
PharoBlocMorphicForm class >> isNeeded [
	"Whether the VM lacks the manual surfaces of SurfacePlugin, asked once in each session"

	NeededSession == Smalltalk session ifFalse: [
		Needed := self lacksManualSurfaces.
		NeededSession := Smalltalk session ].
	^ Needed
]

{ #category : 'testing' }
PharoBlocMorphicForm class >> lacksManualSurfaces [
	"Whether the VM fails to make a manual surface of SurfacePlugin (one of a pixel, destroyed at once)"

	| form surface |
	form := BlExternalForm basicNew.
	surface := [
	           form
		           primCreateManualSurfaceWidth: 1
		           height: 1
		           rowPitch: 4
		           depth: 32
		           isMSB: true ]
		           on: PrimitiveFailed
		           do: [ :error | error return: nil ].
	surface ifNil: [ ^ true ].
	form primDestroyManualSurface: surface.
	^ false
]

{ #category : 'instance creation' }
PharoBlocMorphicForm class >> newExtent: aPoint [
	"A transparent form of aPoint, its bits pinned"

	^ (self extent: aPoint depth: 32)
		  fillColor: Color transparent;
		  yourself
]

{ #category : 'accessing' }
PharoBlocMorphicForm >> allocateSpace [
	"My bits are my space"
]

{ #category : 'accessing' }
PharoBlocMorphicForm >> autoRelease [
	"My bits go with me"
]

{ #category : 'accessing' }
PharoBlocMorphicForm >> destroySurface [
	"I have no surface"
]

{ #category : 'testing' }
PharoBlocMorphicForm >> isNull [
	^ bits isNil
]

{ #category : 'accessing' }
PharoBlocMorphicForm >> pointer [
	"What cairo draws into: my bits, which do not move"

	^ bits
]

{ #category : 'private' }
PharoBlocMorphicForm >> setManualSurfacePointer: anExternalAddress [
	"As a BlExternalForm, a null address leaves me null, as a new session leaves Bloc's form (for which a space makes new buffers: BlMorphicHostTest asks for that); my bits are the only pixels I take"

	anExternalAddress isNull ifFalse: [ self error: 'my pixels are my bits' ].
	bits := nil
]

{ #category : 'private' }
PharoBlocMorphicForm >> setExtent: extent depth: bitsPerPixel [
	bitsPerPixel == 32 ifFalse: [ self error: 'only 32 bits, as BlExternalForm' ].
	super setExtent: extent depth: bitsPerPixel.
	bits pinInMemory
]

{ #category : 'accessing' }
PharoBlocMorphicForm >> stride [
	"The bytes of a row: cairo's stride of ARGB32, which needs no padding"

	^ width * 4
]
