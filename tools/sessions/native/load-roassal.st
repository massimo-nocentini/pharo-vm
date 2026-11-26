"load-roassal.st - loads RoassalLayouts (LAYOUTS_DIR) and RoassalMN's group Full (ROASSAL_DIR), each
the src/ of a local checkout, with Metacello, the incoming versions winning, and locks them; prints
warnings, Undeclared variables and modified packages.  Clones by HTTPS, non-interactive credentials."
| env undeclaredBefore t warnings layouts roassal |
env := OSEnvironment current.
layouts := 'tonel://', (env at: 'LAYOUTS_DIR').
roassal := 'tonel://', (env at: 'ROASSAL_DIR').
IceCredentialsProvider providerType: IceNonInteractiveCredentialsProvider.
Iceberg remoteTypeSelector: #httpsUrl.
undeclaredBefore := Undeclared keys asSet.
warnings := OrderedCollection new.
[ Metacello new baseline: 'RoassalLayouts'; repository: layouts;
	onConflictUseIncoming; onUpgrade: [ :ex | ex useIncoming ];
	onWarning: [ :w | (w isKindOf: MCMergeOrLoadWarning) ifTrue: [ w load ] ifFalse: [ warnings add: 'LAYOUTS ', w messageText asString. w resume ] ];
	load ] on: Warning do: [ :w | warnings add: w messageText asString. w resume ].
Metacello new baseline: 'RoassalLayouts'; repository: layouts; lock.
t := Time millisecondClockValue.
[ Metacello new baseline: 'RoassalMN'; repository: roassal;
	onConflictUseIncoming; onUpgrade: [ :ex | ex useIncoming ]; onLock: [ :ex | ex honor ];
	onWarning: [ :w | (w isKindOf: MCMergeOrLoadWarning) ifTrue: [ w load ] ifFalse: [ warnings add: w class name , ': ' , w messageText asString. w resume ] ];
	load: 'Full' ] on: Warning do: [ :w | warnings add: w class name , ': ' , w messageText asString. w resume ].
Metacello new baseline: 'RoassalMN'; repository: roassal; lock.
Stdio stdout nextPutAll: 'ROASSAL LOADED in ', ((Time millisecondClockValue - t) // 1000) printString, ' s'; lf.
warnings do: [ :w | Stdio stdout nextPutAll: 'WARNING ', w; lf ].
Undeclared keys do: [ :k | Stdio stdout nextPutAll: 'UNDECLARED ', k, ' in ', (SystemNavigation default allReferencesTo: (Undeclared associationAt: k)) printString; lf ].
(MCWorkingCopy allWorkingCopies select: [ :c | c modified ]) do: [ :c | Stdio stdout nextPutAll: 'MODIFIED ', c packageName; lf ].
Stdio stdout flush.
