/* Automatically generated by
	CCodeGenerator VMMaker.oscog-eem.775 uuid: dd27f525-f775-49fc-8bf0-2463d78bfb97
 */

typedef struct {
	unsigned short	homeOffset;
	unsigned short	startpc;
	unsigned		cmNumArgs : 8;
	unsigned		cmType : 3;
	unsigned		cmRefersToYoung : 1;
	unsigned		cpicHasMNUCase : 1;
	unsigned		cmUsageCount : 3;
	unsigned		cmUsesPenultimateLit : 1;
	unsigned		cmUsesMethodClass : 1;
	unsigned		cmUnusedFlags : 2;
	unsigned		stackCheckOffset : 12;
 } CogBlockMethod;

typedef struct {
	sqInt	objectHeader;
	unsigned		cmNumArgs : 8;
	unsigned		cmType : 3;
	unsigned		cmRefersToYoung : 1;
	unsigned		cpicHasMNUCase : 1;
	unsigned		cmUsageCount : 3;
	unsigned		cmUsesPenultimateLit : 1;
	unsigned		cmUsesMethodClass : 1;
	unsigned		cmUnusedFlags : 2;
	unsigned		stackCheckOffset : 12;
	unsigned short	blockSize;
	unsigned short	blockEntryOffset;
	sqInt	methodObject;
	sqInt	methodHeader;
	sqInt	selector;
	sqInt	numCounters;
 } CogMethod;

#define SistaCogMethod CogMethod
