"
This plugin is a fake. It doesn't do anything useful. It's sole purpose is to wrap the C code that's associated with the SurfacePlugin into the main VM generation process. Since the C code isn't easily generated from ST code this is (unfortunately) necessary. But look on the bright side - you don't have to define any weird stuff for the C compiler. Isn't that great?! (just kidding...) 
"
Class {
	#name : 'SurfacePlugin',
	#superclass : 'InterpreterPlugin',
	#instVars : [
		'maxSurfaces',
		'surfaceArray',
		'numSurfaces'
	],
	#category : 'VMMaker-Plugins-Surface',
	#package : 'VMMaker',
	#tag : 'Plugins-Surface'
}

{ #category : 'translation' }
SurfacePlugin class >> declareCVarsIn: aCCodeGenerator [

	aCCodeGenerator addHeaderFile: '"SurfacePlugin.h"'.
	aCCodeGenerator var:#surfaceArray declareC: 'static SqueakSurface *surfaceArray = NULL'.
	aCCodeGenerator var:#numSurfaces declareC: 'static int numSurfaces = 0'.
	aCCodeGenerator var:#maxSurfaces declareC: 'static int maxSurfaces = 0'.	
]

{ #category : 'translation' }
SurfacePlugin class >> hasHeaderFile [
	"If there is a single intrinsic header file to be associated with the plugin, here is where you want to flag"
	^true
]

{ #category : 'translation' }
SurfacePlugin class >> requiresCrossPlatformFiles [
	"If there cross platform files to be associated with the plugin, here is where you want to flag"
	^true
]

{ #category : 'primitives' }
SurfacePlugin >> destroyManualSurface [
	"Defined in C code"

	<doNotGenerate>
	self notYetImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> dispatchOf: aSurface [
	<cmacro: '(aSurface) aSurface->dispatch'>
	^ self shouldBeImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> dispatchOfSurfaceIndex: i [

	<cmacro: '(i) surfaceArray[i].dispatch'>

	^ self shouldBeImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> getSurfaceFormatOf: aSurface [
	<cmacro: '(aSurface) aSurface->dispatch->getSurfaceFormat'>
	^ self shouldBeImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> handleOf: aSurface [
	<cmacro: '(aSurface) aSurface->handle'>
	^ self shouldBeImplemented
]

{ #category : 'initialize' }
SurfacePlugin >> initialiseModule [
	<export: true>
	
	surfaceArray := nil.
	numSurfaces := 0.
	maxSurfaces := 0.
	
	^ 1

]

{ #category : 'exported functions' }
SurfacePlugin >> ioFindSurface: surfaceID _: fn _: surfaceHandle [

	"Find the surface with the given ID, and, optionally,
	the given set of surface functions. The registered handle
	is returned in surfaceHandle. Return true if successful
	(e.g., the surface has been found), false otherwise.
	Valid IDs are 0 to maxSurfaces - 1. Before the first registration
	maxSurfaces is 0 and surfaceArray is NULL, so every ID is rejected."

	<export: true>
	<var: #surfaceID type: 'int'>
	<var: #fn type: 'sqSurfaceDispatch *'>
	<var: #surfaceHandle type: 'sqIntptr_t *'>
	<var: #surface type: 'SqueakSurface *'>

	| surface |
	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ ^ 0 ].
	
	(fn ~= 0 and: [fn ~= (self dispatchOf: surface)])
		ifTrue: [ ^ 0 ].
	
	surfaceHandle ifNil: [ ^ 0 ].
	
	self 
		cCode: '*surfaceHandle = surface->handle'
		inSmalltalk: [ self pointerAtPointer: surfaceHandle put: (self handleOf: surface) ].
	
	^ 1
]

{ #category : 'exported functions' }
SurfacePlugin >> ioGetSurfaceFormat:surfaceID _:width _:height _:depth _: isMSB [
	<export: true>
	<var: #surfaceID type: 'int'>
	<var: #width type: 'int*'>
	<var: #height type: 'int*'>
	<var: #depth type: 'int*'>
	<var: #isMSB type: 'int*'>
	<var: #surface type: 'SqueakSurface *'>
	<var: #fnGetSurfaceFormat type: 'fn_getSurfaceFormat'>

	"Return information describing the given surface.
	Return true if successful, false otherwise."

	| surface fnGetSurfaceFormat |
	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ self primitiveFail. ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ self primitiveFail. ^ 0 ].
	
	"Allow getSurfaceFormat to be defaulted to 0"
	
	(fnGetSurfaceFormat := self getSurfaceFormatOf: surface)
		ifNil: [ ^ -1 ].

	^ self perform: fnGetSurfaceFormat with: (self handleOf: surface) with: width with: height with: depth with: isMSB.


]

{ #category : 'exported functions' }
SurfacePlugin >> ioLockSurface: surfaceID _: pitch _: x _: y _: w _: h [

	"Lock the bits of the surface. 
	Return a pointer to the actual surface bits,
	or NULL on failure. "
	<export: true>
	<var: #surfaceID type: #'int'>
	<var: #pitch type: #'int*'>
	<var: #x type: #'int'>
	<var: #y type: #'int'>
	<var: #w type: #'int'>
	<var: #h type: #'int'>

	<var: #surface type: 'SqueakSurface *'>
	<var: #fnLockSurface type: 'fn_lockSurface'>

	| surface fnLockSurface |

	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ self primitiveFail. ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ self primitiveFail. ^ 0 ].
	
	(fnLockSurface := self lockSurfaceOf: surface)
		ifNil: [ self primitiveFail. ^ 0 ].

	^ self perform: fnLockSurface with: (self handleOf: surface) with: pitch with: x with: y with: w with: h.



]

{ #category : 'exported functions' }
SurfacePlugin >> ioRegisterSurface: surfaceHandle _: fn _: surfaceIDPtr [
	"Register a new surface with the given handle and
	the set of surface functions. The new ID is returned
	in surfaceID. Returns true if successful, false 
	otherwise."
	
	<var: #surfaceHandle type: #'sqIntptr_t'>
	<var: #fn type: #'sqSurfaceDispatch *'>
	<var: #surfaceIDPtr type: #'int*'>
	<var: #index type: #'int'>
	<var: #newMaxSurfaces type: #'int'>
	<var: #newSurfaceArray type: 'SqueakSurface *'>
	
	<export: true>
	
	| index newMaxSurfaces newSurfaceArray |
	
	fn = 0 ifTrue: [ ^ 0 ].
	surfaceIDPtr = 0 ifTrue: [ ^ 0 ].
	"Validating the sqSurfaceDispatch is the correct version"
	self cCode: 'if(fn->majorVersion != 1 && fn->minorVersion != 0) return 0'.

	index := -1.

	numSurfaces = maxSurfaces 
		ifTrue: [ 
			"Grow the array only once realloc succeeded, so a failure keeps the old one"
			newMaxSurfaces := maxSurfaces * 2 + 10.
			newSurfaceArray := self realloc: surfaceArray _:  (self sizeof: #SqueakSurface) * newMaxSurfaces.
			newSurfaceArray ifNil: [ ^ 0 ].
			surfaceArray := newSurfaceArray.
			maxSurfaces := newMaxSurfaces.
			numSurfaces to: maxSurfaces - 1 do: [ :i | 
				self surfaceIndex: i putHandle: 0.
				self surfaceIndex: i putDispatch: 0  ].
			index := numSurfaces ]
		ifFalse: [ 
			0 to: maxSurfaces - 1 do: [ :i | 
				(index = -1 and: [ (self dispatchOfSurfaceIndex: i) == 0 ])
					ifTrue: [ index := i ] ]].

	"No free slot found: never write at surfaceArray[-1]"
	(index < 0 or: [ index >= maxSurfaces ]) ifTrue: [ ^ 0 ].
	self surfaceIndex: index putHandle: surfaceHandle.
	self surfaceIndex: index putDispatch: fn.
	
	self 
		cCode: '*surfaceIDPtr = index' 
		inSmalltalk: [ self error ].
		
	numSurfaces := numSurfaces + 1.
	^ 1.
]

{ #category : 'exported functions' }
SurfacePlugin >> ioShowSurface: surfaceID _: x _: y _: w _: h [
	"Transfer the bits of a surface to the screen."
	<export: true>

	<var: #surfaceID type: #'int'>
	<var: #x type: #'int'>
	<var: #y type: #'int'>
	<var: #w type: #'int'>
	<var: #h type: #'int'>

	<var: #surface type: 'SqueakSurface *'>
	<var: #fnShowSurface type: 'fn_showSurface'>

	| surface fnShowSurface |

	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ self primitiveFail. ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ self primitiveFail. ^ 0 ].
	
	(fnShowSurface := self showSurfaceOf: surface)
		ifNil: [ ^ -1 ].

	^ self perform: fnShowSurface with: (self handleOf: surface) with: x with: y with: w with: h.	
]

{ #category : 'exported functions' }
SurfacePlugin >> ioUnlockSurface: surfaceID _: x _: y _: w _: h [

	"Unlock the bits of the surface. 
	The return value is ignored. "
	<export: true>
	<var: #surfaceID type: #'int'>
	<var: #x type: #'int'>
	<var: #y type: #'int'>
	<var: #w type: #'int'>
	<var: #h type: #'int'>

	<var: #surface type: 'SqueakSurface *'>
	<var: #fnUnlockSurface type: 'fn_unlockSurface'>

	| surface fnUnlockSurface |

	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ self primitiveFail. ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ self primitiveFail. ^ 0 ].
	
	(fnUnlockSurface := self unlockSurfaceOf: surface)
		ifNil: [ ^ -1 ].

	^ self perform: fnUnlockSurface with: (self handleOf: surface) with: x with: y with: w with: h.
]

{ #category : 'exported functions' }
SurfacePlugin >> ioUnregisterSurface: surfaceID [
	"Unregister the surface with the given ID.
	Returns true if successful, false otherwise."
	<export: true>
	<var: #surfaceID type: #'int'>
	<var: #surface type: 'SqueakSurface *'>

	| surface |

	(surfaceID < 0 or: [ surfaceID >= maxSurfaces ])
		ifTrue: [ ^ 0 ].

	surface := self addressOf: (surfaceArray at: surfaceID).

	(self dispatchOf: surface)
		ifNil: [ ^ 0 ].
	
	self surfaceIndex: surfaceID putHandle: 0.
	self surfaceIndex: surfaceID putDispatch: 0.
	
	numSurfaces := numSurfaces - 1.
	
	^ 1

]

{ #category : 'private - utils' }
SurfacePlugin >> lockSurfaceOf: aSurface [
	<cmacro: '(aSurface) aSurface->dispatch->lockSurface'>
	^ self shouldBeImplemented
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveCreateManualSurface [
	"arguments: name(type, stack offset)
		width(Integer, 4)
		height(Integer, 3)
		rowPitch(Integer, 2)
		depth(Integer, 1)
		isMSB(Boolean, 0)"
	| width height rowPitch depth isMSB result |
	<export: true>
	
	interpreterProxy methodArgumentCount == 5 ifFalse: [^interpreterProxy primitiveFail].
	width := interpreterProxy stackIntegerValue: 4.
	height := interpreterProxy stackIntegerValue: 3.
	rowPitch := interpreterProxy stackIntegerValue: 2.
	depth := interpreterProxy stackIntegerValue: 1.
	isMSB := interpreterProxy stackObjectValue: 0.
	isMSB := interpreterProxy booleanValueOf: isMSB. 
	interpreterProxy failed ifTrue: [^nil].
	
	"createManualSurface() takes C ints: reject here what it would truncate.
	Its pitch check is done in 64 bits, so width * depth may exceed an int."
	(width < 0 or: [ width > 16r7FFFFFFF ]) ifTrue: [^interpreterProxy primitiveFail].
	(height < 0 or: [ height > 16r7FFFFFFF ]) ifTrue: [^interpreterProxy primitiveFail].
	(rowPitch < 0 or: [ rowPitch > 16r7FFFFFFF ]) ifTrue: [^interpreterProxy primitiveFail].
	(depth < 1 or: [ depth > 32 ]) ifTrue: [^interpreterProxy primitiveFail].
	
	self touch: width; touch: height; touch: rowPitch; touch: depth; touch: isMSB.
	
	result := self cCode: 'createManualSurface(width, height, rowPitch, depth, isMSB)'.
	result < 0 ifTrue: [^interpreterProxy primitiveFail].
	result := interpreterProxy signed32BitIntegerFor: result.
	interpreterProxy pop: 6 thenPush: result
	
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveDestroyManualSurface [
	"arguments: name(type, stack offset)
		surfaceID(Integer, 0)"
	| surfaceID result |
	<export: true>
	
	interpreterProxy methodArgumentCount == 1 ifFalse: [^interpreterProxy primitiveFail].
	surfaceID := interpreterProxy stackIntegerValue: 0.
	interpreterProxy failed ifTrue: [^nil].
	result := self cCall: 'destroyManualSurface' withArguments: {surfaceID}.
	result = 0 ifTrue: [^interpreterProxy primitiveFail].
	^interpreterProxy pop: 1
	
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveFindSurface [
	"arguments: name(type, stack offset)
		externalID(Integer, 1)
		surfaceHandleHolder(ByteArray(8) on 64 bits, ByteArray(4) on 32 bits, 0) "

	| externalID surfaceHandleHolder handle result |
	<export: true>
	<var: #result type:'int'>
	<var: #handle type: #'sqIntptr_t'>
	
	interpreterProxy methodArgumentCount == 2 ifFalse: [^interpreterProxy primitiveFail].
	
	externalID := interpreterProxy stackIntegerValue: 1.
	surfaceHandleHolder := interpreterProxy stackObjectValue: 0.
	interpreterProxy failed ifTrue: [^nil].
	
	(interpreterProxy isBytes: surfaceHandleHolder)
		ifFalse: [ ^ interpreterProxy primitiveFail ].
	
	"Look up first, so a missing ID answers false whatever the holder"
	handle := 0.
	result := self ioFindSurface: externalID _: 0 _: (self addressOf: handle).
	result ifFalse: [
		interpreterProxy pop: 3 thenPush: interpreterProxy falseObject.
		^ nil ].
	
	"The handle is a whole sqIntptr_t"
	(interpreterProxy byteSizeOf: surfaceHandleHolder) >= (self sizeof: #sqIntptr_t)
		ifFalse: [ ^ interpreterProxy primitiveFail ].
	
	(interpreterProxy isOopImmutable: surfaceHandleHolder)
		ifTrue: [ ^ interpreterProxy primitiveFailFor: PrimErrNoModification ].
	
	(self cCoerce: (interpreterProxy firstIndexableField: surfaceHandleHolder) to: #'sqIntptr_t *')
		at: 0 put: handle.
	
	interpreterProxy pop: 3 thenPush: interpreterProxy trueObject
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveRegisterSurface [
	"arguments: name(type, stack offset)
		surfaceHandle(ExternalAddress, 2)
		dispatch(ExternalAddress, 1)
		surfaceIDHolder(ByteArray(4), 2) "

	| surfaceHandle surfaceHandlePtr surfaceIDHolder surfaceIDPtr result sqSurfaceDispatchPtr dispatchAddress |

	<export: true>
	<var: #result type:'int'>
	<var: #surfaceHandlePtr type: #'sqIntptr_t'>
	<var: #surfaceIDPtr type: 'void*'>
	<var: #sqSurfaceDispatchPtr type: #'sqSurfaceDispatch *'>
	
	interpreterProxy methodArgumentCount == 3 
		ifFalse: [^interpreterProxy primitiveFail].
	
	surfaceHandle := interpreterProxy stackObjectValue: 2.
	dispatchAddress := interpreterProxy stackObjectValue: 1.
	surfaceIDHolder := interpreterProxy stackObjectValue: 0.
	interpreterProxy failed ifTrue: [^nil].
	
	(interpreterProxy isBytes: surfaceIDHolder)
		ifFalse: [ ^ interpreterProxy primitiveFail ].

	(interpreterProxy byteSizeOf: surfaceIDHolder) >= 4
		ifFalse: [ ^ interpreterProxy primitiveFail ].

	(interpreterProxy isOopImmutable: surfaceIDHolder)
		ifTrue: [ ^ interpreterProxy primitiveFailFor: PrimErrNoModification ].

	(interpreterProxy is: dispatchAddress KindOfClass: interpreterProxy classExternalAddress)
		ifFalse: [ ^ interpreterProxy primitiveFail ].

	(interpreterProxy is: surfaceHandle KindOfClass: interpreterProxy classExternalAddress)
		ifFalse: [ ^ interpreterProxy primitiveFail ].

	
	surfaceIDPtr := interpreterProxy firstIndexableField: surfaceIDHolder.
	sqSurfaceDispatchPtr := self cCoerce: (interpreterProxy fetchPointer: 0 ofObject: dispatchAddress) to: #'sqSurfaceDispatch *'.
	surfaceHandlePtr := interpreterProxy fetchPointer: 0 ofObject: surfaceHandle.
	
	result := self ioRegisterSurface: surfaceHandlePtr _: sqSurfaceDispatchPtr _: surfaceIDPtr.
	
	interpreterProxy pop: 4 thenPush: (result ifTrue: [interpreterProxy trueObject] ifFalse: [interpreterProxy falseObject])
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveSetManualSurfacePointer [
	"Create a 'manual surface' data-structure.  See the ExternalForm class in the FFI package for example usage."
	"arguments: name(type, stack offset)
		surfaceID(Integer, 1)
		ptr(uint32, 0)"
	| surfaceID ptr result |
	<export: true>
	<var: #ptr type: #'usqIntptr_t'>
	
	interpreterProxy methodArgumentCount == 2 ifFalse: [^interpreterProxy primitiveFail].
	surfaceID := interpreterProxy stackIntegerValue: 1.
	ptr := interpreterProxy positiveMachineIntegerValueOf: (interpreterProxy stackValue: 0).
	interpreterProxy failed ifTrue: [^nil].

	self touch: surfaceID; touch: ptr.
	
	result := self cCode: 'setManualSurfacePointer(surfaceID, (void*)ptr)'.
	result = 0 ifTrue: [^interpreterProxy primitiveFail].
	^interpreterProxy pop: 2
	
]

{ #category : 'primitives' }
SurfacePlugin >> primitiveUnregisterSurface [
	"arguments: name(type, stack offset)
		surfaceID(Integer, 0) "

	| surfaceID result |

	<export: true>
	<var: #result type:'int'>
	
	interpreterProxy methodArgumentCount == 1 
		ifFalse: [^interpreterProxy primitiveFail].
	
	surfaceID := interpreterProxy stackIntegerValue:  0.
	interpreterProxy failed ifTrue: [^nil].
	
	result := self ioUnregisterSurface: surfaceID.
	
	interpreterProxy pop: 2 thenPush: (result ifTrue: [interpreterProxy trueObject] ifFalse: [interpreterProxy falseObject])
	
]

{ #category : 'private - utils' }
SurfacePlugin >> showSurfaceOf: aSurface [
	<cmacro: '(aSurface) aSurface->dispatch->showSurface'>
	^ self shouldBeImplemented
]

{ #category : 'initialize' }
SurfacePlugin >> shutdownModule [
	<export: true>

	"This module can only be shut down if no surfaces are registered"
	(numSurfaces = 0) 
		ifFalse: [ ^ 0 ].

	self free: surfaceArray.
	"Forget the freed array, so that no later call reads or writes it"
	surfaceArray := nil.
	maxSurfaces := 0.
	
	^ 1.
	

]

{ #category : 'private - utils' }
SurfacePlugin >> surfaceIndex: i putDispatch: aValue [

	<cmacro: '(i,aValue) surfaceArray[i].dispatch = aValue'>

	^ self shouldBeImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> surfaceIndex: i putHandle: aValue [

	<cmacro: '(i,aValue) surfaceArray[i].handle = aValue'>

	^ self shouldBeImplemented
]

{ #category : 'private - utils' }
SurfacePlugin >> unlockSurfaceOf: aSurface [
	<cmacro: '(aSurface) aSurface->dispatch->unlockSurface'>
	^ self shouldBeImplemented
]
