"Run the SimdJSON benchmark (SJBench.st) in an image prepared by prepare-image.st, with the
 settings of the environment, and exit with 1 when it printed an ERROR line:

	pharo --headless sjbench.image --no-default-preferences st --quit run-bench.st"

Smalltalk exit: ((Smalltalk at: #SJBench) run = 0 ifTrue: [ 0 ] ifFalse: [ 1 ])
