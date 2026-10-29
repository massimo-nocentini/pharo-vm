"git-http-hosts.st - where the credentials of a push may go, over the smart-HTTP transport of the VM
(tests/wasm/lanes/66-libgit2.sh)

For each URL of WASM_GIT_HOSTS_REMOTES (separated by spaces), clones the repository there with
Iceberg (IceRepositoryCreator, the image's own LGit) into a directory git-http-hosts/<n> next to
the image, which it empties first, commits a method of a package of its own (WasmHostsDemo<n>),
stores the credentials WASM_GIT_HOSTS_USER and WASM_GIT_HOSTS_PASSWORD in IceCredentialStore for
the host of that URL, as the user would, and pushes: the lane gives it remotes whose host
redirects the push to another host (tests/wasm/lib/git-http.mjs --mover), which must not get
those credentials, and a proxy over plain http:// on another computer (PHARO_WASM_GIT_PROXY),
through which they must not go.  It prints a line each, '<key>: <value>':

	clone <n>       the branch of the clone of the n-th remote (from 1)
	push <n>        'ok', or what the push raised
	after           3 + 4, which shows that the VM goes on

A step prints its value or, when it raised an error, 'error <class of the error>: <its
messageText>', on one line.

Environment:

	WASM_GIT_HOSTS_REMOTES  the URLs, without credentials
	WASM_GIT_HOSTS_USER, WASM_GIT_HOSTS_PASSWORD
	                        the credentials stored for their hosts
	WASM_WEB_LIBGIT         if set, web-libgit.st, which the script evaluates first, as the
	                        Console and the notebook kernel do at their boot

It changes the image (the repositories of Iceberg, the store of credentials) but does not save
it: run it with st --quit,

	pharo --headless Pharo.image --no-default-preferences st --quit git-http-hosts.st"

| out line env root user password |
out := WriteStream on: String new.
line := [ :key :block |
	| value |
	value := [ block value ] on: Error do: [ :error |
		('error ' , error class name , ': ' , error messageText asString)
			collect: [ :character | character isSeparator ifTrue: [ $  ] ifFalse: [ character ] ] ].
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf ].
env := OSEnvironment current.
(env at: 'WASM_WEB_LIBGIT' ifAbsent: [ '' ]) ifNotEmpty: [ :file |
	Smalltalk compiler evaluate: file asFileReference contents ].
user := env at: 'WASM_GIT_HOSTS_USER'.
password := env at: 'WASM_GIT_HOSTS_PASSWORD'.
root := Smalltalk imageDirectory / 'git-http-hosts'.
root ensureDeleteAll.
root ensureCreateDirectory.
((env at: 'WASM_GIT_HOSTS_REMOTES') substrings: ' ') doWithIndex: [ :remote :n |
	| repo cls host credentials |
	host := (IceGitRemote url: remote) host.
	line value: 'clone ' , n printString value: [
		repo := IceRepositoryCreator new
			remote: (IceGitRemote url: remote);
			location: root / n printString;
			createRepository.
		repo register.
		repo branchName ].
	line value: 'push ' , n printString value: [
		cls := (Object << ('WasmHostsDemo' , n printString) asSymbol) package: 'WasmHostsDemo' , n printString; install.
		cls compile: 'one ^ 1' classified: 'accessing'.
		repo workingCopy addPackageNamed: 'WasmHostsDemo' , n printString.
		repo commitWithMessage: 'hosts ' , n printString.
		credentials := IceCredentialStore current plaintextCredentialForHostname: host.
		credentials isPresent ifTrue: [ IceCredentialStore current removeCredential: credentials ].
		IceCredentialStore current
			storeCredential: (IcePlaintextCredentials new username: user; password: password; yourself)
			forHostname: host.
		[ repo branch pushTo: repo origin ] ensure: [
			IceCredentialStore current removeCredential:
				(IceCredentialStore current plaintextCredentialForHostname: host) ].
		'ok' ] ].
line value: 'after' value: [ 3 + 4 ].
Stdio stdout nextPutAll: out contents; flush.
