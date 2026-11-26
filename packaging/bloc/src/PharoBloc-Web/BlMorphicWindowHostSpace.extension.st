Extension { #name : 'BlMorphicWindowHostSpace' }

{ #category : '*PharoBloc-Web' }
BlMorphicWindowHostSpace >> cursor: aCursor [
	"Change current cursor image/view to the given one, the hand staying where it points (PharoBloc-Web replaces Bloc's method, which leaves the hand one pixel off: HandMorph>>#showTemporaryCursor:hotSpotOffset: of Pharo 15 moves the position of the hand by the offset of the cursor, -1@-1 for the arrow, until the next move of the mouse)"

	| hand point |
	hand := self currentHand.
	point := hand position.
	hand showTemporaryCursor: aCursor.
	hand position: point
]
