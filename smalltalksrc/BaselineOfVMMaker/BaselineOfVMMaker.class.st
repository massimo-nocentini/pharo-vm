Class {
	#name : 'BaselineOfVMMaker',
	#superclass : 'BaselineOf',
	#category : 'BaselineOfVMMaker',
	#package : 'BaselineOfVMMaker'
}

{ #category : 'baselines' }
BaselineOfVMMaker >> baseline: spec [
	<baseline>
	spec
		for: #common
		do: [
			
			"Slang, the basics"			
			spec
				package: 'CAST' with: [ spec requires: #('SmaCC-GLR' ) ];
				package: 'CAST-Tests' with: [ spec requires: #('CAST') ];
				package: 'Slang' with: [ spec requires: #('CAST') ];
				package: 'Slang-Tests' with: [ spec requires: #('Slang' 'Melchor') ].
			
			"Melchor is a VM-oriented Slang"
			spec package: 'Melchor' with: [
				spec requires: #('Slang') ].
			
			"External Dependencies, pinned by commit: each one is the head of
			the branch its comment names, as git ls-remote gave it on
			2026-11-19 (they feed the code generator, so a branch that moves
			would change the generated C unseen).  Their baselines at these
			commits load no other repository: the one github:// dependency
			they name, guillep/SMark, is in a group of OpalSimdBytecode that
			VMMaker does not load.  Should a bump bring one, lock it by commit
			in scripts/installVMMaker.st (Metacello new ... lock)"
			spec baseline: 'SmaCC-GLR' with: [ 
				spec
					loads: #( 'SmaCC-GLR-Runtime' );
					className: 'BaselineOfSmaCC';
					repository: 'github://guillep/SmaCC:b171fca906003be29525aac92b737b6e0c28bcee' "master" ].
			spec baseline: 'Unicorn' with: [ 
				spec repository: 'github://pharo-project/pharo-unicorn:1c08b5efbe226893fa53718e1be481a743cfc196' "unicorn2" ].
			spec baseline: 'LLVMDisassembler' with: [ 
				spec repository: 'github://pharo-project/pharo-llvmDisassembler:04e447e6ebba35e36ec878bbc8031a460cb6f806' "master" ].
			spec baseline: 'OpalSimdBytecode' with: [ 
				spec repository: 'github://evref-inria/pharo-opal-simd-bytecode:3071b0ef862a080c799f5a96ac0c402378a06a1e' "pharo13" ].

			"Tests"			
			spec
				package: 'VMMakerTests' with: [
					spec requires: #('VMMaker' 'Unicorn' 'LLVMDisassembler' 'Slang-Tests' 'OpalSimdBytecode') ].

			"Tools"			
			spec package: 'VMMaker-Tools' with: [ 
				spec requires: #( 'VMMakerTests' ) ].
			
			spec
				package: 'VMMakerLoadingDependencies'.
			
			"Dependencies from this repositories"
			spec 
				package: 'Printf'.
				
			"Main VMMaker packages"	
			spec
				package: 'VMMaker'
				with: [ spec requires: #( 
					'VMMakerLoadingDependencies' 'Printf' 'Melchor') ].
								
			"VM plugins"
			spec package: 'FileAttributesPlugin' with: [ spec requires: #( 'VMMaker' ) ].
			
			spec group: 'plugins' with: #('FileAttributesPlugin').
			spec group: 'core' with: #('VMMaker').
			spec group: 'tools' with: #('VMMaker-Tools').			
			spec group: 'tests' with: #('VMMakerTests' 'CAST-Tests').
			spec group: 'default' with: #( 'core' 'plugins' 'tools' 'tests') ]
]
