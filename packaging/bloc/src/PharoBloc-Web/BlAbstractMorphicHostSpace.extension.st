Extension { #name : 'BlAbstractMorphicHostSpace' }

{ #category : '*PharoBloc-Web' }
BlAbstractMorphicHostSpace >> newExternalForm [
	"A buffer of my space: Bloc's BlExternalForm, a manual surface of SurfacePlugin, as BlocHost-Morphic of Bloc v3.1.0 has it, or a PharoBlocMorphicForm on a VM without SurfacePlugin (PharoBloc-Web replaces Bloc's method with this one)"

	| form |
	PharoBlocMorphicForm isNeeded ifTrue: [ ^ PharoBlocMorphicForm newExtent: self physicalSize asPoint ].
	form := BlExternalForm extent: self physicalSize asPoint depth: 32.
	form autoRelease.
	form allocateSpace.
	form fillColor: Color transparent.
	^ form
]
