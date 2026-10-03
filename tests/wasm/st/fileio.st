"S4 of wasm-smoke.sh: the files of the working directory, which the node CLI
 (NODERAWFS) shares with the host.  It reads from-host.txt, written by the
 shell, writes fileio/round-trip.txt in a new directory and reads it back, lists
 the working directory, and deletes a file.

	pharo --headless Pharo.image --no-default-preferences st --quit fileio.st

 prints

	from the host: written by the shell
	round trip: written by Pharo
	directory: true
	listing: true
	deleted: true

 and leaves fileio/round-trip.txt for the shell to read."

| here dir file scratch |
here := FileSystem workingDirectory.
dir := here / 'fileio'.
dir ensureCreateDirectory.
file := dir / 'round-trip.txt'.
file writeStreamDo: [ :stream | stream nextPutAll: 'written by Pharo' ].
scratch := dir / 'scratch.txt'.
scratch writeStreamDo: [ :stream | stream nextPutAll: 'deleted' ].
scratch delete.
Stdio stdout
	nextPutAll: 'from the host: ';
	nextPutAll: (here / 'from-host.txt') contents trimRight;
	lf;
	nextPutAll: 'round trip: ';
	nextPutAll: file contents;
	lf;
	nextPutAll: 'directory: ';
	print: (dir exists and: [ dir isDirectory ]);
	lf;
	nextPutAll: 'listing: ';
	print: (here children size > 0 and: [ dir children includes: file ]);
	lf;
	nextPutAll: 'deleted: ';
	print: scratch exists not;
	lf;
	flush.
