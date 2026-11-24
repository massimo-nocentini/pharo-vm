"Lane 77-interpreter: the control flow of the interpreter: sends, blocks,
 jumps, returns, contexts, exceptions and processes.

	pharo --headless Pharo.image --no-default-preferences st --quit interp-control.st

 Prints lines that must be the same on every VM."

| out say c s deep |
out := [ :x | Stdio stdout nextPutAll: x; lf; flush ].
say := [ :label :block | out value: label , ': ' , ([ block value printString ] on: Error do: [ :e | 'ERR ' , e class name ]) ].
c := (Object << #InterpC slots: { #a. #n }; sharedVariables: { #Shared }; package: 'Interp-Test') install.

"---- temporaries, literals and jumps of every size"
c compile: 'temps ' , (String streamContents: [ :w | w nextPutAll: '|'. 1 to: 40 do: [ :i | w nextPutAll: ' t'; print: i ]. w nextPutAll: ' | t1 := 0. '. 2 to: 40 do: [ :i | w nextPutAll: 't'; print: i; nextPutAll: ' := t'; print: i - 1; nextPutAll: ' + 1. ' ]. w nextPutAll: '^ [ :x | t40 + t1 + t30 + x ]' ]).
say value: 'many temporaries' value: [ (1 to: 50) inject: 0 into: [ :x :y | x + (c new temps value: y) ] ].
c compile: 'temps2 ' , (String streamContents: [ :w | w nextPutAll: '|'. 1 to: 20 do: [ :i | w nextPutAll: ' t'; print: i ]. w nextPutAll: ' | '. 1 to: 20 do: [ :i | w nextPutAll: 't'; print: i; nextPutAll: ' := '; print: i; nextPutAll: '. ' ]. w nextPutAll: '^ { '. 1 to: 20 do: [ :i | w nextPutAll: 't'; print: i; nextPutAll: '. ' ]. w nextPutAll: '(t1 := t20). (t20 := t2). t1 . t20 }' ]).
say value: 'temporaries one by one' value: [ c new temps2 ].
c compile: 'lits ^ { ' , (String streamContents: [ :w | 1 to: 60 do: [ :i | w print: i + 10000; nextPutAll: '. #s'; print: i; nextPutAll: '. ' ] ]) , '}'.
say value: 'many literals' value: [ | l | l := c new lits. l size -> (l inject: 0 into: [ :x :y | y isSymbol ifTrue: [ x + y size ] ifFalse: [ x + y ] ]) ].
c compile: 'globals ^ ((1 to: 40) collect: [ :i | Shared ]) asArray'. c compile: 'shared: x Shared := x'. c compile: 'shared ^ Shared'.
say value: 'shared variables' value: [ | o | o := c new. o shared: 3. { o shared. o globals size. (o shared: 'x' copy; shared) } ].
c compile: 'manyGlobals ^ { Object. String. Array. OrderedCollection. Dictionary. Set. Bag. Float. Integer. Fraction. Symbol. Character. Boolean. True. False. UndefinedObject. Class. Metaclass. Point. Rectangle. Date. Time. Interval. Association. Error } collect: [ :x | x name size ]'.
say value: 'many global variables' value: [ c new manyGlobals ].
c compile: 'loop: k | sum i | sum := 0. i := 0. [ i < k ] whileTrue: [ ' , (String streamContents: [ :w | 1 to: 60 do: [ :i | w nextPutAll: 'i \\ '; print: i + 1; nextPutAll: ' = 0 ifTrue: [ sum := sum + (self a: '; print: i; nextPutAll: ') a ] ifFalse: [ sum := sum - 1 ]. ' ] ]) , ' i := i + 1 ]. ^ sum'.
c compile: 'a: x a := x'. c compile: 'a ^ a'. c compile: 'n ^ n'. c compile: 'n: x n := x'.
say value: 'long jumps' value: [ c new loop: 3000 ].
c compile: 'far: x ^ x ifTrue: [ { ' , (String streamContents: [ :w | 1 to: 100 do: [ :i | w nextPutAll: 'self a: '; print: i; nextPutAll: '. ' ] ]) , ' } size ] ifFalse: [ { ' , (String streamContents: [ :w | 1 to: 90 do: [ :i | w nextPutAll: 'self a: '; print: i; nextPutAll: '. ' ] ]) , ' } size ]'.
say value: 'far conditional jumps' value: [ { c new far: true. c new far: false. [ c new far: nil ] on: Error do: [ :e | e class name ] } ].
say value: 'not a Boolean' value: [ { 3. nil. 'a'. 3.0 } collect: [ :x | { [ x ifTrue: [ 1 ] ] on: Error do: [ :e | e class name ]. [ x ifFalse: [ 1 ] ifTrue: [ 2 ] ] on: Error do: [ :e | e class name ]. [ [ x ] whileTrue: [ 1 ] ] on: Error do: [ :e | e class name ] } ] ].
c compile: 'cases: x x = 1 ifTrue: [ ^ #one ]. x < 5 ifTrue: [ ^ #few ]. x > 100 ifTrue: [ x > 1000 ifTrue: [ ^ #huge ] ifFalse: [ ^ #many ] ]. (x = 50 or: [ x = 60 ]) ifTrue: [ ^ #round ]. (x > 20 and: [ x ~= 33 ]) ifFalse: [ ^ #other ]. ^ x \\ 2 == 0 ifTrue: [ #even ] ifFalse: [ #odd ]'.
say value: 'cases' value: [ #( 1 2 5 50 60 70 71 33 10 101 1001 ) collect: [ :x | c new cases: x ] ].
c compile: 'nilOr: x ^ { x ifNil: [ #nil ]. x ifNotNil: [ :y | y -> y ]. x == nil. nil == x. x isNil. x notNil. x ifNil: [ 1 ] ifNotNil: [ 2 ]. (x ~~ nil) ifTrue: [ 3 ] ifFalse: [ 4 ] }'.
say value: 'nil or not' value: [ { c new nilOr: nil. c new nilOr: false. c new nilOr: 3 } ].
say value: 'loops' value: [ | r | r := OrderedCollection new. 1 to: 10 by: 3 do: [ :i | r add: i ]. 10 to: 1 by: -4 do: [ :i | r add: i ]. 1 to: 0 do: [ :i | r add: #never ]. 1.5 to: 3 do: [ :i | r add: i ]. SmallInteger maxVal - 2 to: SmallInteger maxVal do: [ :i | r add: SmallInteger maxVal - i ]. 5 timesRepeat: [ r add: 0 ]. r asArray ].

"---- sends"
c compile: 'args: a1 b: a2 c: a3 d: a4 e: a5 f: a6 g: a7 h: a8 i: a9 j: a10 k: a11 l: a12 m: a13 ^ [ :z | { z. a1. a2. a3. a4. a5. a6. a7. a8. a9. a10. a11. a12. a13 } ] value: a2'.
say value: 'many arguments' value: [ c new args: 1 b: 2 c: 3 d: 4 e: 5 f: 6 g: 7 h: 8 i: 9 j: 10 k: 11 l: 12 m: 13 ].
c compile: 'id ^ self'. c compile: 'one: x ^ x'. c compile: 'one: x two: y ^ { x. y }'. c compile: 'one: x two: y three: z ^ { x. y. z }'. c compile: 'one: x two: y three: z four: t ^ { x. y. z. t }'.
say value: 'arguments' value: [ | o | o := c new. { o id == o. o one: 1. o one: 1 two: 2. o one: 1 two: 2 three: 3. o one: 1 two: 2 three: 3 four: 4. (o one: (o one: 1 two: 2) two: (o one: 3)) } ].
say value: 'perform:' value: [ { 3 perform: #+ with: 4. #(1 2) perform: #at: with: 2. [ 3 perform: #foo ] on: MessageNotUnderstood do: [ :e | e message selector ]. (c perform: #new) class. (c new perform: #id) class. [ 3 perform: #+ ] on: Error do: [ :e | #error ]. (c new perform: #one:two: withArguments: #(5 6)) } ].
say value: 'not understood' value: [ { [ 3 foo ] on: MessageNotUnderstood do: [ :e | e message selector ]. [ nil foo: 1 bar: 2 ] on: MessageNotUnderstood do: [ :e | e message arguments ]. [ 3 foo ] on: MessageNotUnderstood do: [ :e | e return: 7 ]. [ 3 foo + 1 ] on: MessageNotUnderstood do: [ :e | e resume: 7 ] } ].
c compile: 'doesNotUnderstand: m ^ m selector -> m arguments'.
say value: 'doesNotUnderstand:' value: [ | o | o := c new. { o zork. o zork: 1. o zork: 1 with: 2. (o zork; zork: 5) } ].
s := (c << #InterpD slots: { #z }; package: 'Interp-Test') install.
c compile: 'who ^ #c'. s compile: 'who ^ #d -> super who'. c compile: 'me ^ self who'. s compile: 'later ^ [ super who ]'. s compile: 'one: x two: y ^ super one: y two: x'. c compile: 'z ^ #none'. s compile: 'z ^ z ifNil: [ super z ]'. s compile: 'n: x ^ super n: x + 1'.
say value: 'super' value: [ | o | o := s new. { o who. o me. o later value. o one: 1 two: 2. o z. (o n: 4; n). o id == o } ].
say value: 'class side' value: [ c class compile: 'make ^ self new n: 3; yourself'. s class compile: 'make ^ super make'. { c make n. s make n. s make class name } ].

"---- blocks"
say value: 'blocks' value: [ { [ ] value. [ 3 ] value. [ :x | x ] value: 4. [ :x :y | x - y ] value: 5 value: 3. [ :x :y :t | x * y + t ] value: 5 value: 3 value: 1. [ :x :y :t :u | { x. y. t. u } ] value: 1 value: 2 value: 3 value: 4. [ nil ] value. [ :x | ] value: 3 } ].
say value: 'wrong numbers of arguments' value: [ (0 to: 5) collect: [ :k | [ [ :x :y | x + y ] valueWithArguments: (Array new: k withAll: 1) ] on: Error do: [ :e | #error ] ] ].
say value: 'wrong numbers of arguments, sent' value: [ { [ [ :n1 | n1 ] value ] on: Error do: [ :e | #error ]. [ [ 1 ] value: 2 ] on: Error do: [ :e | #error ]. [ [ :n1 | n1 ] value: 1 value: 2 ] on: Error do: [ :e | #error ] } ].
say value: 'cull:' value: [ { [ 1 ] cull: 2. [ :x | x ] cull: 2. [ :x | x ] cull: 2 cull: 3 } ].
say value: 'value of others' value: [ { 3 value. #size value: 'abc'. nil value. 'a' value. (MessageSend receiver: 3 selector: #+ argument: 4) value } ].
say value: 'copied and written variables' value: [ | x blocks | x := 0. blocks := (1 to: 5) collect: [ :i | | y | y := i * 10. { [ x := x + i ]. [ y := y + x ]. [ y ] } ].
	blocks do: [ :b | b first value. b second value ]. { x. blocks collect: [ :b | b third value ] } ].
say value: 'nested' value: [ ([ :x | [ :y | [ :t | x + y + t ] ] ] value: 1) in: [ :f | ((f value: 2) value: 3) + ((f value: 20) value: 30) ] ].
c compile: 'counter | k | k := 0. ^ [ k := k + 1. [ k := k * 2. self n: k. k ] value ]'.
say value: 'counter' value: [ | o f | o := c new. f := o counter. { f value. f value. f value. o n } ].
c compile: 'find: x do: aBlock x do: [ :y | [ (aBlock value: y) ifTrue: [ ^ y ] ] ensure: [ n := (n ifNil: [ 0 ]) + 1 ] ]. ^ nil'.
say value: 'non-local return' value: [ | o | o := c new. { o find: #(1 2 3 4) do: [ :y | y = 3 ]. o n. o find: #(5) do: [ :y | false ]. o n } ].
c compile: 'dead ^ [ ^ 3 ]'.
say value: 'return to a dead context' value: [ [ c new dead value ] on: BlockCannotReturn do: [ :e | #cannotReturn ] ].
c compile: 'early: x x ifTrue: [ ^ 1 ]. [ x ifFalse: [ ^ 2 ] ] value. ^ 3'.
say value: 'returns' value: [ { c new early: true. c new early: false. [ :x | x ifTrue: [ 1 ] ] value: false } ].
say value: 'recursion' value: [ | fib | fib := nil. fib := [ :x | x < 2 ifTrue: [ x ] ifFalse: [ (fib value: x - 1) + (fib value: x - 2) ] ]. fib value: 20 ].
deep := [ :r :m | m = 0 ifTrue: [ 0 ] ifFalse: [ 1 + (r value: r value: m - 1) ] ].
say value: 'deep recursion' value: [ #( 10 1000 50000 200000 ) collect: [ :m | deep value: deep value: m ] ].
c compile: 'down: k ^ k = 0 ifTrue: [ #bottom ] ifFalse: [ self down: k - 1 ]'. c compile: 'sum: k ^ k = 0 ifTrue: [ 0 ] ifFalse: [ k + (self sum: k - 1) ]'.
say value: 'deep sends' value: [ { c new down: 100000. c new sum: 100000 } ].

"---- contexts"
c compile: 'context | t | t := 5. ^ { thisContext tempNamed: #t. thisContext receiver == self. thisContext selector. thisContext method numArgs. [ self. thisContext outerContext selector ] value }'.
say value: 'thisContext' value: [ c new context ].
say value: 'a context in a block' value: [ { thisContext sender class name. thisContext receiver class name. thisContext method class name. thisContext pc class name. thisContext stackPtr class name. [ :x | thisContext method numArgs ] value: 3 } ].
say value: 'a temporary of a context' value: [ | t ctx | t := 5. ctx := thisContext. ctx tempNamed: #t put: 6. { t. ctx tempNamed: #t } ].
say value: 'tempAt:' value: [ (1 to: 5) collect: [ :i | thisContext tempAt: 1 ] ].
c compile: 'leave: aBlock | k | k := 1. aBlock value: thisContext. k := k + 1. ^ k'.
say value: 'return through a context' value: [ { c new leave: [ :ctx | ctx ]. c new leave: [ :ctx | ctx return: 7 ]. c new leave: [ :ctx | ctx tempAt: 2 put: 10 ] } ].
say value: 'a stack of contexts' value: [ | ctx k | ctx := thisContext. k := 0. [ ctx isNil ] whileFalse: [ k := k + 1. ctx := ctx sender ]. k > 5 ].
say value: 'contexts kept' value: [ | l | l := OrderedCollection new. 1 to: 1000 do: [ :i | l add: ([ :x | thisContext ] value: i) ]. l inject: 0 into: [ :x :y | x + (y tempAt: 1) ] ].
say value: 'pc of a context' value: [ | ctx p1 p2 | ctx := thisContext. p1 := ctx pc. p2 := ctx pc. { p2 > p1. ctx stackPtr >= 0. ctx sender isNil } ].
say value: 'another sender' value: [ | result | result := [ :k | thisContext swapSender: thisContext sender sender. k ] value: 3. result ].

"---- exceptions"
say value: 'exceptions' value: [ | k | k := 0. { [ 1 / 0 ] on: ZeroDivide do: [ :e | e return: 5 ]. [ (Warning signal: 'x') ifNil: [ 1 ] ] on: Warning do: [ :e | e resume: 7 ]. [ [ Error signal ] ensure: [ k := 1 ] ] on: Error do: [ :e | e retryUsing: [ k ] ]. [ [ Error signal. 5 ] on: Error do: [ :e | e pass ] ] on: Error do: [ :e | e return: 9 ]. [ [ 3 ] ifCurtailed: [ k := 2 ] ] value. k } ].
say value: 'exceptions in a loop' value: [ | k | k := 0. 1 to: 2000 do: [ :i | k := k + ([ i \\ 3 = 0 ifTrue: [ Error signal ]. 1 ] on: Error do: [ :e | e return: 2 ]) ]. k ].
say value: 'retry' value: [ | k | k := 0. [ k := k + 1. k < 5 ifTrue: [ Error signal ]. k ] on: Error do: [ :e | e retry ] ].
say value: 'ensure: runs' value: [ | l | l := OrderedCollection new. [ [ [ l add: 1. Error signal. l add: #never ] ensure: [ l add: 2 ] ] on: Error do: [ :e | l add: 3 ] ] ensure: [ l add: 4 ]. l asArray ].
say value: 'outer and signal in a handler' value: [ [ [ Warning signal. 5 ] on: Warning do: [ :e | e outer + 1 ] ] on: Warning do: [ :e | e resume: 10 ] ].

"---- processes"
say value: 'a queue of processes' value: [ | q r | r := OrderedCollection new. q := SharedQueue new. 1 to: 5 do: [ :i | [ 1 to: 200 do: [ :k | q nextPut: i * k. k \\ 10 = 0 ifTrue: [ Processor yield ] ] ] forkAt: Processor userSchedulingPriority - 1 ]. 1000 timesRepeat: [ r add: q next ]. r inject: 0 into: [ :x :y | x + y ] ].
say value: 'a loop that is preempted' value: [ | x process | x := 0. process := [ [ true ] whileTrue: [ x := x + 1 ] ] forkAt: Processor userSchedulingPriority - 1. (Delay forMilliseconds: 50) wait. process terminate. x > 100 ].
say value: 'a loop of sends that is preempted' value: [ | process done | done := Semaphore new. process := [ [ true ] whileTrue: [ c new id ] ] fork. [ (Delay forMilliseconds: 30) wait. process terminate. done signal ] forkAt: Processor userInterruptPriority. done wait. #ok ].
say value: 'a recursion that is preempted' value: [ | process done f | f := [ :k | k = 0 ifTrue: [ 0 ] ifFalse: [ f value: k - 1 ] ]. done := Semaphore new. process := [ [ true ] whileTrue: [ f value: 50 ] ] fork. [ (Delay forMilliseconds: 30) wait. process terminate. done signal ] forkAt: Processor userInterruptPriority. done wait. #ok ].
say value: 'an empty loop that is preempted' value: [ | process done | done := Semaphore new. process := [ [ true ] whileTrue ] fork. [ (Delay forMilliseconds: 30) wait. process terminate. done signal ] forkAt: Processor userInterruptPriority. done wait. #ok ].
say value: 'semaphores' value: [ | one other k | one := Semaphore new. other := Semaphore new. k := 0. [ 5000 timesRepeat: [ one wait. k := k + 1. other signal ] ] fork. 5000 timesRepeat: [ one signal. other wait ]. k ].
say value: 'mutexes' value: [ | m l done | m := Mutex new. l := OrderedCollection new. done := Semaphore new. 1 to: 3 do: [ :i | [ 20 timesRepeat: [ m critical: [ l add: i. Processor yield. l add: i ] ]. done signal ] fork ]. 3 timesRepeat: [ done wait ]. (1 to: l size by: 2) allSatisfy: [ :i | (l at: i) = (l at: i + 1) ] ].
say value: 'a process that is suspended' value: [ | process k | k := 0. process := [ k := 1. Processor activeProcess suspend. k := 2 ] fork. Processor yield. { k. process resume. Processor yield. k } in: [ :r | { r first. k } ] ].
say value: 'process variables and termination' value: [ | l process | l := OrderedCollection new. process := [ [ l add: 1. (Delay forSeconds: 100) wait ] ensure: [ l add: 2 ] ] fork. Processor yield. process terminate. l asArray ].

"---- more of the image"
say value: 'sorts' value: [ | r | r := Random seed: 42. (((1 to: 5000) collect: [ :i | r nextInteger: 1000000 ]) asSortedCollection: [ :x :y | x > y ]) isSorted ].
say value: 'strings' value: [ (String streamContents: [ :w | 1 to: 1000 do: [ :i | w print: i; nextPut: $  ] ]) size ].
say value: 'dictionaries' value: [ | d | d := Dictionary new. 1 to: 8000 do: [ :i | d at: 'key' , (i \\ 500) printString put: ((d at: 'key' , (i \\ 500) printString ifAbsent: [ 0 ]) + 1) ]. { d size. (d at: 'key7'). d values sum } ].
say value: 'symbols' value: [ ((1 to: 2000) collect: [ :i | ('interp' , i printString) asSymbol ]) asSet size ].
say value: 'compiled in a loop' value: [ (1 to: 30) collect: [ :i | Smalltalk compiler evaluate: i printString , ' * 2 + ([ :x | x + 1 ] value: 3)' ] ].
out value: 'end'.
Smalltalk exitSuccess!
