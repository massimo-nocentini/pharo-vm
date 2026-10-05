"iceflow.st - Iceberg on a local repository, through the image's own bindings
of libgit2 (tests/wasm/lanes/66-libgit2.sh)

Goes through what a user of Iceberg does with a repository of their own,
with LGit, the bindings of the image (LGitLibrary: the library git2 of the
registry, found as the placeholder libgit2.so.1.4.4, on WebAssembly), in a
directory iceflow next to the image, which it empties first, and prints a
line each, '<key>: <value>':

	major           the major version of Pharo, which names the golden file
	                of the lane, tests/wasm/golden/iceflow-p<major>.txt
	backend         the class of FFIBackend current
	library         the name of the file that LGitLibrary found, without
	                its directory
	libgit2 version LGitLibrary uniqueInstance version, as 1.4.4
	create repo     a new repository, iceflow/demo, its code in src: its name
	make class      a class WasmIceDemo, in a package WasmIceDemo
	add package     that package added to the repository: its packages
	commit 1        a commit of it: its short id, which changes with the time
	modify          a method more: the size of the diff of the working copy
	commit 2        a commit of that: its short id
	log             the comments of the commits of the branch, newest first
	branch          a new branch, feature: its name
	status          how many packages are modified (none, after the commit)
	file clone      a clone of iceflow/demo into iceflow/clone, through a
	                file:// URL, which checks out its branch: its name

Then the census of these steps, once a full garbage collection and the
finalization process have freed the objects of LGit that they dropped
(git_object_free and the like: their methods would be compiled or not, as
the finalization went):

	ffi methods     how many methods of the packages LibGit-* are compiled
	                for their callouts (the property ffiNonCompiledMethod),
	                then each of them in a line 'ffi method: <class>>><selector>'
	callback classes the classes of the callbacks (FFICallback) that the
	                steps made for libgit2, separated by blanks: while they
	                run, FFICallback class>>basicNew keeps each callback,
	                which a garbage collection would otherwise take once it
	                is dropped (as the one of git_index_add_all is)

And then the steps that the census leaves out:

	clone log       the comments of the commits of the branch of the clone
	unresolved clone a clone of git://example.invalid/demo.git: what it
	                raised (the WebAssembly VMs resolve no name, and libgit2
	                is told that the name cannot be resolved)
	after           3 + 4, which shows that the VM goes on
	errors          the steps that raised an error, or none

A step prints its value or, when it raised an error, 'error <class of the
error>: <its messageText>', and ' (in <class>>><selector>)' with the
innermost method of the packages LibGit-* on the stack of the error when
there is one (the callout that failed, for a PrimitiveFailed).  Then 'ms
<step>: <ms>' says how long it took.  Any error is caught and printed as the
value of its line, so that every line is there.  A commit needs a user,
which libgit2 reads from the .gitconfig of HOME: the lane writes one.

Environment:

	WASM_ICEFLOW_STEPS  the steps to run, separated by commas: by default
	                    all of them but unresolved clone, 'all' for every
	                    one; e.g. 'libgit2 version,create repo,after' for the
	                    negative control of the registry, with git2 hidden

It changes the image (a class, a package, FFICallback class>>basicNew for a
while, the repositories of Iceberg) but does not save it: run it with st
--quit,

	pharo --headless Pharo.image --no-default-preferences st --quit iceflow.st"

| out describe line env wanted steps run failed root dir cloneDir repo clone cls made called callbacks |
out := WriteStream on: String new.
describe := [ :error |
	| context where |
	"the innermost method of LGit on the stack of the error, if any"
	context := error signalerContext.
	[ context notNil and: [
		(context homeMethod methodClass instanceSide package name beginsWith: 'LibGit') not ] ]
			whileTrue: [ context := context sender ].
	where := context
		ifNil: [ '' ]
		ifNotNil: [ ' (in ' , context homeMethod methodClass name , '>>' , context homeMethod selector , ')' ].
	('error ' , error class name , ': ' , error messageText asString , where)
		collect: [ :character | character isSeparator ifTrue: [ $  ] ifFalse: [ character ] ] ].
line := [ :key :block |
	| value |
	value := [ block value ] on: Error do: [ :error | describe value: error ].
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf ].
env := OSEnvironment current.
wanted := (env at: 'WASM_ICEFLOW_STEPS' ifAbsent: [ nil ]) ifNotNil: [ :names |
	((names substrings: ',') collect: [ :each | each trimBoth ]) asSet ].
failed := OrderedCollection new.
root := Smalltalk imageDirectory / 'iceflow'.
dir := root / 'demo'.
cloneDir := root / 'clone'.
root ensureDeleteAll.
root ensureCreateDirectory.
made := OrderedCollection new.
"each callback that the steps make, for the census"
Smalltalk at: #WasmIceflowCallbacks put: made.
FFICallback class
	compile: 'basicNew
	"iceflow.st (tests/wasm/st): keeps each callback, for its census"
	^ (Smalltalk at: #WasmIceflowCallbacks) add: super basicNew'
	classified: 'iceflow'.
line value: 'major' value: [ SystemVersion current major ].
line value: 'backend' value: [ FFIBackend current class name ].
steps := OrderedCollection new.
steps add: 'library' -> [ LGitLibrary uniqueInstance libraryName asFileReference basename ].
steps add: 'libgit2 version' -> [ '.' join: (LGitLibrary uniqueInstance version collect: [ :each | each printString ]) ].
steps add: 'create repo' -> [
	repo := IceRepositoryCreator new
		location: dir;
		subdirectory: 'src';
		createNewRepositoryNamed: 'wasmdemo'.
	repo register.
	repo name ].
steps add: 'make class' -> [
	cls := (Object << #WasmIceDemo) package: 'WasmIceDemo'; install.
	cls compile: 'one ^ 1'.
	cls name ].
steps add: 'add package' -> [
	repo workingCopy addPackageNamed: 'WasmIceDemo'.
	repo workingCopy packageNames asArray ].
steps add: 'commit 1' -> [ (repo commitWithMessage: 'first from Pharo') shortId ].
steps add: 'modify' -> [
	cls compile: 'two ^ 2'.
	repo workingCopyDiff codeSubdirectoryNode allChildren size ].
steps add: 'commit 2' -> [ (repo commitWithMessage: 'second') shortId ].
steps add: 'log' -> [ ' / ' join: (repo branch commits collect: [ :each | each comment ]) ].
steps add: 'branch' -> [ (repo createBranch: 'feature') name ].
steps add: 'status' -> [ repo workingCopy modifiedPackages size ].
steps add: 'file clone' -> [
	clone := IceRepositoryCreator new
		remote: (IceGitRemote url: 'file://' , dir fullName);
		location: cloneDir;
		createRepository.
	clone branchName ].
run := [ :list |
	list do: [ :step |
		(wanted isNil
			ifTrue: [ step key ~= 'unresolved clone' ]
			ifFalse: [ (wanted includes: 'all') or: [ wanted includes: step key ] ]) ifTrue: [
			| value ms |
			"(described in the handler: once it returns, the stack of the error is gone)"
			ms := [ value := [ step value value ] on: Error do: [ :error |
				failed add: step key.
				describe value: error ] ] timeToRun asMilliSeconds.
			line value: step key value: [ value ].
			line value: 'ms ' , step key value: [ ms ] ] ] ].
run value: steps.
"the census, once the finalization process, which a full garbage collection
signals, has freed what the steps dropped"
Smalltalk garbageCollect.
(Delay forMilliseconds: 200) wait.
FFICallback class removeSelector: #basicNew.
Smalltalk globals removeKey: #WasmIceflowCallbacks.
called := SortedCollection new.
(self packageOrganizer packages select: [ :package | package name beginsWith: 'LibGit' ]) do: [ :package |
	package definedClasses do: [ :class |
		(class methods , class class methods) do: [ :method |
			(method hasProperty: #ffiNonCompiledMethod) ifTrue: [
				called add: method methodClass name , '>>' , method selector ] ] ] ].
callbacks := (made collect: [ :each | each class name ]) asSet asSortedCollection.
line value: 'ffi methods' value: [ called size ].
called do: [ :each | line value: 'ffi method' value: [ each ] ].
line value: 'callback classes' value: [ ' ' join: callbacks ].
run value: {
	'clone log' -> [ ' / ' join: (clone branch commits collect: [ :each | each comment ]) ].
	'unresolved clone' -> [
		(IceRepositoryCreator new
			remote: (IceGitRemote url: 'git://example.invalid/demo.git');
			location: root / 'unresolved';
			createRepository) branchName ].
	'after' -> [ 3 + 4 ] }.
line value: 'errors' value: [
	failed isEmpty
		ifTrue: [ 'none' ]
		ifFalse: [ (failed asArray collect: [ :each | each asString ]) printString ] ].
Stdio stdout nextPutAll: out contents; flush.
