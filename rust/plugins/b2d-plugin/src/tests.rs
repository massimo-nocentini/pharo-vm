//! Engine-core tests: a work buffer is built the way
//! `primitiveInitializeBuffer` builds one, tiny renders are driven through
//! the same state machine the primitives use, and the resulting spans /
//! framebuffer pixels are asserted exactly.
//!
//! No VM is involved: the [`Host`] is a test double that serves form bits
//! from vectors and "blits" the span buffer into a small framebuffer, which
//! is what BitBlt's `copyBitsFromtoat` does for the C plugin.

#![allow(non_snake_case)]

use crate::consts::*;
use crate::engine::{absoluteSquared8Dot24, Engine, Host, SqInt};
use crate::shapes::PointsRef;

/// Test double for the engine's outside world.
struct TestHost {
    span: *const u32,
    span_len: usize,
    fb_width: usize,
    fb: Vec<u32>,
    /// `forms[i]` plays the role of `(formArray at: i+1) bits`.
    forms: Vec<Vec<i32>>,
    blits: Vec<(SqInt, SqInt, SqInt)>,
}

impl Host for TestHost {
    fn bits_of_form(&mut self, x_index: SqInt) -> Option<(*const i32, SqInt)> {
        if x_index > self.forms.len() as SqInt {
            return None;
        }
        let f = self.forms.get(x_index as usize)?;
        Some((f.as_ptr(), f.len() as SqInt))
    }

    fn copyBitsFromtoat(&mut self, x0: SqInt, x1: SqInt, y_value: SqInt) {
        self.blits.push((x0, x1, y_value));
        for x in x0..x1 {
            let idx = y_value as usize * self.fb_width + x as usize;
            if idx < self.fb.len() && (x as usize) < self.span_len {
                // SAFETY: x < span_len, and the test keeps the span Vec alive.
                self.fb[idx] = unsafe { *self.span.add(x as usize) };
            }
        }
    }

    fn ioMicroMSecs(&mut self) -> SqInt {
        0
    }
}

/// A work buffer + span buffer + engine, wired the way the primitives wire
/// them. The buffers must outlive the engine, hence the tuple.
fn make_engine(
    wb_slots: usize,
    span_slots: usize,
    fb_w: usize,
    fb_h: usize,
) -> (Vec<i32>, Vec<u32>, Engine<TestHost>) {
    let mut wb = vec![0i32; wb_slots];
    let mut span = vec![0u32; span_slots];
    let host = TestHost {
        span: span.as_ptr(),
        span_len: span_slots,
        fb_width: fb_w,
        fb: vec![0; fb_w * fb_h],
        forms: Vec::new(),
        blits: Vec::new(),
    };
    let mut e = Engine::new(host);
    // SAFETY: the vectors outlive the engine (returned together) and Vec's
    // heap buffer does not move when the Vec itself is moved.
    unsafe {
        e.set_work_buffer(wb.as_mut_ptr(), wb_slots);
        e.set_span_buffer(span.as_mut_ptr(), span_slots);
    }
    e.initializeBuffer(wb_slots as SqInt);
    e.objUsed = e.wb_at(GWObjUsed);
    // The span-buffer half of loadSpanBufferFrom:.
    e.wb_put(GWSpanSize, span_slots as SqInt - 1);
    (wb, span, e)
}

/// The image-side setup every render does: AA level, clip rect, offsets.
fn prepare_render(e: &mut Engine<TestHost>, w: SqInt, h: SqInt) {
    e.setAALevel(1);
    e.wb_put(GWClipMinX, 0);
    e.wb_put(GWClipMinY, 0);
    e.wb_put(GWClipMaxX, w);
    e.wb_put(GWClipMaxY, h);
    e.wb_put(GWDestOffsetX, 0);
    e.wb_put(GWDestOffsetY, 0);
}

/// The primitiveRenderImage sequence, minus the oop plumbing.
fn render_image(e: &mut Engine<TestHost>) {
    e.proceedRenderingScanline();
    assert!(!e.engineStopped, "render stopped: reason {}", e.wb_at(GWStopReason));
    e.proceedRenderingImage();
    assert!(!e.engineStopped, "render stopped: reason {}", e.wb_at(GWStopReason));
    assert!(e.finishedProcessing());
}

const FILL: SqInt = 0xFF123456u32 as SqInt;

// ---------------------------------------------------------------------------
// Work-buffer layout
// ---------------------------------------------------------------------------

#[test]
fn initialize_buffer_lays_out_the_header_like_the_c() {
    let (_wb, _span, e) = make_engine(512, 16, 1, 1);
    assert_eq!(e.wb_at(GWMagicIndex), GWMagicNumber);
    assert_eq!(e.wb_at(GWSize), 512);
    assert_eq!(e.wb_at(GWBufferTop), 512);
    assert_eq!(e.wb_at(GWState), GEStateUnlocked);
    assert_eq!(e.wb_at(GWObjStart), GWHeaderSize);
    assert_eq!(e.wb_at(GWObjUsed), 4);
    // Object 0 is the reserved "no fill" record.
    assert_eq!(e.objat(0, GEObjectType), GEPrimitiveFill);
    assert_eq!(e.objat(0, GEObjectLength), 4);
    assert_eq!(e.objat(0, GEObjectIndex), 0);
    // Identity transforms, marked absent.
    assert_eq!(e.wb_at(GWHasEdgeTransform), 0);
    assert_eq!(e.wb_at(GWHasColorTransform), 0);
    assert_eq!(e.wb_f32(GWEdgeTransform), 1.0);
    assert_eq!(e.wb_f32(GWColorTransform + 6), 1.0);
}

#[test]
fn attach_work_buffer_reports_the_c_failure_codes() {
    let (_wb, _span, mut e) = make_engine(512, 16, 1, 1);
    assert_eq!(e.attach_work_buffer_checked(512), 0);
    // Wrong slot count.
    assert_eq!(e.attach_work_buffer_checked(511), GEFWorkBufferWrongSize);
    // Wrong object start.
    e.wb_put(GWObjStart, 100);
    assert_eq!(e.attach_work_buffer_checked(512), GEFWorkBufferStartWrong);
    e.wb_put(GWObjStart, GWHeaderSize);
    // Tables claiming more than the buffer holds.
    e.wb_put(GWObjUsed, 1000);
    assert_eq!(e.attach_work_buffer_checked(512), GEFWorkTooBig);
    e.wb_put(GWObjUsed, 4);
    // Bad magic, checked first of the content checks.
    e.wb_put(GWMagicIndex, 42);
    assert_eq!(e.attach_work_buffer_checked(512), GEFWorkBufferBadMagic);
}

#[test]
fn allocation_failure_sets_the_no_more_space_stop_reason() {
    let (_wb, _span, mut e) = make_engine(GWMinimalSize as usize, 16, 1, 1);
    // 256 slots minus the 128-word header minus objUsed=4 leaves 124 free.
    assert!(e.allocateObjEntry(100));
    e.objUsed += 100; // the allocate* helpers advance objUsed like this
    assert!(!e.engineStopped);
    assert!(!e.allocateObjEntry(100));
    assert!(e.engineStopped);
    assert_eq!(e.wb_at(GWStopReason), GErrorNoMoreSpace);
}

// ---------------------------------------------------------------------------
// Small arithmetic
// ---------------------------------------------------------------------------

#[test]
fn absolute_squared_8dot24_squares_fixed_point() {
    // 0.5^2 = 0.25 in 8.24.
    assert_eq!(absoluteSquared8Dot24(0x800000), 0x400000);
    // 0.25^2 = 0.0625.
    assert_eq!(absoluteSquared8Dot24(0x400000), 0x100000);
    // 1.0 is outside the documented [0,1) range: the integer part is masked
    // to 8 bits, so 0x1000000 squares to 0 — in the C too.
    assert_eq!(absoluteSquared8Dot24(0x1000000), 0);
    // The largest in-range value.
    assert_eq!(absoluteSquared8Dot24(0xFFFFFF), 0xFFFFFE);
}

#[test]
fn set_aa_level_derives_the_mask_family() {
    let (_wb, _span, mut e) = make_engine(512, 16, 1, 1);
    e.setAALevel(1);
    assert_eq!(
        (e.wb_at(GWAALevel), e.wb_at(GWAAShift), e.wb_i32(GWAAColorMask) as u32, e.wb_at(GWAAScanMask)),
        (1, 0, 0xFFFFFFFF, 0)
    );
    e.setAALevel(2);
    assert_eq!(
        (e.wb_at(GWAALevel), e.wb_at(GWAAShift), e.wb_i32(GWAAColorMask) as u32, e.wb_at(GWAAScanMask)),
        (2, 1, 4244438268, 1)
    );
    assert_eq!(e.wb_at(GWAAColorShift), 2);
    e.setAALevel(4);
    assert_eq!(
        (e.wb_at(GWAALevel), e.wb_at(GWAAShift), e.wb_i32(GWAAColorMask) as u32, e.wb_at(GWAAScanMask)),
        (4, 2, 4042322160, 3)
    );
    // 3 clamps down to 2, 0 up to 1.
    e.setAALevel(3);
    assert_eq!(e.wb_at(GWAALevel), 2);
    e.setAALevel(0);
    assert_eq!(e.wb_at(GWAALevel), 1);
}

#[test]
fn transform_width_uses_the_shorter_axis_and_never_answers_zero() {
    let (_wb, _span, mut e) = make_engine(512, 16, 1, 1);
    e.setAALevel(1);
    e.wb_put(GWDestOffsetX, 0);
    e.wb_put(GWDestOffsetY, 0);
    assert_eq!(e.transformWidth(0), 0);
    assert_eq!(e.transformWidth(3), 3);
    assert_eq!(e.transformWidth(1), 1);
}

#[test]
fn transform_color_stops_for_translucency_only_when_flush_is_pending() {
    let (_wb, _span, mut e) = make_engine(512, 16, 1, 1);
    // Opaque color: unchanged, no stop.
    assert_eq!(e.transformColor(FILL), FILL);
    assert!(!e.engineStopped);
    // No alpha bits means "fill index", answered untouched (not a color).
    assert_eq!(e.transformColor(0x00123456), 0x00123456);
    // Translucent without pending flush: passes through.
    let translucent = 0x80123456u32 as SqInt;
    assert_eq!(e.transformColor(translucent), translucent);
    assert!(!e.engineStopped);
    // Translucent with needsFlush set: the GErrorNeedFlush stop the image
    // resumes off.
    e.wb_put(GWNeedsFlush, 1);
    assert_eq!(e.transformColor(translucent), translucent);
    assert!(e.engineStopped);
    assert_eq!(e.wb_at(GWStopReason), GErrorNeedFlush);
    // A plain fill index is never a color and never stops.
    e.engineStopped = false;
    assert_eq!(e.transformColor(4), 4);
    assert!(!e.engineStopped);
}

#[test]
fn short_points_use_the_little_endian_halfword_order() {
    let words = [0x0002_0001i32, 0xFFFF_FFFEu32 as i32];
    let p = PointsRef::from_slice(&words);
    assert_eq!(p.short_at(0), 1);
    assert_eq!(p.short_at(1), 2);
    assert_eq!(p.short_at(2), -2);
    assert_eq!(p.short_at(3), -1);
    assert_eq!(p.int_at(0), 0x0002_0001);
}

// ---------------------------------------------------------------------------
// Edge stepping
// ---------------------------------------------------------------------------

#[test]
fn line_stepping_matches_bresenham() {
    let (_wb, _span, mut e) = make_engine(1024, 16, 1, 1);
    // A line from (8,0) to (0,8): x-major boundary case, xInc -1 per line.
    let line = e.allocateLine();
    e.objatput(line, GEXValue, 8);
    e.objatput(line, GEYValue, 0);
    e.objatput(line, GLEndX, 0);
    e.objatput(line, GLEndY, 8);
    e.objatput(line, GLYDirection, 1);
    e.stepToFirstLineInat(line, 0);
    assert_eq!(e.objat(line, GENumLines), 8);
    assert_eq!(e.objat(line, GLXIncrement), -1);
    assert_eq!(e.objat(line, GLErrorAdjUp), 0);
    for expected_x in [7, 6, 5, 4, 3, 2, 1, 0] {
        e.stepToNextLineInat(line, 0);
        assert_eq!(e.objat(line, GEXValue), expected_x);
    }

    // A shallow line from (0,0) to (7,3): x = 0 -> 2 -> 4 -> 7.
    let line = e.allocateLine();
    e.objatput(line, GEXValue, 0);
    e.objatput(line, GEYValue, 0);
    e.objatput(line, GLEndX, 7);
    e.objatput(line, GLEndY, 3);
    e.objatput(line, GLYDirection, 1);
    e.stepToFirstLineInat(line, 0);
    assert_eq!(e.objat(line, GENumLines), 3);
    assert_eq!(e.objat(line, GLXIncrement), 2);
    assert_eq!(e.objat(line, GLErrorAdjUp), 1);
    assert_eq!(e.objat(line, GLErrorAdjDown), 3);
    let mut xs = Vec::new();
    for _ in 0..3 {
        e.stepToNextLineInat(line, 0);
        xs.push(e.objat(line, GEXValue));
    }
    // Step 1: x = 0+2, err = 0+1 > 0 so x -> 3, err -> -2; then 5, 7.
    assert_eq!(xs, [3, 5, 7]);
}

#[test]
fn stepping_to_a_later_scanline_catches_up() {
    let (_wb, _span, mut e) = make_engine(1024, 16, 1, 1);
    let line = e.allocateLine();
    e.objatput(line, GEXValue, 8);
    e.objatput(line, GEYValue, 0);
    e.objatput(line, GLEndX, 0);
    e.objatput(line, GLEndY, 8);
    e.stepToFirstLineInat(line, 3);
    assert_eq!(e.objat(line, GEXValue), 5);
    assert_eq!(e.objat(line, GENumLines), 5);
}

#[test]
fn degenerate_bezier_steps_like_its_chord() {
    let (_wb, _span, mut e) = make_engine(1024, 16, 1, 1);
    // (0,0) via (4,4) to (8,8): control point on the chord, so x == y.
    let bz = e.allocateBezier();
    e.objatput(bz, GEXValue, 0);
    e.objatput(bz, GEYValue, 0);
    e.objatput(bz, GBViaX, 4);
    e.objatput(bz, GBViaY, 4);
    e.objatput(bz, GBEndX, 8);
    e.objatput(bz, GBEndY, 8);
    e.stepToFirstBezierInat(bz, 0);
    assert_eq!(e.objat(bz, GENumLines), 8);
    for y in 1..=8 {
        e.stepToNextBezierInat(bz, y);
        assert_eq!(e.objat(bz, GEXValue), y, "x at scanline {y}");
    }
}

#[test]
fn tall_beziers_are_subdivided() {
    let (_wb, _span, mut e) = make_engine(4096, 16, 1, 1);
    e.setAALevel(1);
    // 600 scan lines tall: must split until each piece is <= 255 tall.
    e.point_put(GWPoint1, 0, 0);
    e.point_put(GWPoint2, 0, 300);
    e.point_put(GWPoint3, 0, 600);
    let segs = e.loadAndSubdivideBezierFromviatoisWide(GWPoint1, GWPoint2, GWPoint3, false);
    assert!(!e.engineStopped);
    assert!(segs >= 3, "600 lines need at least 3 pieces, got {segs}");
    assert!(e.wb_at(GWBezierHeightSubdivisions) > 0);
}

// ---------------------------------------------------------------------------
// AET ordering
// ---------------------------------------------------------------------------

#[test]
fn aet_insertion_sorts_by_x_then_y() {
    let (_wb, _span, mut e) = make_engine(1024, 16, 1, 1);
    let mk = |e: &mut Engine<TestHost>, x: SqInt, y: SqInt| {
        let l = e.allocateLine();
        e.objatput(l, GEXValue, x);
        e.objatput(l, GEYValue, y);
        e.objatput(l, GENumLines, 5);
        l
    };
    let a = mk(&mut e, 5, 0);
    let b = mk(&mut e, 2, 0);
    let c = mk(&mut e, 9, 0);
    let d = mk(&mut e, 5, 1); // same x as `a`, later y: sorts after `a`
    // Tables sit after the objects, as initializeGETProcessing arranges.
    e.get = e.obj + e.objUsed as usize;
    e.aet = e.obj + e.objUsed as usize;
    e.wb_put(GWAETUsed, 0);
    for edge in [a, b, c, d] {
        e.insertEdgeIntoAET(edge);
    }
    assert_eq!(e.wb_at(GWAETUsed), 4);
    let order: Vec<SqInt> = (0..4).map(|i| e.aet_at(i)).collect();
    assert_eq!(order, [b, a, d, c]);
}

#[test]
fn fill_stack_shows_deeper_fills_on_top() {
    let (_wb, _span, mut e) = make_engine(1024, 16, 1, 1);
    // Stack cells are 32 bits: values come back sign-extended, exactly as the
    // C's topFill sign-extends the int cell, so the depth-toggling callers
    // always pass sign-extended values.
    let se = |v: SqInt| v as i32 as SqInt;
    e.showFilldepthrightX(se(FILL), 2, 100);
    let deeper = 0xFFAA0000u32 as i32 as SqInt;
    e.showFilldepthrightX(deeper, 4, 100);
    assert_eq!(e.topFill(), deeper);
    assert_eq!(e.topDepth(), 4);
    // Hiding the top re-establishes the other entry.
    assert!(e.hideFilldepth(deeper, 4));
    assert_eq!(e.topFill(), se(FILL));
    assert!(e.hideFilldepth(se(FILL), 2));
    assert_eq!(e.wbStackSize(), 0);
    assert_eq!(e.topFill(), 0);
    assert_eq!(e.topRightX(), 999999999);
}

// ---------------------------------------------------------------------------
// Whole tiny renders
// ---------------------------------------------------------------------------

/// Renders an 8x8 rectangle the way primitiveAddRect + primitiveRenderImage
/// would (identity transform, AA level 1).
#[test]
fn renders_a_filled_rectangle() {
    let (_wb, _span, mut e) = make_engine(2048, 16, 8, 9);
    prepare_render(&mut e, 8, 9);
    e.point_put(GWPoint1, 0, 0);
    e.point_put(GWPoint3, 8, 8);
    e.makeRectFromPoints();
    e.transformPoints(4);
    e.loadRectanglelineFillleftFillrightFill(0, 0, 0, FILL);
    assert!(!e.engineStopped);
    render_image(&mut e);
    for y in 0..9usize {
        for x in 0..8usize {
            let expected = if y < 8 { FILL as u32 } else { 0 };
            assert_eq!(e.host.fb[y * 8 + x], expected, "pixel ({x},{y})");
        }
    }
}

/// A right triangle from three lines, exercising the polygon loader, the AET
/// and the Bresenham stepping together.
#[test]
fn renders_a_triangle_from_lines() {
    let (_wb, _span, mut e) = make_engine(2048, 16, 8, 9);
    prepare_render(&mut e, 8, 9);
    let points = [0, 0, 8, 0, 0, 8, 0, 0];
    e.loadPolygonnPointsfilllineWidthlineFillpointsShort(
        PointsRef::from_slice(&points),
        4,
        FILL,
        0,
        0,
        false,
    );
    assert!(!e.engineStopped);
    render_image(&mut e);
    for y in 0..9usize {
        for x in 0..8usize {
            let inside = y < 8 && (x as SqInt) < 8 - y as SqInt;
            let expected = if inside { FILL as u32 } else { 0 };
            assert_eq!(e.host.fb[y * 8 + x], expected, "pixel ({x},{y})");
        }
    }
}

/// The same triangle with the hypotenuse as a degenerate bezier must render
/// pixel-identically to the line version.
#[test]
fn renders_a_triangle_from_a_bezier() {
    let expected = {
        let (_wb, _span, mut e) = make_engine(2048, 16, 8, 9);
        prepare_render(&mut e, 8, 9);
        let points = [0, 0, 8, 0, 0, 8, 0, 0];
        e.loadPolygonnPointsfilllineWidthlineFillpointsShort(
            PointsRef::from_slice(&points),
            4,
            FILL,
            0,
            0,
            false,
        );
        render_image(&mut e);
        e.host.fb.clone()
    };

    let (_wb, _span, mut e) = make_engine(2048, 16, 8, 9);
    prepare_render(&mut e, 8, 9);
    // Top edge (0,0)-(8,0) and left edge (0,8)-(0,0) as lines...
    e.point_put(GWPoint1, 0, 0);
    e.point_put(GWPoint2, 8, 0);
    e.transformPoints(2);
    e.loadWideLinefromtolineFillleftFillrightFill(0, GWPoint1, GWPoint2, 0, FILL, 0);
    e.point_put(GWPoint1, 0, 8);
    e.point_put(GWPoint2, 0, 0);
    e.transformPoints(2);
    e.loadWideLinefromtolineFillleftFillrightFill(0, GWPoint1, GWPoint2, 0, FILL, 0);
    // ...and the hypotenuse (8,0) via (4,4) to (0,8) as a bezier.
    e.point_put(GWPoint1, 8, 0);
    e.point_put(GWPoint2, 4, 4);
    e.point_put(GWPoint3, 0, 8);
    e.transformPoints(3);
    let segs = e.loadAndSubdivideBezierFromviatoisWide(GWPoint1, GWPoint2, GWPoint3, false);
    assert!(!e.engineStopped);
    e.loadWideBezierlineFillleftFillrightFilln(0, 0, FILL, 0, segs);
    assert!(!e.engineStopped);
    render_image(&mut e);
    assert_eq!(e.host.fb, expected);
}

// ---------------------------------------------------------------------------
// Gradient and bitmap fills
// ---------------------------------------------------------------------------

/// Sets the clip-derived fill bounds and span bookkeeping the way
/// initializeGETProcessing would, without building a GET.
fn prepare_span_fill(e: &mut Engine<TestHost>, w: SqInt) {
    prepare_render(e, w, 8);
    e.wb_put(GWFillMinX, 0);
    e.wb_put(GWFillMinY, 0);
    e.wb_put(GWFillMaxX, w);
    e.wb_put(GWFillMaxY, 8);
    e.wb_put(GWSpanStart, e.wb_at(GWSpanSize));
    e.wb_put(GWSpanEnd, 0);
    e.wb_put(GWSpanEndAA, 0);
    e.wb_put(GWCurrentY, 0);
}

#[test]
fn linear_gradient_walks_the_ramp_one_entry_per_pixel() {
    let (_wb, span, mut e) = make_engine(2048, 16, 8, 8);
    prepare_span_fill(&mut e, 8);
    let ramp: Vec<i32> = (0..8).map(|i| (0xFF000000u32 | i) as i32).collect();
    let fill = e.allocateGradientFillrampWidthisRadial(PointsRef::from_slice(&ramp), 8, false);
    assert!(!e.engineStopped);
    // Orientation: origin (0,0), direction (8,0), normal (0,8) -> dsX == one
    // ramp step per pixel.
    e.point_put(GWPoint1, 0, 0);
    e.point_put(GWPoint2, 8, 0);
    e.point_put(GWPoint3, 0, 8);
    e.loadFillOrientationfromalongnormalwidthheight(fill, GWPoint1, GWPoint2, GWPoint3, 8, 8);
    assert_eq!(e.objat(fill, GFDirectionX), 65536);
    assert_eq!(e.objat(fill, GFNormalY), 65536);
    let external = e.fillSpanfromto(fill as u32, 0, 8);
    assert!(!external);
    for (x, want) in ramp.iter().enumerate() {
        assert_eq!(span[x], *want as u32, "span[{x}]");
    }
    // The exported-fill words the image reads back are recorded.
    assert_eq!(e.wb_at(GWLastExportedFill), fill);
    assert_eq!(e.wb_at(GWLastExportedLeftX), 0);
    assert_eq!(e.wb_at(GWLastExportedRightX), 8);
}

#[test]
fn radial_gradient_fills_both_ramp_directions() {
    let (_wb, span, mut e) = make_engine(2048, 16, 8, 8);
    prepare_span_fill(&mut e, 8);
    let ramp: Vec<i32> = (0..8).map(|i| (0xFF000000u32 | i) as i32).collect();
    let fill = e.allocateGradientFillrampWidthisRadial(PointsRef::from_slice(&ramp), 8, true);
    // Origin at (4,0); one ramp step per pixel in each axis.
    e.point_put(GWPoint1, 4, 0);
    e.point_put(GWPoint2, 8, 0);
    e.point_put(GWPoint3, 0, 8);
    e.loadFillOrientationfromalongnormalwidthheight(fill, GWPoint1, GWPoint2, GWPoint3, 8, 8);
    e.fillRadialGradientfromtoat(fill, 0, 8, 0);
    // Derived by executing the C's arithmetic by hand (see the radial fill
    // functions): the decreasing part runs x in 0..4, the increasing part
    // x in 4..8, with the +-1 ramp-index offsets the C's loop structure
    // produces.
    let want: Vec<u32> = [4, 4, 3, 2, 0, 0, 1, 2]
        .iter()
        .map(|i| 0xFF000000u32 | *i)
        .collect();
    assert_eq!(&span[0..8], &want[..], "radial span");
}

#[test]
fn bitmap_fill_tiles_and_force_fills_alpha_at_depth_32() {
    let (_wb, span, mut e) = make_engine(2048, 16, 8, 8);
    prepare_span_fill(&mut e, 8);
    // A 2x2 32-bit form; pixel B has no alpha and must get 0xFF alpha forced.
    let a = 0xFF0000AAu32 as i32;
    let b = 0x000000BBu32 as i32;
    let c = 0xFF0000CCu32 as i32;
    let d = 0xFF0000DDu32 as i32;
    e.host.forms = vec![vec![a, b, c, d]];
    let fill = e.allocateBitmapFillcolormap(0, None);
    assert!(!e.engineStopped);
    e.objatput(fill, GBBitmapWidth, 2);
    e.objatput(fill, GBBitmapHeight, 2);
    e.objatput(fill, GBBitmapDepth, 32);
    e.objatput(fill, GBBitmapRaster, 2);
    e.objatput(fill, GBBitmapSize, 4);
    e.objatput(fill, GBTileFlag, 1);
    e.objatput(fill, GEObjectIndex, 0);
    e.point_put(GWPoint1, 0, 0);
    e.point_put(GWPoint2, 2, 0);
    e.point_put(GWPoint3, 0, 2);
    e.loadFillOrientationfromalongnormalwidthheight(fill, GWPoint1, GWPoint2, GWPoint3, 2, 2);
    e.fillBitmapSpanfromtoat(fill, 0, 8, 0);
    let b_forced = 0xFF0000BBu32;
    assert_eq!(
        &span[0..8],
        &[a as u32, b_forced, a as u32, b_forced, a as u32, b_forced, a as u32, b_forced]
    );
    // Row 1 samples the second bitmap row.
    e.fillBitmapSpanfromtoat(fill, 0, 4, 1);
    assert_eq!(&span[0..4], &[c as u32, d as u32, c as u32, d as u32]);
}

#[test]
fn merge_fill_copies_and_extends_the_span() {
    let (_wb, span, mut e) = make_engine(2048, 16, 8, 8);
    prepare_span_fill(&mut e, 8);
    let bits = [1i32, 2, 3, 4];
    e.fillBitmapSpanfromto(&bits, 2, 6);
    assert_eq!(&span[2..6], &[1, 2, 3, 4]);
    assert_eq!(e.wb_at(GWSpanEnd), 6);
    assert_eq!(e.wb_at(GWSpanEndAA), 6);
}

// ---------------------------------------------------------------------------
// External entities: the stop-reason protocol
// ---------------------------------------------------------------------------

#[test]
fn an_external_edge_stops_rendering_with_the_get_entry_reason() {
    let (_wb, _span, mut e) = make_engine(2048, 16, 8, 8);
    prepare_render(&mut e, 8, 8);
    // Register an external edge record the way primitiveRegisterExternalEdge
    // does.
    assert!(e.allocateObjEntry(GEBaseEdgeSize));
    let edge = e.objUsed;
    e.objUsed = edge + GEBaseEdgeSize;
    e.objatput(edge, GEObjectType, GEPrimitiveEdge);
    e.objatput(edge, GEObjectLength, GEBaseEdgeSize);
    e.objatput(edge, GEObjectIndex, 7);
    e.objatput(edge, GEXValue, 1);
    e.objatput(edge, GEYValue, 0);
    e.objatput(edge, GEZValue, 0);
    e.objatput(edge, GEFillIndexLeft, FILL);
    e.objatput(edge, GEFillIndexRight, 0);
    e.proceedRenderingScanline();
    assert!(e.engineStopped);
    assert_eq!(e.wb_at(GWStopReason), GErrorGETEntry);
    assert_eq!(e.wb_at(GWState), GEStateWaitingForEdge);
}

// ---------------------------------------------------------------------------
// Fail-fast after a panic mid-mutation of the plugin globals
// ---------------------------------------------------------------------------

/// The `Section` around [`crate::with_globals`], and what deleting it costs.
///
/// This site is not like the others in the tree. There is no mutex here for
/// `std` to poison: [`crate::GLOBALS`] is a bare `UnsafeCell` asserted `Sync`
/// on the single-interpreter-thread contract, exactly as the C left those
/// globals in file scope. So `Section::enter` is not an addition to a mutex's
/// own poison -- it is the *whole* of the fail-fast story, and deleting the one
/// line
///
/// ```ignore
/// let _section = Section::enter();
/// ```
///
/// leaves nothing at all behind it. The SDK proves the mechanism in
/// `pharo-vm-plugin/tests/section_without_a_mutex.rs`; this proves the wiring
/// at the real site, on the real globals, through a real exported primitive.
///
/// The invariant is the one `with_globals`' own doc names: `initialiseModule`
/// writes `loadBBFn` and `copyBitsFn` as a pair and `moduleUnloaded` nulls
/// them as a pair, so a panic between the two leaves one live function pointer
/// into a `dlclose`d BitBltPlugin -- which `loadBitBltFrom` and the engine's
/// span blitter then call. Nothing downstream can tell that pointer from a
/// good one; poisoning the module so no later primitive body runs is the only
/// thing that stops the call.
///
/// This module owns the panic hook for the whole test binary, which is safe
/// because nothing else in `tests.rs` opens a `Section` (no other test calls
/// `with_globals` or any primitive) and because `poison::is_poisoned` is
/// per-cdylib and never cleared -- so this test may set it, and must be the
/// only one that cares.
#[cfg(test)]
mod poison_wiring {
    use core::ffi::c_void;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use pharo_vm_plugin::{poison, sqInt, PrimErr, VirtualMachine};

    /// The code the last refused primitive reported through the proxy.
    static FAILED_WITH: AtomicIsize = AtomicIsize::new(-1);

    unsafe extern "C" fn primitiveFailFor(code: sqInt) -> sqInt {
        FAILED_WITH.store(code, Ordering::SeqCst);
        code
    }

    unsafe extern "C" fn methodReturnReceiver() -> sqInt {
        0
    }

    /// A stand-in for a `BitBltPlugin` entry point `ioLoadFunctionFrom` found.
    const A_LOADED_FN: *const c_void = 0x1000 as *const c_void;

    #[test]
    fn a_panic_inside_with_globals_disables_the_module() {
        // SAFETY: every field is an `Option<fn>`, whose all-zero bit pattern
        // is `None`; only the two entries filled in below are called through,
        // and the gate under test is what guarantees no other one is reached.
        let mut vt: VirtualMachine = unsafe { core::mem::zeroed() };
        vt.primitiveFailFor = Some(primitiveFailFor);
        vt.methodReturnReceiver = Some(methodReturnReceiver);
        let vt: &'static mut VirtualMachine = Box::leak(Box::new(vt));
        // Installs the panic hook, which is what this whole test depends on.
        assert_eq!(
            pharo_vm_plugin::__private::set_interpreter(vt, "B2DPlugin"),
            1
        );

        // --- healthy -------------------------------------------------------
        //
        // The pair as `initialiseModule` leaves it: both entry points found.
        crate::with_globals(|g| {
            g.loadBBFn = A_LOADED_FN;
            g.copyBitsFn = A_LOADED_FN;
        });
        assert!(
            !poison::is_poisoned(),
            "a module whose globals nothing has torn"
        );

        // --- a panic outside every section ---------------------------------
        //
        // No `Section` open, so this is an ordinary primitive failure and the
        // module carries on. Anything else would let one bad argument deep in
        // a rasterizer loop disable the plugin for the rest of the session.
        let outside = std::panic::catch_unwind(|| panic!("nothing is mid-mutation"));
        assert!(outside.is_err());
        assert!(!poison::is_poisoned(), "a panic over nothing is not poison");

        // --- a panic between the two writes --------------------------------
        let inside = std::panic::catch_unwind(|| {
            crate::with_globals(|g| {
                // `moduleUnloaded` nulls the two together; this is the first
                // of them.
                g.loadBBFn = core::ptr::null();
                panic!("the proxy raised an error mid-unload");
            });
        });
        assert!(inside.is_err());

        assert!(
            poison::is_poisoned(),
            "with no mutex to poison, the Section is the only thing that can \
             tell the hook this panic tore something"
        );

        // The globals really are torn, which is what makes the gate matter.
        crate::with_globals(|g| {
            assert!(g.loadBBFn.is_null(), "one half nulled");
            assert_eq!(
                g.copyBitsFn, A_LOADED_FN,
                "and the other still a live pointer into a dlclose'd library"
            );
        });

        // --- so the next primitive fails fast, without running its body ----
        //
        // `primitiveDoProfileStats` is the shortest body that reaches these
        // globals: it reads `doProfileStats`, asks the proxy for the new
        // value, and writes it back. The proxy table above has no
        // `stackObjectValue`, so if the body ran it would panic into the
        // wrapper's fence and report `GenericFailure` -- which is why the code
        // asserted here also proves the gate is *before* the body and not
        // after it.
        let before = crate::with_globals(|g| g.doProfileStats);
        FAILED_WITH.store(-1, Ordering::SeqCst);
        assert_eq!(crate::primitiveDoProfileStats(), 0);
        assert_eq!(
            FAILED_WITH.load(Ordering::SeqCst),
            PrimErr::Unsupported.code(),
            "Unsupported, never NoMemory: the image retries NoMemory after a \
             scavenge and then a full GC, on every single call"
        );
        assert_eq!(
            crate::with_globals(|g| g.doProfileStats),
            before,
            "nothing read or wrote the torn globals"
        );
    }
}
