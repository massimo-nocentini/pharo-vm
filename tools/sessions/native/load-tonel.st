"load-tonel.st - loads packages of a local Tonel directory over the image with Monticello,
in the order given, without Metacello (so no baseline and no dependency is fetched): for a
checkout whose packages are already loaded from GitHub, to try its working tree.

	TONEL_DIR=<repo>/src TONEL_PACKAGES='Pkg-A Pkg-B' SAVE=1 run.sh dev.image load-tonel.st

It prints 'loaded <package>' or 'FAILED <package>: <error>' for each, then the new
Undeclared variables."
| out env repo undeclared |
out := Stdio stdout.
env := OSEnvironment current.
repo := TonelRepository new directory: (env at: 'TONEL_DIR') asFileReference.
undeclared := Undeclared keys asSet.
((env at: 'TONEL_PACKAGES') substrings: ' ') do: [ :name |
	[ (repo versionWithInfo: (repo versionInfoFromVersionNamed: name)) load.
	  out nextPutAll: 'loaded ', name ]
		on: Error
		do: [ :e |
			[ (repo versionFromFileNamed: name) load.
			  out nextPutAll: 'loaded ', name ]
				on: Error
				do: [ :e2 | out nextPutAll: 'FAILED ', name, ': ', e description, ' / ', e2 description ] ].
	out lf; flush ].
(Undeclared keys reject: [ :k | undeclared includes: k ]) do: [ :k | out nextPutAll: 'UNDECLARED ', k; lf ].
out flush.
