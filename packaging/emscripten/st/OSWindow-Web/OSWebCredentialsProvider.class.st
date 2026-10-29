"
I give Iceberg the credentials of a git host in the world of a web page: OSWebDriver class>>#setUpGit makes me IceCredentialsProvider's providerType.

When an http:// or https:// host asks for credentials (a 401: github.com answers it to a push, and to a fetch or a clone of a private repository), the smart-HTTP transport of the web VM (src/emscripten/gitSupport.c) asks libgit2's credentials callback, which Iceberg answers with a provider such as me (LGitCredentialsPlaintext class>>#createFrom:username:onto:): first the user name and password (or token) that IceCredentialStore keeps for the host, as Iceberg's own provider does. When it keeps none, or the host refused them, Iceberg's provider opens its dialog, IceTipAskForPlaintextCredentialsPresenter (IceTipAskGithubForPlaintextCredentialsPresenter, which asks for a token, for github.com), and waits for it. Inside the callback of a web VM that would hang the page: the VM gets no input and the page draws nothing until the callout of the clone, fetch, pull or push returns (the worker takes input and draws only between the slices of the VM, and a slice ends only outside callbacks).

So inside the callback I give none (LGitNoCredentialsProvided: the operation fails, and the transport says that the host asked for credentials, or refused those it got), and I ask once the callout has returned: the dialog opens in the next cycle of the world (#askLaterFor:). What the user gives there is stored in IceCredentialStore when its checkbox says so (it does by default), as Iceberg stores it, else kept for the session of the image and forgotten when the image is saved or quits (#forgetUnstored): either way the next push, pull or fetch takes it at its first try. Outside callbacks I am Iceberg's own provider (the Iceberg plugin of GitHub asks for credentials there), in a world; without one (the Console and the notebook kernel, whose web-libgit.st chooses IceNonInteractiveCredentialsProvider instead, or the node VM) I give none and ask nothing.

IceCredentialStore writes what it stores, in plain text, to FileLocator localDirectory / 'iceberg' / 'credentials.fuel', which in the browser is /pharo/pharo-local/iceberg/credentials.fuel: the page keeps it with the image when the image is saved in the browser, and puts it into a Download.
"
Class {
	#name : 'OSWebCredentialsProvider',
	#superclass : 'IceTipInteractiveCredentialsProvider',
	#classVars : [
		'Asking',
		'Unstored'
	],
	#category : 'OSWindow-Web',
	#package : 'OSWindow-Web'
}

{ #category : 'asking' }
OSWebCredentialsProvider class >> askFor: aHost [
	"Iceberg's dialog of the credentials of aHost, as IceTipInteractiveCredentialsProvider opens it, outside libgit2's callback: what the user gives is stored in IceCredentialStore when the checkbox of the dialog says so, else kept for this session (see #plaintextCredentials). Cancelled, it leaves everything as it was"

	| credentials stored |
	credentials := IcePlaintextCredentials new.
	[
	(self new
		 remote: (IceRemoteWithOnlyHostname for: aHost);
		 yourself) obtainPlaintextCredentials: credentials ]
		on: LGitNoCredentialsProvided
		do: [ :cancelled | ^ self ].
	credentials isPresent ifFalse: [ ^ self ].
	stored := IceCredentialStore current plaintextCredentialForHostname: aHost.
	(stored isPresent and: [
		 stored username = credentials username and: [ stored password = credentials password ] ])
		ifTrue: [ self unstored removeKey: aHost ifAbsent: [  ] ]
		ifFalse: [ self unstored at: aHost put: credentials ]
]

{ #category : 'asking' }
OSWebCredentialsProvider class >> askLaterFor: aHost [
	"Ask for the credentials of aHost in the next cycle of the world, once the callout that asked for them has returned (see the class comment); once, while that dialog is not answered. Without a world (a UIManager that is not Morphic's, which would evaluate the block at once) ask nothing"

	(UIManager default isKindOf: MorphicUIManager) ifFalse: [ ^ self ].
	Asking ifNil: [ Asking := Set new ].
	(Asking includes: aHost) ifTrue: [ ^ self ].
	Asking add: aHost.
	UIManager default defer: [
		[ self askFor: aHost ] ensure: [ Asking remove: aHost ifAbsent: [  ] ] ]
]

{ #category : 'testing' }
OSWebCredentialsProvider class >> canAsk [
	"Whether a dialog can be shown: in a world, which Morphic's UIManager draws"

	^ UIManager default isKindOf: MorphicUIManager
]

{ #category : 'asking' }
OSWebCredentialsProvider class >> forgetUnstored [
	"Forget the credentials given for this session only (OSWebDriver class>>#shutDown: calls me before every save of the image, and before it quits): they never go into a saved image"

	Unstored := nil
]

{ #category : 'testing' }
OSWebCredentialsProvider class >> isDefault [
	"Not Iceberg's default, which IceCredentialsProvider class>>#defaultCredentialProviderClass finds among the subclasses of IceCredentialsProvider: OSWebDriver class>>#setUpGit chooses me"

	^ false
]

{ #category : 'asking' }
OSWebCredentialsProvider class >> pendingHosts [
	"The hosts whose dialog is to open, or is open"

	^ Asking ifNil: [ #(  ) ] ifNotNil: [ :hosts | hosts asArray ]
]

{ #category : 'accessing' }
OSWebCredentialsProvider class >> unstored [
	"The credentials given for this session only, by host"

	^ Unstored ifNil: [ Unstored := Dictionary new ]
]

{ #category : 'testing' }
OSWebCredentialsProvider >> insideCallback [
	"Whether libgit2's credentials callback runs me: the block of LGitAbstractRemoteCallbacks class>>#newCredentialsCallbackFor:, inside the callout of a clone, a fetch, a pull or a push"

	^ (thisContext findContextSuchThat: [ :context |
		   context homeMethod selector = #newCredentialsCallbackFor: and: [
			   context homeMethod methodClass instanceSide includesBehavior: LGitAbstractRemoteCallbacks ] ]) notNil
]

{ #category : 'providing' }
OSWebCredentialsProvider >> obtainCredentials: credentials [
	"No dialog inside libgit2's callback, nor without a world (see the class comment)"

	(self class canAsk and: [ self insideCallback not ]) ifTrue: [
		^ super obtainCredentials: credentials ].
	LGitNoCredentialsProvided signal
]

{ #category : 'providing' }
OSWebCredentialsProvider >> obtainPlaintextCredentials: credentials [
	"Inside libgit2's callback, give none, and ask once its callout has returned; outside, the dialog of Iceberg, in a world (see the class comment)"

	| inside |
	inside := self insideCallback.
	(self class canAsk and: [ inside not ]) ifTrue: [
		^ super obtainPlaintextCredentials: credentials ].
	inside ifTrue: [ self class askLaterFor: remote host ].
	LGitNoCredentialsProvided signal
]

{ #category : 'providing' }
OSWebCredentialsProvider >> obtainSshCredentials: credentials [
	"No dialog inside libgit2's callback, nor without a world (see the class comment); and a web VM reaches no SSH remote anyway"

	(self class canAsk and: [ self insideCallback not ]) ifTrue: [
		^ super obtainSshCredentials: credentials ].
	LGitNoCredentialsProvided signal
]

{ #category : 'accessing' }
OSWebCredentialsProvider >> plaintextCredentials [
	"Those given for this session only first, then those of IceCredentialStore"

	^ self class unstored at: remote host ifAbsent: [ super plaintextCredentials ]
]
