"Driver for the Rust HelloPlugin example.

Build the plugin and put it where the VM looks for modules (next to the
executable, or in the VM's Plugins directory):

	cd rust && cargo build --release -p hello-plugin
	cp target/release/libHelloPlugin.dylib <vm directory>

then run this script headless:

	pharo --headless my.image st rust/examples/hello-plugin/hello.st

The method body after the pragma is the fallback: it runs only if the VM
cannot load the module or the primitive fails, so reaching it here means
the plugin was not found."

| greeting |
Object compile: 'helloFromRust
	<primitive: ''primitiveHelloString'' module: ''HelloPlugin''>
	^ self error: ''HelloPlugin missing: is libHelloPlugin next to the VM?'''.

greeting := nil helloFromRust.

Transcript show: greeting; cr; flush.
greeting = 'Hello from Rust!'
	ifTrue: [ Transcript show: 'OK'; cr; flush ]
	ifFalse: [ self error: 'unexpected answer: ', greeting printString ].
