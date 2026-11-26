"load-ce.st - loads Containers-Essentials from a local checkout (CE_DIR, the repository root) with
Metacello, using the RoassalMN (ROASSAL_DIR) and RoassalLayouts (LAYOUTS_DIR, each the src/ of a
checkout) already loaded and locked by load-roassal.st; prints warnings, new Undeclared variables and
modified packages.  Run with fresh.sh after load-roassal.st."
| env t warnings ce roassal layouts |
env := OSEnvironment current.
ce := 'tonel://', (env at: 'CE_DIR').
roassal := 'tonel://', (env at: 'ROASSAL_DIR').
layouts := 'tonel://', (env at: 'LAYOUTS_DIR').
IceCredentialsProvider providerType: IceNonInteractiveCredentialsProvider.
Iceberg remoteTypeSelector: #httpsUrl.
warnings := OrderedCollection new.
Metacello new baseline: 'RoassalMN'; repository: roassal; lock.
Metacello new baseline: 'RoassalLayouts'; repository: layouts; lock.
t := Time millisecondClockValue.
[ Metacello new baseline: 'ContainersEssentials'; repository: ce;
	onConflictUseLoaded; onUpgrade: [ :ex | ex useLoaded ]; onLock: [ :ex | ex honor ];
	onWarning: [ :w | (w isKindOf: MCMergeOrLoadWarning) ifTrue: [ w load ] ifFalse: [ warnings add: w class name , ': ' , w messageText asString. w resume ] ];
	load ] on: Warning do: [ :w | warnings add: w class name , ': ' , w messageText asString. w resume ].
Stdio stdout nextPutAll: 'CE LOADED in ', ((Time millisecondClockValue - t) // 1000) printString, ' s'; lf.
warnings do: [ :w | Stdio stdout nextPutAll: 'WARNING ', w; lf ].
Undeclared keys do: [ :k | Stdio stdout nextPutAll: 'UNDECLARED ', k, ' in ', (SystemNavigation default allReferencesTo: (Undeclared associationAt: k)) printString; lf ].
(MCWorkingCopy allWorkingCopies select: [ :c | c modified ]) do: [ :c | Stdio stdout nextPutAll: 'MODIFIED ', c packageName; lf ].
Stdio stdout flush.
