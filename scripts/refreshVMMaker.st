"Reload the Slang sources of a pharo-vm tree into a VMMaker image that was built
from an older one, so that it generates the sources of this tree.  make wasm
runs it when a .st file of smalltalksrc changed after the last generation:

	pharo --headless VMMaker.image --no-default-preferences --save --quit refreshVMMaker.st <tree>

The packages are loaded already, from another repository, and possibly in
newer versions: take the ones of <tree> in every case."

| path |
path := CommandLineArguments new commandLineArguments last.

Iceberg remoteTypeSelector: #httpsUrl.

Metacello new
	baseline: 'VMMaker';
	repository: 'tonel://' , path , '/smalltalksrc';
	onConflictUseIncoming;
	onUpgrade: [ :ex | ex useIncoming ];
	onDowngrade: [ :ex | ex useIncoming ];
	load
