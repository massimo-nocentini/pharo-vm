"
I copy the Pharo world's Form to the page's canvas. Each damaged rectangle is blitted into the RGBA frame of the WebDisplayPlugin, and the frame is then presented once per Morphic cycle; the page paints it when the VM returns to the worker. The frame is the page's only one: I draw only while the page shows my window (OSWebBackendWindow>>#isShown).

Natively the primitives fail, and I only count what the page would have received (see #stats).
"
Class {
	#name : 'OSWebFormRenderer',
	#superclass : 'OSWindowFormRenderer',
	#instVars : [
		'blits',
		'blitPixels',
		'presents'
	],
	#category : 'OSWindow-Web',
	#package : 'OSWindow-Web'
}

{ #category : 'initialization' }
OSWebFormRenderer >> initialize [

	super initialize.
	blits := 0.
	blitPixels := 0.
	presents := 0
]

{ #category : 'accessing' }
OSWebFormRenderer >> outputExtent [
	"The canvas has one pixel per CSS pixel, so OSWorldRenderer keeps its canvasScaleFactor at 1. The Form extent would not do: after the page shrinks the canvas to less than half, the old Form would ask for a scale of 2"

	^ backendWindow extent
]

{ #category : 'primitives' }
OSWebFormRenderer >> primBlit: bits width: width height: height depth: depth left: left top: top right: right bottom: bottom [
	"Copy the rows top to bottom - 1 and the columns left to right - 1 of the 32-bit Form bits into the frame of the page"

	<primitive: #primitiveWebBlit module: #WebDisplayPlugin>
	blits := blits + 1.
	blitPixels := blitPixels + (right - left * (bottom - top))
]

{ #category : 'primitives' }
OSWebFormRenderer >> primPresent [

	<primitive: #primitiveWebPresent module: #WebDisplayPlugin>
	presents := presents + 1
]

{ #category : 'accessing' }
OSWebFormRenderer >> stats [
	"Answer the blits, the blitted pixels and the presents that fell back to Smalltalk"

	^ {
		  blits.
		  blitPixels.
		  presents }
]

{ #category : 'updating screen' }
OSWebFormRenderer >> updateAll [

	form ifNil: [ ^ self ].
	self updateAreas: { form boundingBox } immediate: true
]

{ #category : 'morphic integration' }
OSWebFormRenderer >> updateAreas: allDamage immediate: forceToScreen [

	| damaged |
	form ifNil: [ ^ self ].
	(backendWindow isNil or: [ backendWindow isShown ]) ifFalse: [ ^ self ].
	damaged := false.
	allDamage do: [ :rectangle |
		(self updateRectangle: rectangle) ifTrue: [ damaged := true ] ].
	damaged ifTrue: [ self primPresent ]
]

{ #category : 'updating screen' }
OSWebFormRenderer >> updateRectangle: aRectangle [
	"Blit the part of aRectangle inside the Form, widened to whole pixels. Answer whether there was one"

	| area |
	form ifNil: [ ^ false ].
	area := aRectangle intersect: form boundingBox ifNone: [ ^ false ].
	area := area origin floor corner: area corner ceiling.
	area hasPositiveExtent ifFalse: [ ^ false ].
	self
		primBlit: form bits
		width: form width
		height: form height
		depth: form depth
		left: area left
		top: area top
		right: area right
		bottom: area bottom.
	^ true
]
