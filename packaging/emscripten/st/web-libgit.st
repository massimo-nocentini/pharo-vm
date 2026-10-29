"Give LGitConfig the free function of Pharo 12, git_config_free, in an image that frees its
 configurations with git_object_free (and Iceberg a provider of credentials that opens no dialog:
 see The credentials of a push, below).  It is the one copy of that patch: the Console and the
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
 web-libgit.st'), to answer false where Morphic's UIManager does not draw a world, after it
 signalled IceMergeAborted with what to do: the pull then fails at once, with the repository as it
 was but for what the fetch brought (the merge is made in the world, or by hand).  In a world it
 runs Iceberg's own, kept as #webWorldDefaultAction, its source unchanged but for its selector.
 Both in the protocol and the package of Iceberg's (an extension of Iceberg-TipUI, or Iceberg
 itself), without the announcements of the system, as for LGitConfig above, so that Monticello
 does not see that package modified; a load of it removes them, and the next boot compiles them
 again.  Only where the class is there and its default action is Iceberg's own (a unary method
 whose source starts with its selector); their source goes to the .changes, once.

 It answers nothing, and says nothing.  An error goes to the one that evaluates it: the boots of
 the Console and of the notebook report it on stderr, and start anyway; the set up of an image
 for the world fails with it."

| config web method protocols provider request original |
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
provider := Smalltalk globals at: #IceCredentialsProvider ifAbsent: [ nil ].
(provider notNil and: [ provider providerType name = #IceTipInteractiveCredentialsProvider ]) ifTrue: [
	provider providerType: (Smalltalk globals at: #IceNonInteractiveCredentialsProvider) ].
request := Smalltalk globals at: #IceMergeResolutionRequest ifAbsent: [ nil ].
original := request ifNotNil: [ request compiledMethodAt: #defaultAction ifAbsent: [ nil ] ].
(original notNil and: [
	(original sourceCode includesSubstring: 'compiled by web-libgit.st') not
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
	 the notebook kernel of a web page), where nothing could answer it, abort the merge.  Compiled by
	 web-libgit.st of the WebAssembly VMs"

	(UIManager default isKindOf: MorphicUIManager) ifTrue: [ ^ self webWorldDefaultAction ].
	IceMergeAborted new signal: self messageText ,
		'': the merge has conflicts, which Iceberg resolves in a dialog that only the world shows (world.html): pull there, or merge by hand; nothing was merged''.
	^ false'
			classified: original protocolName
			withStamp: ''
			notifying: nil
			logSource: true ] ].
nil
