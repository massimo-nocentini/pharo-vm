"The image's own tests of integer arithmetic, which reach the plugin through the
 LargePositiveInteger and LargeNegativeInteger fallback code.  One line per class:
 '<class> <TestResult>', then '   bad: <test>' for each failure or error;
 'missing <class>' for a class this image does not have."!

| classes |
classes := #(#LargePositiveIntegerTest #LargeNegativeIntegerTest #IntegerTest #IntegerDigitLogicTest
	#SmallIntegerTest #NumberTest #FloatTest #FractionTest).
classes do: [:n | | r |
	(Smalltalk hasClassNamed: n)
		ifFalse: [Stdio stdout nextPutAll: 'missing ', n; lf]
		ifTrue:
			[r := (Smalltalk at: n) suite run.
			 Stdio stdout nextPutAll: n, ' ', r printString; lf.
			 (r failures, r errors) do: [:t | Stdio stdout nextPutAll: '   bad: ', t printString; lf]]].
Stdio stdout flush!
