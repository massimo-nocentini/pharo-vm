"git-http-push.st - Iceberg pushes and pulls over the smart-HTTP transport of the VM, with credentials
(tests/wasm/lanes/66-libgit2.sh)

Clones with Iceberg (IceRepositoryCreator, the image's own LGit) the repository
WASM_GIT_PUSH_REMOTE of the git server of the lane, tests/wasm/lib/git-http.mjs with --push
and --auth USER:PASSWORD, which takes a push only with HTTP Basic credentials (and answers 401
without them, as github.com does), through the smart-HTTP transport of src/emscripten/gitSupport.c,
which the node VM makes with curl, into a directory git-http-push next to the image, which it
empties first; then pushes, and pulls what the lane commits on the server meanwhile.  It prints a
line each, '<key>: <value>':

	provider        the class of Iceberg's provider of credentials, which the boot gave the
	                image (web-libgit.st: IceNonInteractiveCredentialsProvider; the world's:
	                OSWebCredentialsProvider)
	clone           the branch of the clone, which needs no credentials
	commit 1        a commit of a package of its own, WasmPushDemo (a class, a method #one)
	push none       a push of it, with no credentials in IceCredentialStore: what it raised,
	                the 401 of the transport, which says that none was given
	push wrong      with a wrong password stored for the host: what it raised, the refusal of
	                the transport, with what the host said
	push stored     with the right one stored: ok
	commit 2        a second commit, a method #zero
	push url        with no credentials stored, a push to a second remote of the same
	                repository, whose URL has the user name and the password
	                (http://USER:PASSWORD@host/...): ok
	commit rounds   a commit of a method #rounds
	push rounds     with no credentials stored, and a provider of Iceberg's
	                (WasmPushRoundsProvider, a class of its own package) that gives a wrong
	                password at the first callback and the right one at the second: 'ok,
	                answers left 0' (two rounds of credentials for the same request)
	push reuse 1    with the right password stored, a commit of a method #reuse, pushed through
	                a git_remote of its own (LGitRemote, looked up once), as Iceberg pushes
	                (IceGitLocalBranch>>#pushBranchToRemote:gitRemote:progress:): ok
	push reuse 2    with nothing stored, another commit (#reuse answers 2), pushed through the
	                same git_remote: the 401 of none given (libgit2 closed the subtransport
	                at the end of the first push, which forgot its credentials; the error is
	                LGit's, which pushTo: would make an IceGenericError)
	push reuse 3    with the right password stored again, a push as Iceberg makes it: ok
	pull ff         once the lane committed a method #two on the server (the handshake below):
	                a pull, which fast-forwards
	ff head         the comment of the head and the number of its parents
	ff selectors    the methods of WasmPushDemo after the pull: #two is loaded
	ff modified     whether the working copy differs from the head after the pull
	commit 3        a local commit, a method #three
	push behind     once the lane committed a method #four on the server: a push, which
	                Iceberg refuses (IceRemoteDesynchronized)
	pull merge      a pull, which merges
	merge head      the comment of the head, and the number of its parents
	merge selectors the methods of WasmPushDemo after the merge
	merge modified  whether the working copy differs from the head after the merge
	push merge      a push of the merge: ok
	commit 4        a local commit that changes #one to answer 11
	pull conflict   once the lane committed #one answering 111 on the server: a pull, whose
	                merge conflicts: IceMergeAborted, from the default action of
	                IceMergeResolutionRequest that web-libgit.st compiles (no world here,
	                so no dialog of the merge, which nothing would answer)
	conflict head   the comment of the head, its number of parents, and what #one answers:
	                the local commit, as it was
	conflict incoming
	                the number of commits of the server that are not merged: 1
	pending         the hosts whose dialog of credentials OSWebCredentialsProvider would open
	                in a world, outside libgit2's callback (none in node, which has no world)
	after           3 + 4, which shows that the VM goes on

A step prints its value or, when it raised an error, 'error <class of the error>: <its
messageText>', on one line.  The handshake: before each pull, the script writes
WASM_GIT_SYNC/<name>.wait (remote-ff, remote-div, then remote-conflict), which the lane answers with a commit on
the server and WASM_GIT_SYNC/<name>.done; it waits for that file 60 s at most.

Environment:

	WASM_GIT_PUSH_REMOTE    the URL of the repository, without credentials
	WASM_GIT_PUSH_USER, WASM_GIT_PUSH_PASSWORD
	                        the credentials that the server takes
	WASM_GIT_SYNC           the directory of the handshake
	WASM_WEB_LIBGIT         if set, web-libgit.st, which the script evaluates first, as the
	                        Console and the notebook kernel do at their boot

It changes the image (the repositories of Iceberg, the store of credentials) but does not save
it: run it with st --quit,

	pharo --headless Pharo.image --no-default-preferences st --quit git-http-push.st"

| out line env root repo cls store remote user password host sync waitFor withUser provider gitRemote |
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
remote := env at: 'WASM_GIT_PUSH_REMOTE'.
user := env at: 'WASM_GIT_PUSH_USER'.
password := env at: 'WASM_GIT_PUSH_PASSWORD'.
sync := (env at: 'WASM_GIT_SYNC') asFileReference.
host := (IceGitRemote url: remote) host.
"(the credentials of the host in IceCredentialStore, or none)"
store := [ :secret |
	| credentials |
	credentials := IceCredentialStore current plaintextCredentialForHostname: host.
	credentials isPresent ifTrue: [ IceCredentialStore current removeCredential: credentials ].
	secret ifNotNil: [
		IceCredentialStore current
			storeCredential: (IcePlaintextCredentials new username: user; password: secret; yourself)
			forHostname: host ] ].
waitFor := [ :name |
	| done deadline |
	(sync / (name , '.wait')) writeStreamDo: [ :stream | stream nextPutAll: name ].
	done := sync / (name , '.done').
	deadline := Time millisecondClockValue + 60000.
	[ done exists or: [ Time millisecondClockValue > deadline ] ] whileFalse: [ (Delay forMilliseconds: 50) wait ].
	done exists ifFalse: [ self error: 'no ' , done basename , ' in 60 s' ] ].
root := Smalltalk imageDirectory / 'git-http-push'.
root ensureDeleteAll.
root ensureCreateDirectory.
line value: 'provider' value: [ IceCredentialsProvider providerType name ].
store value: nil.
line value: 'clone' value: [
	repo := IceRepositoryCreator new
		remote: (IceGitRemote url: remote);
		location: root / 'p';
		createRepository.
	repo register.
	repo branchName ].
line value: 'commit 1' value: [
	cls := (Object << #WasmPushDemo) package: 'WasmPushDemo'; install.
	cls compile: 'one ^ 1' classified: 'accessing'.
	repo workingCopy addPackageNamed: 'WasmPushDemo'.
	(repo commitWithMessage: 'pushed from wasm') comment ].
line value: 'push none' value: [ repo branch pushTo: repo origin. 'ok' ].
line value: 'push wrong' value: [
	store value: password , '-wrong'.
	repo branch pushTo: repo origin.
	'ok' ].
line value: 'push stored' value: [
	store value: password.
	repo branch pushTo: repo origin.
	'ok' ].
line value: 'commit 2' value: [
	cls compile: 'zero ^ 0' classified: 'accessing'.
	(repo commitWithMessage: 'second push') comment ].
line value: 'push url' value: [
	store value: nil.
	withUser := IceGitRemote
		name: 'withuser'
		url: (remote copyReplaceAll: '://' with: '://' , user , ':' , password , '@').
	repo addRemote: withUser.
	repo branch pushTo: withUser.
	'ok' ].
line value: 'commit rounds' value: [
	cls compile: 'rounds ^ 5' classified: 'accessing'.
	(repo commitWithMessage: 'rounds') comment ].
line value: 'push rounds' value: [
	store value: nil.
	provider := IceCredentialsProvider providerType.
	(IceNonInteractiveCredentialsProvider << #WasmPushRoundsProvider) package: 'WasmPushRounds'; install.
	(Smalltalk at: #WasmPushRoundsProvider)
		compile: 'obtainPlaintextCredentials: credentials
	| answers |
	answers := Smalltalk at: #WasmPushRoundsAnswers.
	answers isEmpty ifTrue: [ ^ LGitNoCredentialsProvided signal ].
	credentials readFrom: (IcePlaintextCredentials new
		username: (Smalltalk at: #WasmPushRoundsUser);
		password: answers removeFirst;
		yourself)'
		classified: 'providing'.
	Smalltalk at: #WasmPushRoundsUser put: user.
	Smalltalk at: #WasmPushRoundsAnswers put: (OrderedCollection with: password , '-wrong' with: password).
	IceCredentialsProvider providerType: (Smalltalk at: #WasmPushRoundsProvider).
	[ repo branch pushTo: repo origin ] ensure: [ IceCredentialsProvider providerType: provider ].
	'ok, answers left ' , (Smalltalk at: #WasmPushRoundsAnswers) size printString ].
line value: 'push reuse 1' value: [
	store value: password.
	cls compile: 'reuse ^ 1' classified: 'accessing'.
	repo commitWithMessage: 'reuse 1'.
	gitRemote := (LGitRemote of: repo repositoryHandle named: 'origin') lookup.
	repo branch pushBranchToRemote: repo origin gitRemote: gitRemote progress: IcePushTransferProgress new.
	'ok' ].
line value: 'push reuse 2' value: [
	store value: nil.
	cls compile: 'reuse ^ 2' classified: 'accessing'.
	repo commitWithMessage: 'reuse 2'.
	repo branch pushBranchToRemote: repo origin gitRemote: gitRemote progress: IcePushTransferProgress new.
	'ok' ].
store value: password.
line value: 'push reuse 3' value: [ repo branch pushTo: repo origin. 'ok' ].
line value: 'pull ff' value: [
	waitFor value: 'remote-ff'.
	repo pull.
	'ok' ].
line value: 'ff head' value: [ repo headCommit comment trimBoth , ', parents ' , repo headCommit ancestors size printString ].
line value: 'ff selectors' value: [ cls selectors asSortedCollection asArray ].
line value: 'ff modified' value: [ repo workingCopy isModified ].
line value: 'commit 3' value: [
	cls compile: 'three ^ 3' classified: 'accessing'.
	(repo commitWithMessage: 'local change') comment ].
line value: 'push behind' value: [
	waitFor value: 'remote-div'.
	repo branch pushTo: repo origin.
	'ok' ].
line value: 'pull merge' value: [ repo pull. 'ok' ].
line value: 'merge head' value: [
	(repo headCommit comment trimBoth copyUpTo: Character space) , ', parents ' , repo headCommit ancestors size printString ].
line value: 'merge selectors' value: [ cls selectors asSortedCollection asArray ].
line value: 'merge modified' value: [ repo workingCopy isModified ].
line value: 'push merge' value: [ repo branch pushTo: repo origin. 'ok' ].
line value: 'commit 4' value: [
	cls compile: 'one ^ 11' classified: 'accessing'.
	(repo commitWithMessage: 'local one') comment ].
line value: 'pull conflict' value: [
	waitFor value: 'remote-conflict'.
	repo pull.
	'ok' ].
line value: 'conflict head' value: [
	repo headCommit comment trimBoth , ', parents ' , repo headCommit ancestors size printString , ', one '
		, cls new one printString ].
line value: 'conflict incoming' value: [ (repo branch incomingCommitsFrom: repo origin) size ].
line value: 'pending' value: [
	(Smalltalk hasClassNamed: #OSWebCredentialsProvider)
		ifTrue: [ (Smalltalk at: #OSWebCredentialsProvider) pendingHosts ]
		ifFalse: [ #() ] ].
store value: nil.
line value: 'after' value: [ 3 + 4 ].
Stdio stdout nextPutAll: out contents; flush.
