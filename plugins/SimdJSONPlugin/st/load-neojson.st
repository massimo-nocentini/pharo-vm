"load-neojson.st - load NeoJSON, with its tests, from a clone of its repository

NEOJSON_DIR names the clone of https://github.com/svenvc/NeoJSON (tested with
commit 11e274d): Metacello loads the groups core and tests of BaselineOfNeoJSON
from its tonel directory, NEOJSON_DIR/repository, with no network.  Run it with
--save, for SimdJSON-NeoTests.st and the benchmark, which compare SimdJSONReader
with NeoJSON (SimdJSON.st itself needs no NeoJSON):

	NEOJSON_DIR=/path/to/NeoJSON pharo --headless X.image --no-default-preferences st --save --quit load-neojson.st

Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| directory out |
out := Stdio stdout.
directory := (OSEnvironment current at: 'NEOJSON_DIR' ifAbsent: [ nil ])
	ifNil: [ self error: 'NEOJSON_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
(directory / 'repository') isDirectory
	ifFalse: [ self error: 'no repository directory in ' , directory fullName ].
[ Metacello new
	baseline: 'NeoJSON';
	repository: 'tonel://' , (directory / 'repository') fullName;
	load: #( 'core' 'tests' ) ]
	on: Warning
	do: [ :warning |
		out nextPutAll: 'NeoJSON: '; nextPutAll: warning messageText; lf.
		warning resume ].
out
	nextPutAll: 'NeoJSON: loaded ';
	nextPutAll: (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'Neo-JSON' ]) asSortedCollection asArray printString;
	lf;
	flush.
!
