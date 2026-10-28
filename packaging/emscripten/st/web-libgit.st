"Give LGitConfig the free function of Pharo 12, git_config_free, in an image that frees its
 configurations with git_object_free.  It is the one copy of that patch: the Console and the
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

 It answers nothing, and says nothing.  An error goes to the one that evaluates it: the boots of
 the Console and of the notebook report it on stderr, and start anyway; the set up of an image
 for the world fails with it."

| config web method protocols |
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
nil
