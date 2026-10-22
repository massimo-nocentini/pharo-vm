"Load the SimdJSONPlugin of the VM, as the CI jobs do with the VMMaker image:
prints 'SimdJSONPlugin <platform> <implementation>' and exits 0, else exits 1"

| info |
Object compile: 'sjCiInfo <primitive: ''primitiveInfo'' module: ''SimdJSONPlugin''> ^ nil' classified: 'probe'.
info := nil sjCiInfo.
Stdio stdout
	nextPutAll: (info
		ifNil: [ 'SimdJSONPlugin did not load' ]
		ifNotNil: [ 'SimdJSONPlugin ', (info at: 5), ' ', (info at: 4) ]);
	lf;
	flush.
Smalltalk exit: (info ifNil: [ 1 ] ifNotNil: [ 0 ])
