"Lane 77-interpreter: the objects of the interpreter: at:, at:put:, size, new,
 class, the stores and their write barrier, and the method cache.

	pharo --headless Pharo.image --no-default-preferences st --quit interp-objects.st

 Prints lines that must be the same on every VM.  Where a bytecode has a
 primitive, its answer is compared with the one of the primitive, sent by
 perform:, and a difference is a line 'DIFF ...'.  (interp-methods.st has the methods
 that become others.)"

| out say describe checksum make receivers indices values define c p churn age old barriers keep |
out := [ :s | Stdio stdout nextPutAll: s; lf; flush ].
say := [ :label :block | out value: label , ': ' , ([ block value printString ] on: Error do: [ :e | 'ERR ' , e class name ]) ].
checksum := [ :h :s | s inject: h into: [ :acc :ch | (acc * 33 + ch asInteger) bitAnd: 16rFFFFFFFFF ] ].
describe := [ :r | (r isNumber or: [ r isCharacter or: [ r isNil or: [ r isSymbol or: [ r isKindOf: Boolean ] ] ] ])
	ifTrue: [ r printString ] ifFalse: [ r isString ifTrue: [ r asString ] ifFalse: [ 'a' , r class name ] ] ].
define := [ :name :superclass :slots :layout |
	((superclass << name) slots: slots; layout: layout; package: 'Interp-Test') install ].

"---- at:, at:put:, size, basicSize, class, == of many classes and indices: the bytecode, then perform:"
make := [ { Array new: 5. Array new: 255. Array new: 300. 'abcdef' copy. (WideString new: 4). #[1 2 3 4] copy. ByteArray new: 2000.
	(DoubleByteArray new: 3). (WordArray new: 3). (DoubleWordArray new: 3). (WeakArray new: 3). OrderedCollection withAll: #(1 2 3).
	1 to: 3. Object new. 5. 5.0. 1.0e200. $a. nil. true. (Array with: 1 with: 2) beReadOnlyObject; yourself. 'ro' copy beReadOnlyObject; yourself.
	#[1 2] copy beReadOnlyObject; yourself. 1 bitShift: 100. 1/2. Dictionary new. Float pi. Set with: 3. Semaphore new. #(nil) copy. '' copy. Array new. #[] copy. 3 -> 4. Bag new. Interval } ].
indices := { 0. 1. 2. 3. 4. 5. 6. 254. 255. 256. 300. 301. 2000. 2001. -1. 1.0. nil. 1 bitShift: 62. SmallInteger maxVal. SmallInteger minVal. $a. 1/2 }.
values := { 65. $a. 300. -1. 255. 256. nil. Character value: 955. Character value: 255. 'x'. 1.5. 1 bitShift: 40. 1 bitShift: 64 }.
1 to: 3 do: [ :round | | h compare objects twins |
	h := 0.
	objects := make value. twins := make value.
	compare := [ :label :r :q |
		(describe value: r) = (describe value: q) ifFalse: [ out value: ('DIFF {1}: {2}, not {3}' format: { label. describe value: r. describe value: q }) ].
		h := checksum value: h value: (describe value: r) ].
	objects with: twins do: [ :x :y |
		indices do: [ :i |
			compare value: x class name , ' at: ' , i printString
				value: ([ x at: i ] on: Error do: [ :e | e class name asSymbol ])
				value: ([ y perform: #at: with: i ] on: Error do: [ :e | e class name asSymbol ]).
			compare value: x class name , ' basicAt: ' , i printString
				value: ([ x basicAt: i ] on: Error do: [ :e | e class name asSymbol ])
				value: ([ y perform: #basicAt: with: i ] on: Error do: [ :e | e class name asSymbol ]).
			values do: [ :v |
				compare value: x class name , ' at: ' , i printString , ' put: ' , v printString
					value: ([ x at: i put: v ] on: Error do: [ :e | e class name asSymbol ])
					value: ([ y perform: #at:put: with: i with: v ] on: Error do: [ :e | e class name asSymbol ]).
				compare value: x class name , ' at: ' , i printString , ' after the put of ' , v printString
					value: ([ x at: i ] on: Error do: [ :e | e class name asSymbol ])
					value: ([ y perform: #at: with: i ] on: Error do: [ :e | e class name asSymbol ]) ] ].
		compare value: x class name , ' size' value: ([ x size ] on: Error do: [ :e | e class name asSymbol ]) value: ([ y perform: #size ] on: Error do: [ :e | e class name asSymbol ]).
		compare value: x class name , ' basicSize' value: ([ x basicSize ] on: Error do: [ :e | e class name asSymbol ]) value: ([ y perform: #basicSize ] on: Error do: [ :e | e class name asSymbol ]).
		compare value: x class name , ' class' value: x class name value: (y perform: #class) name ].
	out value: 'at: ' , h printString ].
"every slot of an array, with its neighbours: the first and the last of what the caches admit"
say value: 'bounds' value: [ #(0 1 2 254 255 256 1000) collect: [ :n | | a | a := Array new: n. 1 to: n do: [ :i | a at: i put: i ]. (a inject: 0 into: [ :s :e | s + e ])
	-> { [ a at: n + 1 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: 0 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: n + 1 put: 1 ] on: SubscriptOutOfBounds do: [ :e | #out ] } ] ].
say value: 'bounds of bytes' value: [ #(0 1 7 8 9 16 2040 2041) collect: [ :n | | a | a := ByteArray new: n. 1 to: n do: [ :i | a at: i put: i \\ 256 ]. (a inject: 0 into: [ :s :e | s + e ])
	-> { [ a at: n + 1 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: 0 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: n + 1 put: 1 ] on: SubscriptOutOfBounds do: [ :e | #out ] } ] ].
say value: 'bounds of strings' value: [ #(0 1 7 8 9 16 2040 2041) collect: [ :n | | a | a := String new: n. 1 to: n do: [ :i | a at: i put: (Character value: i \\ 256) ]. (a inject: 0 into: [ :s :e | s + e asInteger ])
	-> { [ a at: n + 1 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: 0 ] on: SubscriptOutOfBounds do: [ :e | #out ]. [ a at: n + 1 put: $a ] on: SubscriptOutOfBounds do: [ :e | #out ] } ] ].

"---- classes that override what is inlined, and their supers"
c := define value: #InterpA value: Object value: #(a b cc d e f g h i j k l) value: FixedLayout.
#( 'class ^ #(hello)' 'realClass ^ super class' '== x ^ #no' 'same: x ^ super == x' 'selfClass ^ self class' 'performClass ^ self perform: #class'
	'l: x l := x. ^ l' 'l ^ l' 'a: x a := x' 'a ^ a' 'h ^ h' 'h: x h := x' 'i ^ i' 'i: x i := x' 'setAll a := b := cc := d := e := f := g := h := i := j := k := l := 1. ^ { a . b . cc . d . e . f . g . h . i . j . k . l }'
	'getAll ^ a + b + cc + d + e + f + g + h + i + j + k + l' 'popAll: x a := x. b := x. cc := x. d := x. e := x. f := x. g := x. h := x. i := x. j := x. k := x. l := x' 'asArrayOfAll ^ { a . b . cc . d . e . f . g . h . i . j . k . l }' 'true1 ^ true' 'one ^ 1' 'nil1 ^ nil' 'two ^ 2' 'zero ^ 0' 'minus ^ -1' 'false1 ^ false' )
	do: [ :s | c compile: s ].
p := define value: #InterpB value: c value: #() value: VariableLayout.
p compile: 'at: i ^ #over -> (super at: i)'. p compile: 'at: i put: x ^ super at: i put: (Array with: x)'.
p := define value: #InterpArray value: Array value: #() value: VariableLayout.
p compile: 'at: i ^ #array -> (super at: i)'. p compile: 'size ^ super size + 100'. p compile: 'at: i put: x ^ super at: i put: x + 1'.
p := define value: #InterpString value: ByteString value: #() value: ByteLayout.
p compile: 'at: i ^ #string -> (super at: i)'.
p := define value: #InterpPlain value: Array value: #() value: VariableLayout.
p := define value: #InterpBoth value: Object value: #(x y) value: VariableLayout.
p compile: 'x ^ x'. p compile: 'x: v x := v'.
1 to: 3 do: [ :round |
	say value: 'super' value: [ | x | x := (Smalltalk at: #InterpA) new. { x realClass name. x same: x. x same: 3. x selfClass. x performClass. x class. (x == 3). (x perform: #== with: 3). (x ~~ 3). x setAll. x getAll. x l: 9. x l. (x a: 2; a). (x h: 4; h). (x i: 5; i). x true1. x one. x nil1. x two. x zero. x minus. x false1 } ].
	say value: 'variable' value: [ | x | x := (Smalltalk at: #InterpB) new: 4. { x at: 1 put: 3. x at: 1. x size. [ x at: 9 ] on: Error do: [ :e | e class name ]. x l: 8. x l. (x basicAt: 1) } ].
	say value: 'array' value: [ | x | x := (Smalltalk at: #InterpArray) new: 4. { x at: 2 put: 3. x at: 2. x size. (x basicAt: 2) } ].
	say value: 'plain' value: [ | x | x := (Smalltalk at: #InterpPlain) new: 4. { x at: 2 put: 3. x at: 2. x size. (x basicAt: 2). [ x at: 5 ] on: Error do: [ :e | #out ] } ].
	say value: 'both' value: [ | x | x := (Smalltalk at: #InterpBoth) new: 2. { x x: 7; at: 1 put: 3; at: 2 put: 4; x. x at: 1. x at: 2. x size. x basicSize. [ x at: 3 ] on: Error do: [ :e | #out ]. [ x at: 0 ] on: Error do: [ :e | #out ]. [ x at: 3 put: 1 ] on: Error do: [ :e | #out ] } ].
	say value: 'string' value: [ | x | x := (Smalltalk at: #InterpString) new: 2. x at: 1 put: $q. { x at: 1. x size } ].
	say value: 'read-only' value: [ | x | x := (Smalltalk at: #InterpA) new. x beReadOnlyObject. { [ x a: 5 ] on: ModificationForbidden do: [ :e | #mf ]. [ x l: 5 ] on: ModificationForbidden do: [ :e | #mf ]. [ x setAll ] on: ModificationForbidden do: [ :e | #mf ]. [ x popAll: 3 ] on: ModificationForbidden do: [ :e | #mf ]. x l. x a } ].
	say value: 'read-only at:put:' value: [ { Array new: 3. String new: 3. ByteArray new: 3 } collect: [ :x | | v | v := x isString ifTrue: [ $z ] ifFalse: [ 7 ]. x at: 1 put: v. x beReadOnlyObject. [ x at: 1 put: (x isString ifTrue: [ $y ] ifFalse: [ 8 ]) ] on: ModificationForbidden do: [ :e | #mf ]. x at: 1 ] ] ].
say value: 'new' value: [ { (Array new: 3). Object new class. String new. (Array basicNew). [ Array new: -1 ] on: Error do: [ :e | #error ]. [ SmallInteger new ] on: Error do: [ :e | #error ]. [ Character new ] on: Error do: [ :e | #error ]. Class new class. [ CompiledMethod new ] on: Error do: [ :e | #error ]. (WeakArray new) size. (Association new key: 1 value: 2) value. (Smalltalk at: #InterpA) new l. (Smalltalk at: #InterpBoth) new basicSize } ].
say value: 'many new' value: [ | s | s := 0. 1 to: 300000 do: [ :i | | x | x := (Smalltalk at: #InterpA) new. x a: i. (x l isNil and: [ x a = i ]) ifTrue: [ s := s + 1 ] ]. s ].

"---- the write barrier: old objects that are the only ones to hold young ones, after scavenges.  (Objects are old once more than
 new space, 22 MB, was allocated after them and is kept; a garbage collection alone leaves them young.)"
churn := [ 1 to: 400000 do: [ :i | Array new: 12 ] ].
age := [ | kept | kept := (1 to: 250000) collect: [ :i | Array new: 24 ]. kept size ].
old := [ :factory | | r | r := (1 to: 64) collect: factory. age value. r ].
barriers := OrderedCollection new.
keep := [ :label :objects :store :fetch | barriers add: { label. objects. store. fetch } ].
keep value: 'at:put:' value: ((1 to: 64) collect: [ :i | Array new: 4 ]) value: [ :x :i | x at: 3 put: 'young ' , i printString ] value: [ :x | x at: 3 ].
keep value: 'at:put: that the cache has' value: ((1 to: 64) collect: [ :i | | x | x := Array new: 4. x at: 1 put: 0. x ]) value: [ :x :i | x at: 1 put: i. x at: 2 put: i. x at: 3 put: 'young ' , i printString ] value: [ :x | x at: 3 ].
keep value: 'at:put: of a big array' value: ((1 to: 64) collect: [ :i | Array new: 600 ]) value: [ :x :i | x at: 599 put: i. x at: 600 put: 'young ' , i printString ] value: [ :x | x at: 600 ].
keep value: 'at:put: of a class with fields' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpBoth) new: 4 ]) value: [ :x :i | x at: 1 put: i. x at: 4 put: 'young ' , i printString ] value: [ :x | x at: 4 ].
keep value: 'basicAt:put:' value: ((1 to: 64) collect: [ :i | Array new: 4 ]) value: [ :x :i | x basicAt: 3 put: 'young ' , i printString ] value: [ :x | x at: 3 ].
keep value: 'super at:put:' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpB) new: 4 ]) value: [ :x :i | x at: 3 put: 'young ' , i printString ] value: [ :x | (x basicAt: 3) first ].
keep value: 'a slot' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpA) new ]) value: [ :x :i | x a: 'young ' , i printString ] value: [ :x | x a ].
keep value: 'the last short slot' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpA) new ]) value: [ :x :i | x h: 'young ' , i printString ] value: [ :x | x h ].
keep value: 'a far slot' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpA) new ]) value: [ :x :i | x i: 'young ' , i printString ] value: [ :x | x i ].
keep value: 'a far slot, kept on the stack' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpA) new ]) value: [ :x :i | x l: 'young ' , i printString ] value: [ :x | x l ].
keep value: 'an association' value: ((1 to: 64) collect: [ :i | Association new ]) value: [ :x :i | x value: 'young ' , i printString ] value: [ :x | x value ].
keep value: 'a temporary of a block' value: ((1 to: 64) collect: [ :i | | t | { [ :v | t := v ]. [ t ] } ]) value: [ :x :i | x first value: 'young ' , i printString ] value: [ :x | x second value ].
keep value: 'a temporary of a block, kept on the stack' value: ((1 to: 64) collect: [ :i | | t u | u := 0. { [ :v | u := t := v ]. [ t ]. [ u ] } ]) value: [ :x :i | x first value: 'young ' , i printString ] value: [ :x | (x second value == x third value) ifTrue: [ x second value ] ifFalse: [ #different ] ].
keep value: 'instVarAt:put:' value: ((1 to: 64) collect: [ :i | (Smalltalk at: #InterpA) new ]) value: [ :x :i | x instVarAt: 12 put: 'young ' , i printString ] value: [ :x | x l ].
keep value: 'replaceFrom:to:with:' value: ((1 to: 64) collect: [ :i | Array new: 4 ]) value: [ :x :i | x replaceFrom: 1 to: 4 with: { 1. 2. 'young ' , i printString. 4 } startingAt: 1 ] value: [ :x | x at: 3 ].
age value.
barriers do: [ :barrier | | objects bad |
	objects := barrier second.
	objects withIndexDo: [ :x :i | barrier third value: x value: i ].
	churn value.
	bad := 0.
	objects withIndexDo: [ :x :i | ((barrier fourth value: x) = ('young ' , i printString)) ifFalse: [ bad := bad + 1 ] ].
	out value: 'barrier ' , barrier first , ': ' , bad printString ].
say value: 'barrier, slots of every kind' value: [ | xs | xs := old value: [ :i | (Smalltalk at: #InterpA) new ].
	xs withIndexDo: [ :x :i | x popAll: i printString copy ]. churn value.
	(xs withIndexCollect: [ :x :i | x asArrayOfAll allSatisfy: [ :v | v = i printString ] ]) allSatisfy: [ :b | b ] ].
say value: 'barrier, stored again and again' value: [ | xs bad | xs := old value: [ :i | Array new: 8 ]. bad := 0.
	1 to: 40 do: [ :round | xs withIndexDo: [ :x :i | x at: round \\ 8 + 1 put: round printString copy ]. 1 to: 20000 do: [ :i | Array new: 12 ].
		xs do: [ :x | (x at: round \\ 8 + 1) = round printString ifFalse: [ bad := bad + 1 ] ] ]. bad ].

"---- the method cache: methods that change under it"
c := Smalltalk at: #InterpA.
say value: 'compiled again and again' value: [ | n | n := 0.
	1 to: 400 do: [ :i | c compile: 'm ' , (i even ifTrue: [ '| p q r s t | p := 1. ^ p + ' ] ifFalse: [ '^ 1 + ' ]) , i printString. n := n + c new m.
		i \\ 7 = 0 ifTrue: [ c removeSelector: #m. n := n + ([ c new m ] on: MessageNotUnderstood do: [ :e | 5 ]) ] ]. n ].
say value: 'a class that gets at:' value: [ | q x r | q := define value: #InterpLate value: Array value: #() value: VariableLayout. x := q new: 3. x at: 1 put: 7. r := OrderedCollection new. 1 to: 3 do: [ :i | r add: (x at: 1) ].
	q compile: 'at: i ^ #late'. 1 to: 3 do: [ :i | r add: (x at: 1) ]. q compile: 'at: i put: v ^ #lateput'. r add: (x at: 1 put: 9). q removeSelector: #at:. r add: (x at: 1). q removeSelector: #at:put:. r add: (x at: 1 put: 9); add: (x at: 1). r asArray ].
say value: 'a class that gets size, class and ==' value: [ | q x r | q := define value: #InterpLate2 value: Array value: #() value: VariableLayout. x := q new: 3. r := OrderedCollection new. r add: x size.
	q compile: 'size ^ #late'. r add: x size. q compile: 'zork ^ 1'. r add: x zork. q compile: 'zork ^ 2'. r add: x zork. q removeSelector: #size. r add: x size. r asArray ].
say value: 'another superclass' value: [ | p1 p2 k r | p1 := define value: #InterpP1 value: Object value: #() value: FixedLayout. p2 := define value: #InterpP2 value: Object value: #() value: FixedLayout. p1 compile: 'z ^ 1'. p2 compile: 'z ^ 2'.
	k := define value: #InterpK value: p1 value: #() value: FixedLayout. k compile: 'zz ^ super z'. r := { k new z. k new zz }. k superclass: p2. Object flushCache. r , { k new z. k new zz } ].
say value: 'one selector, many classes' value: [ | classes h | classes := (1 to: 60) collect: [ :i | | k | k := define value: ('InterpMega' , i printString) asSymbol value: Object value: #() value: FixedLayout. k compile: 'id ^ ' , i printString. k compile: 'twice ^ self id * 2'. k ].
	h := 0. 1 to: 200 do: [ :r | classes do: [ :k | h := h + k new id + k new twice ] ]. h ].
say value: 'many selectors' value: [ | h | 1 to: 300 do: [ :i | c compile: 'sel' , i printString , ' ^ ' , i printString ]. h := 0. 1 to: 20 do: [ :r | 1 to: 300 do: [ :i | h := h + (c new perform: ('sel' , i printString) asSymbol) ] ]. h ].
say value: 'an object as a method' value: [ [ c methodDict at: #zork put: (MessageSend receiver: 3 selector: #foo). Object flushCache. c new zork ] on: Error do: [ :e | #error ] ].
c methodDict removeKey: #zork ifAbsent: [ ]. Object flushCache.
say value: 'quick methods that change' value: [ | x r | x := c new. r := OrderedCollection new. #( 'q ^ 1' 'q ^ nil' 'q ^ self' 'q ^ a' 'q ^ true' 'q ^ l' 'q ^ 3' 'q ^ #s' ) do: [ :s | c compile: s. x a: 10; l: 20. r add: ([ :v | v == x ifTrue: [ #self ] ifFalse: [ v ] ] value: x q) ]. r asArray ].

"---- identity, become: and classes"
say value: 'become:' value: [ | x y | x := Array with: 1 with: 2. y := Array with: 3 with: 4. x at: 1. x become: y. { x at: 1. y at: 2. x == y. x size } ].
say value: 'becomeForward:' value: [ | x y z | x := Array with: 1 with: 2. z := { x }. y := 'str' copy. x becomeForward: y. { z first. x == y. z first == y. z first ~~ y. x size. x at: 1. z first class } ].
say value: 'becomeForward: of many' value: [ | xs ys zs | xs := (1 to: 50) collect: [ :i | Array with: i with: 0 ]. ys := (1 to: 50) collect: [ :i | Array with: i * 10 ]. zs := xs collect: [ :x | { x } ]. xs do: [ :x | x at: 1 ]. xs asArray elementsForwardIdentityTo: ys asArray.
	(1 to: 50) collect: [ :i | (zs at: i) first size * 100 + ((zs at: i) first at: 1) ] ].
say value: 'become: of a block' value: [ | x y | x := [ :q | q + 1 ]. y := [ :q | q + 2 ]. { x value: 1. (x become: y) value: 1. x value: 1. y value: 1 } ].
say value: 'another class' value: [ | x | x := Array with: 1 with: 2. x at: 1. x size. (Smalltalk at: #InterpPlain) adoptInstance: x. { x class. x at: 1. x size } ].
say value: 'classes' value: [ { 3 class. 3.0 class. 1.0e200 class. $a class. nil class. true class. 'a' class. #a class. Object class. Object class class. (1 bitShift: 70) class. thisContext class. (3 -> 4) class } ].
say value: 'a class with another slot' value: [ | k x r | k := define value: #InterpZ value: Object value: #() value: FixedLayout. x := k new. r := OrderedCollection new. r add: x class instSize. k := define value: #InterpZ value: Object value: #(q) value: FixedLayout. r add: x class instSize. k compile: 'q ^ q'. k compile: 'q: v q := v'. r add: x q; add: (x q: 4; q); add: k new q. r asArray ].
say value: 'weak' value: [ | x | x := WeakArray new: 2. x at: 1 put: Object new. x at: 2 put: 3. Smalltalk garbageCollect. { x at: 1. x at: 2. [ x at: 3 ] on: Error do: [ :e | #out ] } ].
say value: 'garbage collections' value: [ | k | k := OrderedCollection new. 1 to: 300000 do: [ :i | k add: { i. i printString. [ i ] }. i \\ 100000 = 0 ifTrue: [ Smalltalk garbageCollect. k := k copyFrom: k size // 2 to: k size ] ]. (k inject: 0 into: [ :x :y | x + y first + y second size + y third value ]) ].
out value: 'end'.
Smalltalk exitSuccess!
