"git-identity.st - the first commit of a user that git does not know yet, as
Iceberg's dialog of the missing properties goes, then a full garbage
collection, then more commits (tests/wasm/lanes/66-libgit2.sh)

The browser's HOME has no .gitconfig at first, so the first commit of
Iceberg there fails to find the user (IceGitUsernameOrEmailNotFound), and
Iceberg shows its dialog, 'Please provide Git missing properties'
(IceTipInteractiveErrorVisitor>>#visitMissingCredentialsError:).  Its Ok
(IceTipGitUsernameAndEmailDialogPresenter>>#accept) sets the user through an
LGitConfig of the repository (IceLibgitRepository>>#getConfigurationGlobal:)
and resumes the commit.  The finalization of that LGitConfig, at a later
garbage collection, must not break the configuration that the repository keeps:
on an image whose LGitConfig frees with #object_free: (Pharo 13 and later,
before packaging/emscripten/st/web-libgit.st gives it the free function of Pharo
12), the next call of libgit2 that reads it fails.

Run it with a HOME of its own, empty: it goes through the dialog itself, made
as the error visitor makes it but not opened, filled in with the user Wasm
Dialog, dialog@example.org, 'Save properties as global?' checked, which writes
HOME/.gitconfig.  In a directory identity next to the image, which it empties
first, it prints a line each, '<key>: <value>':

	major           the major version of Pharo
	gitconfig before what HOME/.gitconfig says of the user before: none
	free function   LGitConfig freeFunctionSelector: config_free: once
	                web-libgit.st ran in the image (the set up for the
	                world, or a boot of the Console or of the notebook
	                kernel; Pharo 12 has it too), object_free: on a stock
	                Pharo 13 or later
	free calls      the C functions that the method of that free function
	                names in its callout: git_config_free once the image is
	                set up (a free function that is missing, or that calls
	                nothing, fails here)
	set up again    OSWebDriver class>>#setUpLibGit once more, when the
	                image has it, which evaluates st/web-libgit.st of the
	                directory of the image; else that file, when it is there,
	                as a boot of the Console does: unchanged when the method
	                of the free function is the same afterwards, as it must
	                be (web-libgit.st compiles only where LGitConfig frees
	                with object_free:); unchanged too with neither
	create repo     a new repository, identity/demo, its code in src: its name
	make class      a class WasmIdentityDemo, in a package WasmIdentityDemo
	add package     that package added to the repository: its packages
	commit 1        a commit of it, through the dialog: its short id, which
	                changes with the time
	dialogs 1       how many times the commit asked for the user: 1
	gc              a full garbage collection, twice, and time for the
	                finalization process: what is left of the LGitConfig
	                objects that the dialog made, 0
	gitconfig       what HOME/.gitconfig says of the user: the dialog's,
	                'Wasm Dialog <dialog@example.org>'
	signature       the user of the next commit (git_signature_default),
	                which reads the configuration of the repository again
	modify          a method more: the size of the diff of the working copy
	commit 2        a commit of that: its short id
	commit 3        and one more
	dialogs         how many times the commits asked for the user in all: 1
	log             the comments and authors of the commits of the branch,
	                newest first
	after           3 + 4, which shows that the VM goes on
	errors          the steps that raised an error, or none

A step prints its value or, when it raised an error, 'error <class of the
error>: <its messageText>', and ' (in <class>>><selector>)' with the
innermost method of the packages LibGit-* on the stack of the error when
there is one (the callout that failed, for a PrimitiveFailed).  Every step
runs, also after one failed.  Each line is printed when its step ends, so
that the lines before stay when the image quits on an error that nothing
handles (one of the finalization process, a free function that is missing
say).

It changes the image (a class, a package, the repositories of Iceberg) but
does not save it: run it with st --quit,

	HOME=<empty dir> pharo --headless Pharo.image --no-default-preferences st --quit git-identity.st

or evaluate it in the REPL of the Console or in a cell of the notebook kernel,
which lane 66 does too:

	Smalltalk compiler evaluate: '<path>/git-identity.st' asFileReference contents"

| out describe line step failed root dir repo cls dialogs commit user home |
out := Stdio stdout.
describe := [ :error |
	| context where |
	context := error signalerContext.
	[ context notNil and: [
		(context homeMethod methodClass instanceSide package name beginsWith: 'LibGit') not ] ]
			whileTrue: [ context := context sender ].
	where := context
		ifNil: [ '' ]
		ifNotNil: [ ' (in ' , context homeMethod methodClass name , '>>' , context homeMethod selector , ')' ].
	('error ' , error class name , ': ' , error messageText asString , where)
		collect: [ :character | character isSeparator ifTrue: [ $  ] ifFalse: [ character ] ] ].
line := [ :key :value |
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf; flush ].
failed := OrderedCollection new.
"(described in the handler: once it returns, the stack of the error is gone)"
step := [ :key :block |
	line value: key value: ([ block value ] on: Error do: [ :error |
		failed add: key.
		describe value: error ]) ].
"the user that HOME/.gitconfig names, as 'name <email>', or none"
home := FileLocator home / '.gitconfig'.
user := [
	home exists
		ifFalse: [ 'none' ]
		ifTrue: [
			| lines value |
			lines := home contents lines collect: [ :each | each trimBoth ].
			value := [ :name |
				(lines detect: [ :each | each beginsWith: name , ' =' ] ifNone: [ name , ' = ?' ])
					allButFirst: name size + 2 ].
			(value value: 'name') trimBoth , ' <' , (value value: 'email') trimBoth , '>' ] ].
"a commit, which answers its short id: when it asks for the user, the dialog
 answers, as IceTipInteractiveErrorVisitor>>#visitMissingCredentialsError: has
 it answer, its Ok resuming the commit"
dialogs := 0.
commit := [ :message |
	([ repo commitWithMessage: message ]
		on: IceGitUsernameOrEmailNotFound
		do: [ :missing |
			| dialog |
			dialogs := dialogs + 1.
			dialog := (Smalltalk at: #IceTipGitUsernameAndEmailDialogPresenter)
				newApplication: SpApplication new
				model: repo.
			dialog asModalWindow.
			dialog nameInput text: 'Wasm Dialog'.
			dialog emailInput text: 'dialog@example.org'.
			dialog globalCheckbox state: true.
			dialog onAccept: [ missing resume ].
			dialog accept ]) shortId ].
root := Smalltalk imageDirectory / 'identity'.
dir := root / 'demo'.
root ensureDeleteAll.
root ensureCreateDirectory.
step value: 'major' value: [ SystemVersion current major ].
step value: 'gitconfig before' value: user.
step value: 'free function' value: [ LGitConfig freeFunctionSelector ].
step value: 'free calls' value: [
	| free |
	free := LGitConfig class lookupSelector: LGitConfig freeFunctionSelector.
	' ' join: ((free ast allChildren select: [ :node | node isLiteralNode and: [ node value isArray ] ])
		flatCollect: [ :node |
			(node value select: [ :each | each isSymbol and: [ each beginsWith: 'git_' ] ])
				collect: [ :each | each asString ] ]) ].
step value: 'set up again' value: [
	| free libgit |
	free := LGitConfig class lookupSelector: LGitConfig freeFunctionSelector.
	libgit := Smalltalk imageDirectory / 'st' / 'web-libgit.st'.
	(Smalltalk at: #OSWebDriver ifAbsent: [ nil ])
		ifNotNil: [ :driver | driver setUpLibGit ]
		ifNil: [ libgit exists ifTrue: [ Smalltalk compiler evaluate: libgit contents ] ].
	(LGitConfig class lookupSelector: LGitConfig freeFunctionSelector) == free
		ifTrue: [ 'unchanged' ]
		ifFalse: [ 'compiled again' ] ].
step value: 'create repo' value: [
	repo := IceRepositoryCreator new
		location: dir;
		subdirectory: 'src';
		createNewRepositoryNamed: 'wasmidentity'.
	repo register.
	repo name ].
step value: 'make class' value: [
	cls := (Object << #WasmIdentityDemo) package: 'WasmIdentityDemo'; install.
	cls compile: 'one ^ 1'.
	cls name ].
step value: 'add package' value: [
	repo workingCopy addPackageNamed: 'WasmIdentityDemo'.
	repo workingCopy packageNames asArray ].
step value: 'commit 1' value: [ commit value: 'first' ].
step value: 'dialogs 1' value: [ dialogs ].
step value: 'gc' value: [
	Smalltalk garbageCollect.
	(Delay forMilliseconds: 300) wait.
	Smalltalk garbageCollect.
	(Delay forMilliseconds: 300) wait.
	LGitConfig allInstances size ].
step value: 'gitconfig' value: user.
step value: 'signature' value: [
	| signature |
	signature := repo repositoryHandle defaultSignature.
	signature name , ' <' , signature email , '>' ].
step value: 'modify' value: [
	cls compile: 'two ^ 2'.
	repo workingCopyDiff codeSubdirectoryNode allChildren size ].
step value: 'commit 2' value: [ commit value: 'second' ].
step value: 'commit 3' value: [
	cls compile: 'three ^ 3'.
	commit value: 'third' ].
step value: 'dialogs' value: [ dialogs ].
step value: 'log' value: [
	' / ' join: (repo branch commits collect: [ :each | each comment , ' (' , each author , ')' ]) ].
step value: 'after' value: [ 3 + 4 ].
line value: 'errors' value: (failed isEmpty
	ifTrue: [ 'none' ]
	ifFalse: [ (failed asArray collect: [ :each | each asString ]) printString ]).
