"git-http-clone.st - Iceberg clones over the smart-HTTP transport of the VM
(tests/wasm/lanes/66-libgit2.sh)

Clones with Iceberg (IceRepositoryCreator, the image's own LGit) from the
git server of the lane, tests/wasm/lib/git-http.mjs in its redirect mode,
through the smart-HTTP transport of src/emscripten/gitSupport.c, which the
node VM makes with curl, into a directory git-http next to the image, which
it empties first, and prints a line each, '<key>: <value>':

	redirect clone  a clone of WASM_GIT_HTTP_REMOTE, a repository named
	                without its .git, which the server redirects to its .git
	                for the advertisement of its refs, and refuses (422) for
	                the pack: its branch, which the clone checked out
	redirect log    the comments of the commits of that branch
	missing clone   a clone of WASM_GIT_HTTP_MISSING, a repository that is
	                not there (404): what it raised
	after           3 + 4, which shows that the VM goes on

A step prints its value or, when it raised an error, 'error <class of the
error>: <its messageText>', on one line.

Environment:

	WASM_GIT_HTTP_REMOTE   the URL of the repository named without .git
	WASM_GIT_HTTP_MISSING  the URL of a repository that is not there

It changes the image (the repositories of Iceberg) but does not save it:
run it with st --quit,

	pharo --headless Pharo.image --no-default-preferences st --quit git-http-clone.st"

| out line env root clone |
out := WriteStream on: String new.
line := [ :key :block |
	| value |
	value := [ block value ] on: Error do: [ :error |
		('error ' , error class name , ': ' , error messageText asString)
			collect: [ :character | character isSeparator ifTrue: [ $  ] ifFalse: [ character ] ] ].
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf ].
env := OSEnvironment current.
root := Smalltalk imageDirectory / 'git-http'.
root ensureDeleteAll.
root ensureCreateDirectory.
line value: 'redirect clone' value: [
	clone := IceRepositoryCreator new
		remote: (IceGitRemote url: (env at: 'WASM_GIT_HTTP_REMOTE'));
		location: root / 'redirect';
		createRepository.
	clone branchName ].
line value: 'redirect log' value: [ ' / ' join: (clone branch commits collect: [ :each | each comment trimBoth ]) ].
line value: 'missing clone' value: [
	(IceRepositoryCreator new
		remote: (IceGitRemote url: (env at: 'WASM_GIT_HTTP_MISSING'));
		location: root / 'missing';
		createRepository) branchName ].
line value: 'after' value: [ 3 + 4 ].
Stdio stdout nextPutAll: out contents; flush.
