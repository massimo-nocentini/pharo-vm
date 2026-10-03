"Lane 22-old-space: the perm-space primitives fail.

	pharo --headless Pharo.image --no-default-preferences st --quit permspace.st

 WebAssembly memory cannot reach the address of perm space, so the
 primitives that move objects there, 90 (one object), 91 (the objects of an
 Array) and 93 (every old object), must fail with PrimErrNoMemory, rather
 than abort the VM, and leave the objects where they were.  Prints the
 error codes of the three and what primitive 92 (is the object in perm
 space?) answers for the object of 90 afterwards:
 #(#'insufficient object memory' #'insufficient object memory' #'insufficient object memory' false)."

| try object results |
try := [ :source :receiver |
	(OpalCompiler new compile: source)
		valueWithReceiver: receiver
		arguments: #() ].
object := Array new: 3.
results := OrderedCollection new.
results
	add: (try value: 'moveToPermSpace <primitive: 90 error: ec> ^ ec' value: object);
	add: (try value: 'moveToPermSpaceInBulk <primitive: 91 error: ec> ^ ec' value: (Array with: Object new with: 'x' copy));
	add: (try value: 'moveAllToPermSpace <primitive: 93 error: ec> ^ ec' value: nil);
	add: (try value: 'isInPermSpace <primitive: 92> ^ #failed' value: object).
Stdio stdout
	nextPutAll: results asArray printString;
	lf;
	flush.
