"Lane 77-interpreter: the method cache, when methods become others.

	pharo --headless Pharo.image --no-default-preferences st --quit interp-methods.st

 The fast interpreter keeps in the method cache what the activation of a
 method needs of its header: a method whose contents become: swaps, or whose
 header is stored, must be activated by the new one.  (A JIT does not cope
 with that, so this is not compared with a native VM.)"

| out say c |
out := [ :s | Stdio stdout nextPutAll: s; lf; flush ].
say := [ :label :block | out value: label , ': ' , ([ block value printString ] on: Error do: [ :e | 'ERR ' , e class name ]) ].
c := (Object << #InterpA slots: { #a }; package: 'Interp-Test') install.
say value: 'two-way become: of methods' value: [ | x a b r |
	c compile: 'foo: a | p q r | p := a. q := p + 1. r := q + 1. ^ { p . q . r }'.
	c compile: 'bar: a | p | p := a. ^ { p . nil . #bar }'.
	x := c new. r := OrderedCollection new. 1 to: 10 do: [ :i | x foo: i. x bar: i ].
	a := c >> #foo:. b := c >> #bar:.
	a become: b.
	r add: (x foo: 3); add: (x bar: 3).
	a becomeForward: b.
	r add: (x foo: 3); add: (x bar: 3). r asArray ].
say value: 'become: of methods with more temporaries' value: [ | x r |
	c compile: 'less ^ #less'. c compile: 'more | a1 a2 a3 a4 a5 a6 a7 a8 a9 a10 | a1 := a2 := a3 := a4 := a5 := a6 := a7 := a8 := a9 := a10 := 3. ^ { a1 . a5 . a10 } '.
	x := c new. r := OrderedCollection new. 1 to: 5 do: [ :i | x less. x more ].
	(c >> #less) become: (c >> #more). r add: x less; add: x more. 1 to: 5 do: [ :i | r add: x less; add: x more ]. r asArray ].
say value: 'the header of a method stored' value: [ | m x r | c compile: 'hh | t | t := self. ^ { t == self. thisContext size > 0 }'. m := c >> #hh. x := c new. r := OrderedCollection new. r add: x hh. m objectAt: 1 put: (m objectAt: 1). r add: x hh. r asArray ].
say value: 'a header with more temporaries stored' value: [ | m x r seen | c compile: 'temps ^ thisContext stackPtr'. m := c >> #temps. x := c new. r := OrderedCollection new. 1 to: 3 do: [ :i | r add: x temps ].
	m objectAt: 1 put: (m objectAt: 1) + (4 bitShift: 18). seen := x temps. r add: seen - r first. 1 to: 3 do: [ :i | r add: x temps - r first ]. r asArray ].
out value: 'end'.
Smalltalk exitSuccess!
