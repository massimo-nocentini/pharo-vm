Extension { #name : 'AthensCairoSurface' }

{ #category : '*OSWindow-Web' }
AthensCairoSurface class >> registerSurface: anAthensCairoSurface [
	"The web VMs have no SurfacePlugin, through which the stock method shares the pixels of a surface with BitBlt, and fails without: register nothing, and answer nil for the id slot of the surface, where #asForm keeps a copy of the pixels instead. Natively the prepared image takes this path too"

	self checkSession.
	^ nil
]

{ #category : '*OSWindow-Web' }
AthensCairoSurface class >> unregisterSurfaceWithId: anAthensCairoSurfaceId [
	"Nothing was registered (see #registerSurface:)"
]

{ #category : '*OSWindow-Web' }
AthensCairoSurface >> asForm [
	"A Form of my extent holding a copy of my pixels as they are now, the premultiplied ARGB32 of cairo, as the Form of the stock method holds them through the SurfacePlugin (see AthensCairoSurface class>>#registerSurface:): drawing it gives the same pixels. The Form is kept in my id slot, which the SurfacePlugin left free, and reused while my extent is the same, so a Form answered earlier takes my newer pixels at each call, where the stock one always shows them. My resource data, taken while the id slot was still nil, does not hold the Form: finalizing me keeps no copy of my pixels"

	| form |
	self checkSession.
	self flush.
	form := id.
	(form isForm and: [ form extent = self extent ]) ifFalse: [
		form := Form extent: self extent depth: 32.
		id := form ].
	"(the rows of a 32-bit format have 4 bytes a pixel and no padding, as the Form's have: the minimum only keeps a surface of another format from writing past the Form)"
	LibC
		memCopy: self getDataPtr
		to: form bits
		size: (self stride * self height min: form bits byteSize).
	^ form
]
