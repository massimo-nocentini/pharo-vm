"Run the SimdUTF benchmark (SUBench.st) in an image prepared by prepare-image.st, with the
 settings of the environment, and exit with 1 when it printed an ERROR or a CHECK DIFFERENT line:

	pharo --headless subench.image --no-default-preferences st --quit run-bench.st"

Smalltalk exit: ((Smalltalk at: #SUBench) run = 0 ifTrue: [ 0 ] ifFalse: [ 1 ])
