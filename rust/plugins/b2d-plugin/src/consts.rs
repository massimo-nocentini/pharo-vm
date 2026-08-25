//! The constants at the top of the generated `B2DPlugin.c`, verbatim.
//!
//! Every one of these is ABI: the work buffer is an image-side `Bitmap` whose
//! layout the image and the plugin agree on word by word, and the embedded
//! object records (edges, fills) carry these type tags and sizes. Names are
//! kept exactly as in the C so the two sources read side by side.

#![allow(non_upper_case_globals)]

use crate::engine::SqInt;

// --- BalloonEngine instance layout (the engine oop's slots) -----------------
pub const BEBalloonEngineSize: SqInt = 12;
pub const BEBitBltIndex: SqInt = 2;
pub const BEFormsIndex: SqInt = 3;
pub const BESpanIndex: SqInt = 1;
pub const BEWorkBufferIndex: SqInt = 0;

// --- BalloonEdgeData / BalloonFillData instance layouts ---------------------
pub const ETBalloonEdgeDataSize: SqInt = 6;
pub const ETIndexIndex: SqInt = 0;
pub const ETLinesIndex: SqInt = 4;
pub const ETXValueIndex: SqInt = 1;
pub const ETYValueIndex: SqInt = 2;
pub const ETZValueIndex: SqInt = 3;
pub const FTBalloonFillDataSize: SqInt = 6;
pub const FTIndexIndex: SqInt = 0;
pub const FTMaxXIndex: SqInt = 2;
pub const FTMinXIndex: SqInt = 1;
pub const FTYValueIndex: SqInt = 3;

// --- Bezier records ---------------------------------------------------------
pub const GBBaseSize: SqInt = 16;
pub const GBBitmapDepth: SqInt = 12;
pub const GBBitmapHeight: SqInt = 11;
pub const GBBitmapRaster: SqInt = 14;
pub const GBBitmapSize: SqInt = 13;
pub const GBBitmapWidth: SqInt = 10;
pub const GBColormapOffset: SqInt = 18;
pub const GBColormapSize: SqInt = 15;
pub const GBEndX: SqInt = 14;
pub const GBEndY: SqInt = 15;
pub const GBFinalX: SqInt = 21;
pub const GBMBaseSize: SqInt = 18;
pub const GBTileFlag: SqInt = 16;
pub const GBUpdateData: SqInt = 10;
pub const GBUpdateDDX: SqInt = 4;
pub const GBUpdateDDY: SqInt = 5;
pub const GBUpdateDX: SqInt = 2;
pub const GBUpdateDY: SqInt = 3;
pub const GBUpdateX: SqInt = 0;
pub const GBUpdateY: SqInt = 1;
pub const GBViaX: SqInt = 12;
pub const GBViaY: SqInt = 13;
pub const GBWideEntry: SqInt = 18;
pub const GBWideExit: SqInt = 19;
pub const GBWideExtent: SqInt = 20;
pub const GBWideFill: SqInt = 16;
pub const GBWideSize: SqInt = 28;
pub const GBWideUpdateData: SqInt = 22;
pub const GBWideWidth: SqInt = 17;

// --- Generic edge / fill records --------------------------------------------
pub const GEBaseEdgeSize: SqInt = 10;
pub const GEBaseFillSize: SqInt = 4;
pub const GEEdgeFillsInvalid: SqInt = 0x10000;

// --- Failure codes (primitiveFailFor arguments beyond the standard table) ---
pub const GEFAlreadyFailed: SqInt = 100;
pub const GEFBadPoint: SqInt = 121;
pub const GEFBitBltLoadFailed: SqInt = 122;
pub const GEFClassMismatch: SqInt = 114;
pub const GEFEdgeDataTooSmall: SqInt = 112;
pub const GEFEngineIsInteger: SqInt = 101;
pub const GEFEngineIsWords: SqInt = 102;
pub const GEFEngineStopped: SqInt = 104;
pub const GEFEngineTooSmall: SqInt = 103;
pub const GEFEntityCheckFailed: SqInt = 120;
pub const GEFEntityLoadFailed: SqInt = 119;
pub const GEFFillDataTooSmall: SqInt = 113;
pub const GEFFormLoadFailed: SqInt = 123;
pub const GEFillIndexLeft: SqInt = 8;
pub const GEFillIndexRight: SqInt = 9;
pub const GEFSizeMismatch: SqInt = 115;
pub const GEFWorkBufferBadMagic: SqInt = 108;
pub const GEFWorkBufferIsInteger: SqInt = 105;
pub const GEFWorkBufferIsPointers: SqInt = 106;
pub const GEFWorkBufferStartWrong: SqInt = 110;
pub const GEFWorkBufferTooSmall: SqInt = 107;
pub const GEFWorkBufferWrongSize: SqInt = 109;
pub const GEFWorkTooBig: SqInt = 111;
pub const GEFWrongEdge: SqInt = 118;
pub const GEFWrongFill: SqInt = 117;
pub const GEFWrongState: SqInt = 116;
pub const GENumLines: SqInt = 7;
pub const GEObjectIndex: SqInt = 2;
pub const GEObjectLength: SqInt = 1;
pub const GEObjectType: SqInt = 0;

// --- Object type tags -------------------------------------------------------
pub const GEPrimitiveBezier: SqInt = 6;
pub const GEPrimitiveClippedBitmapFill: SqInt = 0x400;
pub const GEPrimitiveEdge: SqInt = 2;
pub const GEPrimitiveEdgeMask: SqInt = 0xFF;
pub const GEPrimitiveFill: SqInt = 0x100;
pub const GEPrimitiveFillMask: SqInt = 0xFF00;
pub const GEPrimitiveLine: SqInt = 4;
pub const GEPrimitiveLinearGradientFill: SqInt = 0x200;
pub const GEPrimitiveRadialGradientFill: SqInt = 0x300;
pub const GEPrimitiveTypeMask: SqInt = 0xFFFF;
pub const GEPrimitiveWide: SqInt = 1;
pub const GEPrimitiveWideBezier: SqInt = 7;
pub const GEPrimitiveWideLine: SqInt = 5;
pub const GEPrimitiveWideMask: SqInt = 0xFE;

// --- Stop reasons (the image resumes rendering off these) -------------------
pub const GErrorAETEntry: SqInt = 6;
pub const GErrorBadState: SqInt = 2;
pub const GErrorFillEntry: SqInt = 5;
pub const GErrorGETEntry: SqInt = 4;
pub const GErrorNeedFlush: SqInt = 3;
pub const GErrorNoMoreSpace: SqInt = 1;

// --- Engine states ----------------------------------------------------------
pub const GEStateAddingFromGET: SqInt = 1;
pub const GEStateBlitBuffer: SqInt = 5;
pub const GEStateCompleted: SqInt = 8;
pub const GEStateScanningAET: SqInt = 3;
pub const GEStateUnlocked: SqInt = 0;
pub const GEStateUpdateEdges: SqInt = 6;
pub const GEStateWaitingChange: SqInt = 7;
pub const GEStateWaitingForEdge: SqInt = 2;
pub const GEStateWaitingForFill: SqInt = 4;

// --- Edge fields ------------------------------------------------------------
pub const GEXValue: SqInt = 4;
pub const GEYValue: SqInt = 5;
pub const GEZValue: SqInt = 6;

// --- Gradient / oriented fill fields ----------------------------------------
pub const GFDirectionX: SqInt = 6;
pub const GFDirectionY: SqInt = 7;
pub const GFNormalX: SqInt = 8;
pub const GFNormalY: SqInt = 9;
pub const GFOriginX: SqInt = 4;
pub const GFOriginY: SqInt = 5;
pub const GFRampLength: SqInt = 10;
pub const GFRampOffset: SqInt = 12;
pub const GGBaseSize: SqInt = 12;

// --- Line records -----------------------------------------------------------
pub const GLBaseSize: SqInt = 16;
pub const GLEndX: SqInt = 14;
pub const GLEndY: SqInt = 15;
pub const GLError: SqInt = 13;
pub const GLErrorAdjDown: SqInt = 15;
pub const GLErrorAdjUp: SqInt = 14;
pub const GLWideEntry: SqInt = 18;
pub const GLWideExit: SqInt = 19;
pub const GLWideExtent: SqInt = 20;
pub const GLWideFill: SqInt = 16;
pub const GLWideSize: SqInt = 21;
pub const GLWideWidth: SqInt = 17;
pub const GLXDirection: SqInt = 10;
pub const GLXIncrement: SqInt = 12;
pub const GLYDirection: SqInt = 11;

// --- Work buffer header word offsets ----------------------------------------
pub const GWAAColorMask: SqInt = 0x33;
pub const GWAAColorShift: SqInt = 50;
pub const GWAAHalfPixel: SqInt = 53;
pub const GWAALevel: SqInt = 48;
pub const GWAAScanMask: SqInt = 0x34;
pub const GWAAShift: SqInt = 49;
pub const GWAETStart: SqInt = 13;
pub const GWAETUsed: SqInt = 14;
pub const GWBezierHeightSubdivisions: SqInt = 109;
pub const GWBezierLineConversions: SqInt = 111;
pub const GWBezierMonotonSubdivisions: SqInt = 108;
pub const GWBezierOverflowSubdivisions: SqInt = 110;
pub const GWBufferTop: SqInt = 10;
pub const GWClearSpanBuffer: SqInt = 69;
pub const GWClipMaxX: SqInt = 43;
pub const GWClipMaxY: SqInt = 45;
pub const GWClipMinX: SqInt = 42;
pub const GWClipMinY: SqInt = 44;
pub const GWColorTransform: SqInt = 24;
pub const GWCountAddAETEntry: SqInt = 97;
pub const GWCountChangeAETEntry: SqInt = 107;
pub const GWCountDisplaySpan: SqInt = 103;
pub const GWCountFinishTest: SqInt = 93;
pub const GWCountInitializing: SqInt = 91;
pub const GWCountMergeFill: SqInt = 101;
pub const GWCountNextAETEntry: SqInt = 105;
pub const GWCountNextFillEntry: SqInt = 99;
pub const GWCountNextGETEntry: SqInt = 95;
pub const GWCurrentY: SqInt = 88;
pub const GWCurrentZ: SqInt = 113;
pub const GWDestOffsetX: SqInt = 46;
pub const GWDestOffsetY: SqInt = 47;
pub const GWEdgeTransform: SqInt = 18;
pub const GWFillMaxX: SqInt = 37;
pub const GWFillMaxY: SqInt = 39;
pub const GWFillMinX: SqInt = 36;
pub const GWFillMinY: SqInt = 38;
pub const GWFillOffsetX: SqInt = 40;
pub const GWFillOffsetY: SqInt = 41;
pub const GWGETStart: SqInt = 11;
pub const GWGETUsed: SqInt = 12;
pub const GWHasColorTransform: SqInt = 17;
pub const GWHasEdgeTransform: SqInt = 16;
pub const GWHeaderSize: SqInt = 128;
pub const GWLastExportedEdge: SqInt = 65;
pub const GWLastExportedFill: SqInt = 66;
pub const GWLastExportedLeftX: SqInt = 67;
pub const GWLastExportedRightX: SqInt = 68;
pub const GWMagicIndex: SqInt = 0;
pub const GWMagicNumber: SqInt = 1097753705;
pub const GWMinimalSize: SqInt = 256;
pub const GWNeedsFlush: SqInt = 63;
pub const GWObjStart: SqInt = 8;
pub const GWObjUsed: SqInt = 9;
pub const GWPoint1: SqInt = 80;
pub const GWPoint2: SqInt = 82;
pub const GWPoint3: SqInt = 84;
pub const GWPoint4: SqInt = 86;
pub const GWPointListFirst: SqInt = 70;
pub const GWSize: SqInt = 1;
pub const GWSpanEnd: SqInt = 34;
pub const GWSpanEndAA: SqInt = 35;
pub const GWSpanSize: SqInt = 33;
pub const GWSpanStart: SqInt = 32;
pub const GWState: SqInt = 2;
pub const GWStopReason: SqInt = 64;
pub const GWTimeAddAETEntry: SqInt = 96;
pub const GWTimeChangeAETEntry: SqInt = 106;
pub const GWTimeDisplaySpan: SqInt = 102;
pub const GWTimeFinishTest: SqInt = 92;
pub const GWTimeInitializing: SqInt = 90;
pub const GWTimeMergeFill: SqInt = 100;
pub const GWTimeNextAETEntry: SqInt = 104;
pub const GWTimeNextFillEntry: SqInt = 98;
pub const GWTimeNextGETEntry: SqInt = 94;

// --- Standard primitive failure codes the C names directly ------------------
pub const PrimErrBadArgument: SqInt = 3;
pub const PrimErrBadNumArgs: SqInt = 5;

/// `stackFillEntryLength` in the Smalltalk source; the C inlines it as
/// `3 /* stackFillEntryLength */` everywhere.
pub const StackFillEntryLength: SqInt = 3;
