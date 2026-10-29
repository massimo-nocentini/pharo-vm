"Give LGitConfig the free function of Pharo 12, git_config_free, in an image that frees its
 configurations with git_object_free (and the git_buf that git_config_find_global fills: see The
 path of the global configuration; and Iceberg a provider of credentials that opens no dialog: see
 The credentials of a push, below).  It is the one copy of that patch: the Console and the
 notebook kernel evaluate it at every boot (web-repl.st and web-notebook.st, from
 /pharo/st, where the page stages it next to them), whatever the image (the site's, one opened
 from a file, a saved one of any version of the site), and the set up of an image for the world
 does too (OSWebDriver class>>#setUpLibGit: at build time, prepare-web-image.st, and in the page,
 web-bootstrap.st), so that the world, which boots its image as it is, runs a patched one.  It
 is a plain expression, which they evaluate with the compiler, so it is not logged to the
 .changes itself.

 The LGitConfig of Pharo 13 and later has no free function of its own: it inherits #object_free:
 of LGitExternalObject, so the finalization of a configuration calls git_object_free on a
 git_config.  That breaks the configuration, which its repository keeps: git_object_free takes
 one from the pointer to its backends, where an object has its counter, and never releases it.
 The next call of libgit2 that reads it goes out of the memory of a web VM ('FFI callout
 trapped ... memory access out of bounds' in Firefox and the node VM; Chromium crashes the tab),
 and a native VM may crash.  Iceberg makes such a configuration when it asks for the user of a
 commit: the dialog of the missing properties (IceTipGitUsernameAndEmailDialogPresenter>>#accept,
 through LGitRepository>>#config), which every image shows once in the browser, since its HOME
 has no .gitconfig at first.  Pharo 12 frees with #config_free:, which calls git_config_free:
 compile its two methods, unchanged but for a comment.

 Only where LGitConfig still frees with #object_free:, so that an image that has its own free
 function (Pharo 12, a later Pharo that fixes it, an image patched before) or no LGit is left
 alone: at the boot of an image patched already, this compiles nothing, logs nothing to the
 .changes, and costs the test (in an image with OSWindow-Web whose LibGit-Core has a
 config_free: of its own, Pharo 12 say, a read of its source too).  And where an earlier
 evaluation put the two methods in LibGit-Core, before the image had OSWindow-Web: see below.

 Where the two methods go.  In an image that has the package OSWindow-Web (one prepared for the
 world), they are extensions of that package, as OSWebDriver class>>#setUpLibGit has made them
 since version 6 of the package: Monticello sees LibGit-Core as it loaded it, unmodified, a load
 of LibGit-Core does not remove them, and a LibGit-Core that defines them takes them over;
 prepare-web-image.st files out only the methods of the sources of the package, not these.  An
 image without OSWindow-Web, a stock one say, gets them in the protocols that Pharo 12 has them
 in, of LibGit-Core: an extension of OSWindow-Web there would make a package OSWindow-Web of
 these two methods only.  So that LibGit-Core is not modified for Monticello and Iceberg either,
 nor the session of Epicea, they are compiled without the announcements of the system; a load
 of another LibGit-Core then removes them, and the next boot of the Console or of the notebook
 compiles them again (the world does not evaluate this at its boots: its images have them as
 extensions).

 Such an image may then get OSWindow-Web: the page prepares a stock image for the world by
 booting it as the Console does (vm-worker.js), so this has compiled the two methods in
 LibGit-Core before web-bootstrap.st files the package in.  The set up for the world
 (#setUpLibGit) then finds config_free: compiled by this file (its comment says so) in a
 protocol of LibGit-Core, and compiles both again, as extensions of OSWindow-Web, which takes
 them out of LibGit-Core: once, at the preparation, which the page saves.  A config_free: of
 LibGit-Core itself, which says nothing of this file, is left alone.

 Their source goes to the .changes, once, when they are compiled (about 400 bytes; so once
 for each boot of an image that is never saved patched, a stock image kept in the browser
 and not saved since, say): a callout of uFFI needs it there.  At its first call uFFI
 replaces the method with the callout it generates, which keeps the pointer to the source
 of the method and nothing else: the source of a method compiled without a log (which the
 method keeps in its properties) would be lost, and the browsers would show none.

 The path of the global configuration.  libgit2 1.4.4 declares int git_config_find_global(git_buf
 *out): it fills the git_buf out, {char *ptr; size_t reserved; size_t size}, 24 bytes, whose ptr
 it allocates, and the caller disposes of it; GIT_ENOTFOUND (-3) when HOME has no .gitconfig.
 LibGit-Core of Pharo 12 to 15 declares out a String* (LGitConfig>>#config_find_global_path:),
 and LGitConfig>>#globalConfigurationPath gives it a zeroed String of 256 characters, which it
 answers: libgit2 writes the git_buf into its first 24 bytes, so it answers 256 characters of
 binary junk, never the path nor nil, and leaks the 256 bytes that ptr points to at each call
 (natively and in the web VMs alike; in the browser, HOME is /pharo/home, which has a .gitconfig
 once Iceberg's dialog of the missing properties wrote it).  A String of fewer than 24 bytes
 given to the callout itself is overrun, in the heap of the image, and one that is not zeroed has
 libgit2 write through the junk it takes for ptr: a native VM crashes, a web VM traps the callout.
 Nothing in Iceberg or LGit calls either method (Iceberg opens the global configuration with
 git_config_open_global, declared right): a user reaches them only by evaluating them.  So compile
 both again, the callout with the LGitBuf (24 bytes, char *ptr; size_t asize; size_t size, which
 1.4.4 lays out the same) that LGitConfig>>#getString:ifPresent:ifAbsent: gives to
 git_config_get_string_buf, an LGitBuf new being GIT_BUF_INIT, and globalConfigurationPath
 answering the path, or nil, and disposing of the buffer (LGitBuf>>#free, git_buf_free, which
 1.4.4 makes git_buf_dispose, and which frees nothing after GIT_ENOTFOUND).  The other callouts
 of LGit that take a git_buf* declare it an LGitBuf*, and no other one fills a git_buf.
 Only where the callout still declares git_config_find_global without LGitBuf, so that a
 LibGit-Core that fixes it, or an image patched before, is left alone (and then nothing is
 compiled or logged); and where an earlier evaluation put the two methods in LibGit-Core, before
 the image had OSWindow-Web.  They go where the two methods of LGitConfig above go, by the same
 rules: extensions of OSWindow-Web in an image that has it, which take them out of LibGit-Core
 (Monticello does not see LibGit-Core modified, but its snapshot then lacks them), else the
 protocols of LibGit-Core that the stock methods are in, without the announcements of the system,
 and their source goes to the .changes when they are compiled, as for the two methods above (about
 900 bytes; so once for each boot of an image that is never saved patched, a stock image kept in
 the browser and not saved since, say).  A load of LibGit-Core puts its own back, in either case:
 the next boot of the Console or of the notebook compiles them again, and the world, which does
 not evaluate this file at its boots, has the stock ones until its image is prepared again.

 The credentials of a push.  When a git host asks for credentials (a 401: github.com answers it
 to a push, and to a fetch or a clone of a private repository), the smart-HTTP transport of the
 web VM (src/emscripten/gitSupport.c) asks libgit2's credentials callback, which Iceberg answers
 with the provider of IceCredentialsProvider class>>#providerType: the user name and password (or
 token) that IceCredentialStore keeps for the host first, as on a native VM.  Iceberg's default
 provider, IceTipInteractiveCredentialsProvider, then opens its dialog inside that callback, and
 waits: in the Console and the notebook kernel, which have no world, nothing ever answers it, and
 the VM spins until the page is closed (in the world too: a web VM takes no input inside a
 callback).  So where the provider is still that one, make it IceNonInteractiveCredentialsProvider,
 which gives the stored credentials and else none: the push then fails with the message of the
 transport, which says to store them (IceCredentialStore current storeCredential: ...
 forHostname: ...).  A provider chosen otherwise is left alone: OSWebCredentialsProvider of the
 images of the world (OSWebDriver class>>#setUpGit, which runs before #setUpLibGit evaluates this
 file), which asks once the callout has returned, or one of the user's.  This compiles nothing.

 The conflicts of a pull.  When a pull (or a merge) has conflicts, IceMerge>>#resolveConflicts
 signals IceMergeResolutionRequest, whose default action opens Iceberg's dialog of the merge, a
 modal one (IceTipMergePresenter in Pharo 13 and later, of Iceberg-TipUI; IceTipMergeMorph in
 Pharo 12, of Iceberg), and waits for it: in the Console and the notebook kernel, which have no
 world, nothing ever answers it, and the pull never returns (the VM stays busy until the page is
 stopped).  So the default action is compiled again, once (its source says 'compiled by
 web-libgit.st', on one line), to answer false where Morphic's UIManager does not draw a world,
 after it signalled IceMergeAborted with what to do: the pull then fails at once, with the repository as it
 was but for what the fetch brought (the merge is made in the world, or by hand).  In a world it
 runs Iceberg's own, kept as #webWorldDefaultAction, its source unchanged but for its selector.
 Both in the protocol and the package of Iceberg's (an extension of Iceberg-TipUI, or Iceberg
 itself), without the announcements of the system, as for LGitConfig above, so that Monticello
 does not see that package modified; a load of it removes them, and the next boot compiles them
 again.  Only where the class is there and its default action is Iceberg's own (a unary method
 whose source starts with its selector and does not name web-libgit.st); their source goes to the
 .changes when they are compiled (about 800 bytes to 1 KB; so once for each boot of an image that
 is never saved patched, as above).

 Until the WebAssembly VMs of OSWindow-Web 8, that test looked for 'compiled by web-libgit.st',
 which the default action of this file did not say (it said 'Compiled by', then broke the line):
 each evaluation after the first compiled both again, and #webWorldDefaultAction became a copy of
 the default action of this file, which in a world sends #webWorldDefaultAction, itself, so that
 a merge with conflicts recursed without end in the world.  An image that was evaluated so twice
 and saved keeps it: one that the page prepared for the world from a stock image (its boot as the
 Console, then #setUpLibGit), or one that the Console saved after it booted an image saved patched
 already, say.  So where
 #webWorldDefaultAction sends itself, compile it again (in its protocol, without the announcements
 of the system, logged) from Iceberg's own default action, found through the .changes: there the
 source of a method is preceded by the pointer to the source that it replaced (prior:), so the
 versions of #webWorldDefaultAction, then those of #defaultAction, lead to the first one that does
 not name web-libgit.st, Iceberg's (copied to the .changes by the first evaluation, or in the
 .sources).  Where none is found (a .changes condensed since, say), it aborts the merge as
 without a world, rather than recurse.  Once: then it no longer sends itself, and a world image
 of an older version gets it at its preparation, which boots it as the Console does.

 It answers nothing, and says nothing.  An error goes to the one that evaluates it: the boots of
 the Console and of the notebook report it on stderr, and start anyway; the set up of an image
 for the world fails with it."

| config web method protocols found provider request original |
config := Smalltalk globals at: #LGitConfig ifAbsent: [ nil ].
web := PackageOrganizer default hasPackage: 'OSWindow-Web'.
(config notNil and: [
	config freeFunctionSelector = #object_free: or: [
		web and: [
			method := config class compiledMethodAt: #config_free: ifAbsent: [ nil ].
			method notNil
				and: [ method protocolName asString ~= '*OSWindow-Web'
				and: [ method sourceCode includesSubstring: 'compiled by web-libgit.st' ] ] ] ] ])
	ifTrue: [
		protocols := web
			ifTrue: [ #( '*OSWindow-Web' '*OSWindow-Web' ) ]
			ifFalse: [ #( 'libgit - calls' 'libgit - finalization' ) ].
		SystemAnnouncer uniqueInstance suspendAllWhile: [
			config class
				compile: 'config_free: handle
	"From Pharo 12, compiled by web-libgit.st of the WebAssembly VMs"

	^ self
		ffiCallSafely: #(void git_config_free #(void *handle))
		options: #()'
				classified: protocols first
				withStamp: ''
				notifying: nil
				logSource: true.
			config class
				compile: 'freeFunctionSelector
	"From Pharo 12, compiled by web-libgit.st of the WebAssembly VMs"

	^ #config_free:'
				classified: protocols last
				withStamp: ''
				notifying: nil
				logSource: true ] ].
found := config ifNotNil: [ config compiledMethodAt: #config_find_global_path: ifAbsent: [ nil ] ].
(found notNil and: [
	(Smalltalk globals includesKey: #LGitBuf) and: [
		((found sourceCode includesSubstring: 'git_config_find_global')
			and: [ (found sourceCode includesSubstring: 'LGitBuf') not ])
		or: [
			web and: [
				found protocolName asString ~= '*OSWindow-Web'
				and: [ found sourceCode includesSubstring: 'compiled by web-libgit.st' ] ] ] ] ])
	ifTrue: [
		protocols := web
			ifTrue: [ #( '*OSWindow-Web' '*OSWindow-Web' ) ]
			ifFalse: [ #( 'libgit - calls' 'accessing' ) ].
		SystemAnnouncer uniqueInstance suspendAllWhile: [
			config
				compile: 'config_find_global_path: out
	"int git_config_find_global(git_buf *out): libgit2 fills the git_buf out and allocates its ptr, which the caller disposes of (LGitBuf>>#free); GIT_ENOTFOUND (-3) when HOME has no .gitconfig. LibGit-Core declared out a String*, into whose bytes libgit2 wrote the git_buf: compiled by web-libgit.st of the WebAssembly VMs"

	^ self
		ffiCallSafely: #(int git_config_find_global #(LGitBuf *out))
		options: #()'
				classified: protocols first
				withStamp: ''
				notifying: nil
				logSource: true.
			config
				compile: 'globalConfigurationPath
	"The path of the global configuration of git, HOME/.gitconfig, or nil when there is none, compiled by web-libgit.st of the WebAssembly VMs"

	| buf |
	buf := LGitBuf new.
	^ [
	  (self config_find_global_path: buf) = 0
		  ifTrue: [ buf prim_ptr utf8StringFromCString ]
		  ifFalse: [ nil ] ] ensure: [ buf free ]'
				classified: protocols last
				withStamp: ''
				notifying: nil
				logSource: true ] ].
provider := Smalltalk globals at: #IceCredentialsProvider ifAbsent: [ nil ].
(provider notNil and: [ provider providerType name = #IceTipInteractiveCredentialsProvider ]) ifTrue: [
	provider providerType: (Smalltalk globals at: #IceNonInteractiveCredentialsProvider) ].
request := Smalltalk globals at: #IceMergeResolutionRequest ifAbsent: [ nil ].
original := request ifNotNil: [ request compiledMethodAt: #defaultAction ifAbsent: [ nil ] ].
(original notNil and: [
	(original sourceCode includesSubstring: 'web-libgit.st') not
		and: [ original sourceCode beginsWith: 'defaultAction' ] ]) ifTrue: [
	SystemAnnouncer uniqueInstance suspendAllWhile: [
		request
			compile: 'webWorldDefaultAction' , (original sourceCode allButFirst: 'defaultAction' size)
			classified: original protocolName
			withStamp: ''
			notifying: nil
			logSource: true.
		request
			compile: 'defaultAction
	"Iceberg''s dialog of the merge (#webWorldDefaultAction) in a world; without one (the Console,
	 the notebook kernel of a web page), where nothing could answer it, abort the merge:
	 compiled by web-libgit.st of the WebAssembly VMs"

	(UIManager default isKindOf: MorphicUIManager) ifTrue: [ ^ self webWorldDefaultAction ].
	IceMergeAborted new signal: self messageText ,
		'': the merge has conflicts, which Iceberg resolves in a dialog that only the world shows (world.html): pull there, or merge by hand; nothing was merged''.
	^ false'
			classified: original protocolName
			withStamp: ''
			notifying: nil
			logSource: true ] ].
method := request ifNotNil: [ request compiledMethodAt: #webWorldDefaultAction ifAbsent: [ nil ] ].
(method notNil and: [ method sourceCode includesSubstring: 'self webWorldDefaultAction' ]) ifTrue: [
	| source |
	SourceFiles flushChangesFile.
	{ method. request compiledMethodAt: #defaultAction ifAbsent: [ nil ] } do: [ :start |
		| pointer steps |
		pointer := (start ifNotNil: [ start sourcePointer ]) ifNil: [ 0 ].
		steps := 0.
		[ source isNil and: [ pointer > 0 and: [ (steps := steps + 1) <= 100 ] ] ] whileTrue: [
			| text preamble index |
			text := [ SourceFiles sourceCodeAt: pointer ] on: Error do: [ :error | nil ].
			(text notNil and: [ (text includesSubstring: 'web-libgit.st') not ]) ifTrue: [
				#( 'webWorldDefaultAction' 'defaultAction' ) do: [ :selector |
					(source isNil and: [ text beginsWith: selector ]) ifTrue: [
						source := 'webWorldDefaultAction' , (text allButFirst: selector size) ] ] ].
			"the preamble of that source, '<class> methodsFor: <protocol> ... prior: <pointer>', read
			 as ChangeRecord readers of Pharo do"
			preamble := [
				SourceFiles readOnlyDo: [ :files |
					(files fileAt: (SourceFiles fileIndexFromSourcePointer: pointer) ifAbsent: [ nil ])
						ifNotNil: [ :file |
							| position chunk |
							position := SourceFiles filePositionFromSourcePointer: pointer.
							file position: (0 max: position - 300).
							[ file position < (position - 1) ] whileTrue: [
								chunk := (ChunkReadStream on: file) next ].
							chunk ] ] ] on: Error do: [ :error | nil ].
			index := preamble ifNil: [ 0 ] ifNotNil: [ preamble indexOfSubCollection: 'prior: ' ].
			pointer := index = 0
				ifTrue: [ 0 ]
				ifFalse: [ ((preamble allButFirst: index + 6) readStream upTo: $ ) asInteger ifNil: [ 0 ] ] ] ].
	SystemAnnouncer uniqueInstance suspendAllWhile: [
		request
			compile: (source ifNil: [ 'webWorldDefaultAction
	"Iceberg''s dialog of the merge, which an earlier web-libgit.st made a call of this method itself,
	 and whose source is lost: abort the merge, as without a world; compiled by web-libgit.st of the
	 WebAssembly VMs"

	IceMergeAborted new signal: self messageText ,
		'': the merge has conflicts, and this image lost the dialog of the merge of Iceberg: merge by hand; nothing was merged''.
	^ false' ])
			classified: method protocolName
			withStamp: ''
			notifying: nil
			logSource: true ] ].
nil
