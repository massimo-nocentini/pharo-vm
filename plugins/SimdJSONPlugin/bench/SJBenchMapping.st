"SJBenchMapping.st - the schemas of the nextAs: series of SJBench.st (Pharo 15, chunk format)

 Three files of the simdjson-data corpus are read into objects, each from its root schema #Top,
 the same mappings configuring a NeoJSONReader and a SimdJSONReader:
     twitter.json       #Top (a NeoJSONObject) whose statuses are SJBTweets, each with an SJBUser
     citm_catalog.json  #Top whose performances are SJBPerformances with their SJBPrices, and
                        whose events are a map of SJBEvents (mapWithValueSchema:)
     canada.json        #Top whose features are SJBFeatures, with an SJBGeometry whose
                        coordinates are rings of points (#Pt, a decoder answering a Point)
 Keys that a mapping does not name are read and dropped, as NeoJSON does.  NeoJSON must be
 loaded: #Top is a NeoJSONObject.  File in after SimdJSON.st and before SJBench.st
 (prepare-image.st does)."

(Object << #SJBTweet
	slots: { #id. #text. #lang. #retweet_count. #favorite_count. #user };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBUser
	slots: { #id. #name. #screen_name. #followers_count };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBPerformance
	slots: { #id. #eventId. #start. #venueCode. #prices };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBPrice
	slots: { #amount. #audienceSubCategoryId. #seatCategoryId };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBEvent
	slots: { #id. #name. #subTopicIds. #topicIds. #description. #logo. #subjectCode };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBFeature
	slots: { #type. #properties. #geometry };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBGeometry
	slots: { #type. #coordinates };
	tag: 'Mapping'; package: 'SimdJSON-Bench') install.
(Object << #SJBenchMapping tag: 'Mapping'; package: 'SimdJSON-Bench') install!

!SJBenchMapping class methodsFor: 'accessing'!
files
	"The corpus files that have a mapping"

	^ #( 'twitter.json' 'citm_catalog.json' 'canada.json' )! !

!SJBenchMapping class methodsFor: 'configuring'!
configure: aReader for: aName
	"Answer aReader, a NeoJSONReader or a SimdJSONReader, with the mappings of the corpus file
	 aName, to be read with nextAs: #Top"

	aName = 'twitter.json' ifTrue: [ ^ self twitter: aReader ].
	aName = 'citm_catalog.json' ifTrue: [ ^ self citmCatalog: aReader ].
	aName = 'canada.json' ifTrue: [ ^ self canada: aReader ].
	^ self error: 'SJBenchMapping: no mapping for ' , aName! !

!SJBenchMapping class methodsFor: 'configuring'!
twitter: aReader

	aReader
		for: SJBTweet do: [ :mapping |
			mapping mapInstVars: #( id text lang retweet_count favorite_count ).
			(mapping mapInstVar: #user) valueSchema: SJBUser ];
		for: SJBUser do: [ :mapping | mapping mapInstVars: #( id name screen_name followers_count ) ];
		for: #Statuses customDo: [ :mapping | mapping listOfElementSchema: SJBTweet ];
		for: #Top do: [ :mapping |
			mapping subjectClass: NeoJSONObject.
			(mapping mapAccessor: #statuses) valueSchema: #Statuses ].
	^ aReader! !

!SJBenchMapping class methodsFor: 'configuring'!
citmCatalog: aReader

	aReader
		for: #Top do: [ :mapping |
			mapping subjectClass: NeoJSONObject.
			(mapping mapAccessor: #performances) valueSchema: #Performances.
			(mapping mapAccessor: #events) valueSchema: #Events ];
		for: #Performances customDo: [ :mapping | mapping listOfElementSchema: SJBPerformance ];
		for: SJBPerformance do: [ :mapping |
			mapping mapInstVars: #( id eventId start venueCode ).
			(mapping mapInstVar: #prices) valueSchema: #Prices ];
		for: #Prices customDo: [ :mapping | mapping listOfElementSchema: SJBPrice ];
		for: SJBPrice do: [ :mapping | mapping mapInstVars: #( amount audienceSubCategoryId seatCategoryId ) ];
		for: #Events customDo: [ :mapping | mapping mapWithValueSchema: SJBEvent ];
		for: SJBEvent do: [ :mapping |
			mapping mapInstVars: #( id name subTopicIds topicIds description logo subjectCode ) ].
	^ aReader! !

!SJBenchMapping class methodsFor: 'configuring'!
canada: aReader

	aReader
		for: #Top do: [ :mapping |
			mapping subjectClass: NeoJSONObject.
			(mapping mapAccessor: #features) valueSchema: #Features ];
		for: #Features customDo: [ :mapping | mapping listOfElementSchema: SJBFeature ];
		for: SJBFeature do: [ :mapping |
			mapping mapInstVars: #( type properties ).
			(mapping mapInstVar: #geometry) valueSchema: SJBGeometry ];
		for: SJBGeometry do: [ :mapping |
			mapping mapInstVar: #type.
			(mapping mapInstVar: #coordinates) valueSchema: #Rings ];
		for: #Rings customDo: [ :mapping | mapping listOfElementSchema: #Ring ];
		for: #Ring customDo: [ :mapping | mapping listOfElementSchema: #Pt ];
		for: #Pt customDo: [ :mapping | mapping decoder: [ :pair | pair first @ pair second ] ].
	^ aReader! !
