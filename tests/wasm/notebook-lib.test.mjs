// notebook-lib.test.mjs - unit tests of the libraries of the Notebook tab, in node
//
// usage: node notebook-lib.test.mjs [IMAGE_DIR]
//
// NotebookSt (packaging/emscripten/web/notebook-st.js): scan and balance,
// their hints, a fuzz of 20000 inputs and their time on adversarial inputs
// (linear: 500000 characters of " ' $ # and digits, 250000 of #( and [),
// toSt and fromSt (round trips, a file without markers, method sections),
// straightenQuotes and the example notebook.  The FrameReader and the PNG of
// PharoNotebookKernel (nb-kernel.js): a split at every byte, an LF inside an
// attachment, the resync after a missing LF, the headers it rejects; the
// PNG inflated, its CRCs, the un-premultiplied pixels.  NotebookLib
// (notebook-lib.js): toJson and fromJson, and the limits of an import;
// tidyHtml, which writes the markup of an html output again before the
// browser parses it: the shapes that would nest refused fast, the shallow
// ones that Chromium parses slowly not written, what it writes, a fuzz of
// 20000 inputs whose output nests as written and holds only what rebuild
// may keep, and its time on 4 MB inputs.
//
// With HOST_PHARO set (a native Pharo VM) and IMAGE_DIR an image with its
// .changes and .sources, tests/wasm/st/nb-scan-corpus.st dumps the tokens
// that the image's scanner finds in 5000 of its methods, and scan must agree
// with them: every method balanced, its brackets and its comments where the
// scanner has them.  Otherwise that check says why it is skipped.
// Prints every check and their count, and exits with status 1 if any fails.
// Lane 61 (tests/wasm/lanes/61-notebook-harness.sh) runs it.

import childProcess from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const web = path.join(here, '..', '..', 'packaging', 'emscripten', 'web');
const St = require(path.join(web, 'notebook-st.js'));
const Lib = require(path.join(web, 'notebook-lib.js'));
const Kernel = require(path.join(web, 'nb-kernel.js'));
const imageDir = process.argv[2] ? path.resolve(process.argv[2]) : null;

let failures = 0, passes = 0;
async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
  }
}
const J = x => JSON.stringify(x);
function eq(got, want, what) {
  if (J(got) !== J(want)) throw new Error(`${what}: got ${J(got)}, want ${J(want)}`);
}
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }
const now = () => performance.now();
const enc = s => new TextEncoder().encode(s);
const concat = (...parts) => Buffer.concat(parts.map(p => Buffer.from(typeof p === 'string' ? enc(p) : p)));

// The fields of balance() that the cases give
const bal = s => { const b = St.balance(s); return [b.depth, b.stray, b.mismatch, b.open, b.ok]; };

await check('balance: brackets, strings, comments, characters, symbols, literal and byte arrays', () => {
  for (const [s, want] of [
    ['', [0, 0, 0, false, true]], ['3 + 4', [0, 0, 0, false, true]], ['[ 1', [1, 0, 0, false, false]],
    ['1 ]', [0, 1, 0, false, false]], ['( ]', [0, 0, 1, false, false]], ['{ 1. 2', [1, 0, 0, false, false]],
    ["'abc", [0, 0, 0, true, false]], ['"abc', [0, 0, 0, true, false]], ['$', [0, 0, 0, true, false]],
    ["$' printString", [0, 0, 0, false, true]], ['#($) $] 2) size', [0, 0, 0, false, true]],
    ['#(foo: [ { ) size', [0, 0, 0, false, true]], ['#(1 #(2 (3)) $)) size', [0, 0, 0, false, true]],
    ['#[1 2 255]', [0, 0, 0, false, true]], ['#[1 ( 2]', [0, 0, 1, false, false]],
    ['#at:put: numArgs ]', [0, 1, 0, false, false]], ['#+ )', [0, 1, 0, false, false]],
    ["#'a(b' (", [1, 0, 0, false, false]], ['"a [ ( comment" 1', [0, 0, 0, false, true]],
    ["'it''s' size", [0, 0, 0, false, true]], ['"a ""(""" 1', [0, 0, 0, false, true]], ['$( ( )', [0, 0, 0, false, true]],
    ['[ :x | x ] value: 3', [0, 0, 0, false, true]], ['$\u{1D11E}]', [0, 1, 0, false, false]],
    ['##foo ]', [0, 1, 0, false, false]], ["'(' , ')'", [0, 0, 0, false, true]],
  ]) eq(bal(s), want, J(s));
});

await check('balanceText: the hint under a cell', () => {
  for (const [s, want] of [['', ''], ['3 + 4', ''], ['[ 1', '1 unclosed ['], ['[ [ 1', '2 unclosed ['],
                           ['[ ( 1', '2 unclosed brackets'], ['1 ]', '1 extra ]'], ['1 ] ]', '2 extra ]'],
                           ['1 ] )', '2 extra closers'], ["'abc", 'unclosed string'], ['"x', 'unclosed comment'],
                           ['$', 'a $ with no character after it'], ['( ]', '1 mismatched bracket'],
                           ['( ] [ )', '2 mismatched brackets'], ['#(1 2', '1 unclosed (']])
    eq(St.balanceText(s), want, J(s));
});

await check('scan: the brackets, the line breaks with their depth, the ends of comments', () => {
  const seen = [];
  const r = St.scan('[ "c" (\n1 )\n]', (kind, i, depth) => seen.push(depth === undefined ? [kind, i] : [kind, i, depth]));
  eq(seen, [['open', 0], ['comment', 4], ['open', 6], ['newline', 7, 2], ['close', 10], ['newline', 11, 1], ['close', 12]], 'calls');
  eq([r.depth, r.stray, r.mismatch, r.open], [0, 0, 0, false], 'the end');
  const lit = [];
  St.scan('#(a [ b ]\n) #[1 ]', (kind, i, depth) => lit.push([kind, i, depth]));
  eq(lit, [['open', 1, undefined], ['newline', 9, 1], ['close', 10, undefined], ['open', 13, undefined],
           ['close', 16, undefined]], 'a literal and a byte array');
  const open = [];
  St.scan('"no end', (kind, i) => open.push([kind, i]));
  eq(open, [['comment', 7]], 'a comment without its end');
});

// a balanced piece of Smalltalk, at random
function balanced(rnd, depth = 0) {
  const atoms = ['3', 'x', ' ', '\n', '.', ';', "'a(b'", '"c]"', '$(', '$)', '#foo:', '#+', "#'s['", '#(a [ { )',
                 '#[1 2]', ':=', '^', 'ifTrue:', "''''", '""""', '\u00E9', '\u{1D11E}', '$\u{1D11E}'];
  let s = '';
  const n = 1 + Math.floor(rnd() * 6);
  for (let i = 0; i < n; i++) {
    const k = rnd();
    if (k < 0.25 && depth < 12) {
      const [o, c] = [['(', ')'], ['[', ']'], ['{', '}']][Math.floor(rnd() * 3)];
      s += o + balanced(rnd, depth + 1) + c;
    } else s += atoms[Math.floor(rnd() * atoms.length)];
  }
  return s;
}
function mulberry(seed) {
  return () => {
    seed |= 0; seed = seed + 0x6D2B79F5 | 0;
    let t = Math.imul(seed ^ seed >>> 15, 1 | seed);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

await check('a fuzz of 20000 inputs: balanced ones are ok, any text scans without throwing, the opens and closes nest', () => {
  const rnd = mulberry(61);
  const alphabet = '()[]{}\'"$#:+ \n\tab1.!\u00E9\u2018\u201C';
  for (let k = 0; k < 10000; k++) {
    const s = balanced(rnd);
    const b = St.balance(s);
    assert(b.ok, 'balanced ' + J(s) + ': ' + J(b));
    eq(St.balanceText(s), '', 'the hint of ' + J(s));
  }
  for (let k = 0; k < 10000; k++) {
    let s = '';
    const n = Math.floor(rnd() * 40);
    for (let i = 0; i < n; i++) s += alphabet[Math.floor(rnd() * alphabet.length)];
    let depth = 0, last = -1;
    const r = St.scan(s, (kind, i, d) => {
      assert(i >= last && i <= s.length, 'in order: ' + J(s));
      last = i;
      if (kind === 'open') depth++;
      if (kind === 'close' && depth) depth--;
      if (kind === 'newline') eq(d, depth, 'the depth at a newline of ' + J(s));
    });
    eq(r.depth, depth, 'the depth of ' + J(s));
    const b = St.balance(s);
    eq(b.ok, !r.depth && !r.stray && !r.mismatch && !r.open, 'ok of ' + J(s));
    assert(typeof St.balanceText(s) === 'string' && (St.balanceText(s) === '') === b.ok, 'the hint of ' + J(s));
  }
});

// The best of 3 runs of f, in ms
function best(f) {
  let t = Infinity;
  for (let k = 0; k < 3; k++) { const t0 = now(); f(); t = Math.min(t, now() - t0); }
  return t;
}
await check('scan is linear: 500000 characters of " \' $ # and digits, 250000 of #( and [', () => {
  const inputs = [['"', 500000], ["'", 500000], ['$', 500000], ['#', 500000], ['7', 500000], ['#(', 125000], ['[', 250000],
                  ['"\'', 250000], ['$\'', 250000], ['#$', 250000]];
  const times = [];
  for (const [unit, n] of inputs) {
    const big = unit.repeat(n), half = unit.repeat(n >> 1);
    const t = best(() => St.balance(big)), h = best(() => St.balance(half));
    best(() => St.balanceText(big));
    times.push(`${J(unit)} ${t.toFixed(0)} ms`);
    assert(t < 1500, `${J(unit)} x ${n}: ${t.toFixed(0)} ms`);
    assert(t < 4 * h + 20, `${J(unit)} x ${n}: ${t.toFixed(0)} ms, half of it ${h.toFixed(0)} ms`);
  }
  console.log('#   ' + times.join(', '));
});

const sample = {
  title: 'A "quoted" title! with a bang',
  cells: [
    { type: 'markdown', source: '# Hi\n\nSay "hi"! and ""twice""' },
    { type: 'code', source: '3 + 4' },
    { type: 'code', source: "Transcript show: 'bang!'; cr.\n\"a comment!\"\n1" },
    { type: 'code', source: '"%%" a marker in a code cell\n2' },
    { type: 'code', source: '"%% [markdown] not markdown" 3' },
    { type: 'markdown', source: 'x' },
    { type: 'code', source: '| t |\n\tt := 3.\n\tt' },
    { type: 'code', source: '' },
  ],
};
const shape = nb => ({ title: nb.title, cells: nb.cells.map(c => ({ type: c.type, source: c.source })) });

await check('toSt and fromSt: the chunk format, round trips, CRs', () => {
  const st = St.toSt(sample);
  assert(st.startsWith('"Pharo notebook: A ""quoted"" title!! with a bang"!\n'), 'the header: ' + J(st.slice(0, 60)));
  assert(st.includes('"%%"\n3 + 4!\n') && st.includes('"%% [markdown]\n# Hi\n\nSay ""hi""!! and """"twice"""""!'),
         'the cells: ' + J(st));
  const back = St.fromSt(st);
  eq(shape(back), shape(sample), 'the round trip');
  eq(back.warnings, [], 'warnings');
  eq(shape(St.fromSt(st.replace(/\n/g, '\r\n'))), shape(sample), 'with CRLF');
  eq(shape(St.fromSt(st.replace(/\n/g, '\r'))), shape(sample), 'with CR');
  const ex = St.example();
  eq(shape(St.fromSt(St.toSt(ex))), shape(ex), 'the example');
  eq(St.toSt({ title: 'two\nlines', cells: [] }).split('\n')[0], '"Pharo notebook: two lines"!', 'a title of one line');
});

await check('fromSt: a file without markers, DoIts with a !, a preamble, method sections left out with a warning', () => {
  eq(shape(St.fromSt('3 + 4!\n\n100 factorial printString size!\n')),
     { title: '', cells: [{ type: 'code', source: '3 + 4' }, { type: 'code', source: '100 factorial printString size' }] },
     'two DoIts');
  eq(St.fromSt("Transcript show: 'a!!b'!").cells.map(c => c.source), ["Transcript show: 'a!b'"], 'a doubled !');
  const m = St.fromSt("Object subclass: #Foo!\n!Foo methodsFor: 'x'!\nbar ^ 1! !\n!Foo class methodsFor: 'y'!\nbaz ^ 2!\nqux ^ 3! !\n" +
                      "Foo comment: 'c'!\n!Foo commentStamp: 'x' prior: 0!\nA comment!\n3 + 4!");
  eq(m.cells.map(c => c.source), ['Object subclass: #Foo', "Foo comment: 'c'", '3 + 4'], 'the DoIts');
  eq(m.warnings.length, 1, 'one warning');
  assert(/3 method definitions and 1 class comment were left out/.test(m.warnings[0]), 'warning ' + m.warnings[0]);
  eq(St.fromSt('"%%"\n!').cells, [{ type: 'code', source: '' }], 'an empty code cell');
  eq(St.fromSt('"Pharo notebook: T"!\n"%% [markdown]\nx"! "%%" y!').title, 'T', 'the title');
});

await check('straightenQuotes: curly quotes outside strings and comments, and after $', () => {
  for (const [s, want] of [['\u2018abc\u2019', "'abc'"], ['\u201Cc\u201D 1', '"c" 1'], ["'it\u2019s'", "'it\u2019s'"],
                           ['"say \u201Chi\u201D"', '"say \u201Chi\u201D"'], ['$\u2018 printString', "$' printString"],
                           ['$\u201C', '$"'], ['x := \u2018a\u2019, \u2018b\u2019', "x := 'a', 'b'"], ['plain', 'plain'],
                           ['\u2018it\u2019s\u2019', "'it's'"]])
    eq(St.straightenQuotes(s), want, J(s));
});

await check('the example: 13 cells, its fences and labels', () => {
  const ex = St.example();
  eq(ex.cells.length, 13, 'cells');
  eq(ex.cells.map(c => c.type[0]).join(''), 'mccccmccccccc', 'the types');
  for (const c of ex.cells) if (c.type === 'code') assert(St.balance(c.source).ok, 'balanced: ' + J(c.source.slice(0, 60)));
  assert(/#\(1 2 3\) at: 5/.test(ex.cells[11].source), 'the error cell');
  for (const f of ['', 'smalltalk', 'ST', 'Pharo']) assert(St.FENCE.test(f), 'fence ' + J(f));
  for (const f of ['scheme', 'js', 'smalltalk2']) assert(!St.FENCE.test(f), 'not a fence ' + J(f));
  eq(St.LANGUAGE, 'Smalltalk', 'LANGUAGE');
});

// ---- the frames of /dev/nbevents

const frames = chunks => {
  const out = [], errors = [];
  const r = new Kernel.FrameReader((ev, att) => out.push([ev, att ? Array.from(att) : null]), t => errors.push(t));
  for (const c of chunks) r.push(new Uint8Array(c));
  return { out, errors };
};
const line = o => J(o) + '\n';

await check('FrameReader: events and an attachment with LFs inside, split at every byte', () => {
  const att = [10, 1, 10, 255, 0, 10, 10, 13];
  const all = concat(line({ ev: 'hello', proto: 1 }),
                     line({ ev: 'display', rid: 1, mime: 'image/x-pharo-bgra', width: 2, height: 1, premultiplied: false, id: null, bytes: 8 }),
                     Buffer.from(att), '\n', line({ ev: 'done', rid: 1, text: '\u00E9\u{1D11E}' }));
  const want = frames([all]);
  eq(want.errors, [], 'errors');
  eq(want.out.map(([e, a]) => [e.ev, a]), [['hello', null], ['display', att], ['done', null]], 'the frames');
  eq(want.out[2][0].text, '\u00E9\u{1D11E}', 'UTF-8');
  for (let k = 0; k <= all.length; k++) eq(frames([all.subarray(0, k), all.subarray(k)]), want, 'split at ' + k);
  eq(frames([...all].map(b => [b])), want, 'a byte at a time');
  // an attachment of 0 bytes
  eq(frames([concat(line({ ev: 'display', mime: 'image/png', bytes: 0 }), '\n', line({ ev: 'x' }))]).out.map(([e, a]) => [e.ev, a]),
     [['display', []], ['x', null]], 'an empty attachment');
});

await check('FrameReader: a missing LF is an error, and it resyncs at the next LF', () => {
  const r = frames([concat(line({ ev: 'display', mime: 'image/png', bytes: 2 }), Buffer.from([1, 2]), 'Xjunk\n',
                           line({ ev: 'done', rid: 3 }))]);
  eq(r.out.map(([e]) => e.ev), ['done'], 'the frames');
  eq(r.errors.length, 1, 'one error: ' + J(r.errors));
  assert(/no LF/.test(r.errors[0]), 'the error ' + r.errors[0]);
});

await check('FrameReader: headers it rejects, skipping their attachments; lines that are not events', () => {
  const cases = [
    [{ ev: 'display', mime: 'image/x-pharo-bgra', width: 1e6, height: 1, bytes: 4 }, 4],
    [{ ev: 'display', mime: 'image/x-pharo-bgra', width: 2, height: 2, bytes: 15 }, 15],
    [{ ev: 'display', mime: 'image/x-pharo-bgra', width: 0, height: 1, bytes: 0 }, 0],
    [{ ev: 'display', mime: 'image/x-pharo-bgra', width: 1.5, height: 2, bytes: 12 }, 12],
    [{ ev: 'display', mime: 'image/x-pharo-bgra', width: 16385, height: 1, bytes: 65540 }, 65540],
    [{ ev: 'display', mime: 'image/png', bytes: Kernel.MAX_ATTACHMENT + 1 }, Kernel.MAX_ATTACHMENT + 1],
  ];
  for (const [header, n] of cases) {
    const r = frames([concat(line(header), Buffer.alloc(n, 10), '\n', line({ ev: 'done' }))]);
    eq(r.out.map(([e]) => e.ev), ['done'], J(header) + ': the frames');
    eq(r.errors.length, 1, J(header) + ': errors ' + J(r.errors));
  }
  // the largest accepted attachment
  const big = frames([concat(line({ ev: 'display', mime: 'image/png', bytes: Kernel.MAX_ATTACHMENT }),
                             Buffer.alloc(Kernel.MAX_ATTACHMENT), '\n')]);
  eq([big.errors, big.out.length, big.out[0][1].length], [[], 1, Kernel.MAX_ATTACHMENT], 'the cap itself');
  // nothing to skip by: the next line is read
  for (const bad of [{ ev: 'display', bytes: -1 }, { ev: 'display', bytes: 'x' }, { ev: 'display', bytes: 1.5 }]) {
    const r = frames([concat(line(bad), line({ ev: 'done' }))]);
    eq([r.out.map(([e]) => e.ev), r.errors.length], [['done'], 1], J(bad));
  }
  for (const bad of ['not json', '[1,2]', '{"no":"ev"}', '{"ev":3}', 'null']) {
    const r = frames([concat(bad + '\n', line({ ev: 'done' }))]);
    eq([r.out.map(([e]) => e.ev), r.errors.length], [['done'], 1], J(bad));
  }
  eq(frames([concat('\n\n', line({ ev: 'done' }))]).errors, [], 'empty lines');
});

// A PNG inflated: {width, height, rgba}, with the CRC of every chunk
// checked by zlib's own
function inflatePng(png) {
  const b = Buffer.from(png);
  eq([...b.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10], 'the signature');
  let at = 8, width = 0, height = 0;
  const idat = [], types = [];
  while (at < b.length) {
    const len = b.readUInt32BE(at), type = b.toString('latin1', at + 4, at + 8);
    types.push(type);
    eq(b.readUInt32BE(at + 8 + len), zlib.crc32(b.subarray(at + 4, at + 8 + len)), 'the CRC of ' + type);
    eq(Kernel.crc32(b.subarray(at + 4, at + 8 + len)), zlib.crc32(b.subarray(at + 4, at + 8 + len)), 'crc32 of ' + type);
    if (type === 'IHDR') { width = b.readUInt32BE(at + 8); height = b.readUInt32BE(at + 12); eq([b[at + 16], b[at + 17]], [8, 6], 'RGBA 8'); }
    if (type === 'IDAT') idat.push(b.subarray(at + 8, at + 8 + len));
    at += 12 + len;
  }
  eq(types, ['IHDR', 'IDAT', 'IEND'], 'the chunks');
  const raw = zlib.inflateSync(Buffer.concat(idat)), rgba = [];
  eq(raw.length, (4 * width + 1) * height, 'the size');
  for (let y = 0; y < height; y++) {
    eq(raw[y * (4 * width + 1)], 0, 'filter of row ' + y);
    rgba.push(...raw.subarray(y * (4 * width + 1) + 1, (y + 1) * (4 * width + 1)));
  }
  return { width, height, rgba };
}

await check('bgraToPng: B and R swapped, the CRCs, zlib; premultiplied pixels divided by their alpha', async () => {
  // 3 x 2: red, half red (straight), transparent / green, blue, white
  const bgra = [0, 0, 255, 255, 0, 0, 255, 128, 0, 0, 0, 0, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255];
  const png = await Kernel.pngOf(3, 2, new Uint8Array(bgra), false);
  const img = inflatePng(png);
  eq([img.width, img.height], [3, 2], 'the size');
  eq(img.rgba, [255, 0, 0, 255, 255, 0, 0, 128, 0, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255], 'the pixels');
  eq(await Kernel.bgraToPng(3, 2, new Uint8Array(bgra), false), Buffer.from(png).toString('base64'), 'base64');
  // premultiplied: half red is 128 0 0 128, a = 0 stays 0, opaque stays
  const pre = [0, 0, 128, 128, 0, 64, 0, 64, 9, 9, 9, 0, 1, 2, 3, 255];
  eq(inflatePng(await Kernel.pngOf(4, 1, new Uint8Array(pre), true)).rgba,
     [255, 0, 0, 128, 0, 255, 0, 64, 9, 9, 9, 0, 3, 2, 1, 255], 'un-premultiplied');
  eq(inflatePng(await Kernel.pngOf(1, 1, new Uint8Array([200, 200, 200, 100]), true)).rgba, [255, 255, 255, 100], 'clamped to 255');
  let e = null;
  try { await Kernel.pngOf(2, 2, new Uint8Array(15), false); } catch (x) { e = x; }
  assert(e, 'a wrong size throws');
  const w = 300, h = 200, big = new Uint8Array(4 * w * h).map((_, i) => (i * 7) & 255);
  const bi = inflatePng(await Kernel.pngOf(w, h, big, false));
  assert(bi.rgba.every((v, i) => v === big[(i & ~3) + [2, 1, 0, 3][i & 3]]), 'a 300 x 200 image');
});

// ---- the JSON format

await check('toJson and fromJson: a round trip; ids, counts and statuses kept or cleaned', () => {
  const nb = { title: 'T', created: '2026-10-06T00:00:00.000Z', modified: null, pharo: '12.0', cells: [
    { id: 'a1', type: 'code', source: '3 + 4', count: 1, status: 'ok',
      outputs: [{ k: 'value', text: '7' }, { k: 'stream', name: 'stdout', text: 'x' }] },
    { id: 'a2', type: 'code', source: 'nil foo', count: 2, status: 'error',
      outputs: [{ k: 'error', error: { text: 'Error: x', kind: ['MessageNotUnderstood'], location: null, form: 1, line: 1, column: 5,
                                       chain: [{ where: 'In[2]:1', proc: null, form: 'nil foo' }] } }] },
    { id: 'a3', type: 'markdown', source: '# x' },
  ] };
  const text = Lib.toJson(nb), j = JSON.parse(text);
  eq([j.format, j.version, j.meta.pharo], ['pharo-notebook', 1, '12.0'], 'the format');
  const back = Lib.fromJson(text);
  eq(back.cells, [{ id: 'a1', type: 'code', source: '3 + 4', count: 1, outputs: nb.cells[0].outputs },
                  { id: 'a2', type: 'code', source: 'nil foo', count: 2, status: 'error', outputs: nb.cells[1].outputs },
                  { id: 'a3', type: 'markdown', source: '# x' }], 'the cells');
  eq([back.title, back.pharo, back.created, back.warnings], ['T', '12.0', nb.created, []], 'the meta');
  const dup = Lib.fromJson(J({ format: 'pharo-notebook', version: 2, cells: [
    { id: 'same', type: 'code', source: '1' }, { id: 'same', type: 'code', source: '2' }, { id: '<bad>', type: 'code', source: '3' },
    { type: 'raw', source: 'x' }, { type: 'code', source: 7 },
    { type: 'code', source: '4', outputs: [{ k: 'display', mime: 'text/javascript', data: 'x' }, { k: 'stream', name: 'stdin', text: 'x' },
                                           { k: 'display', mime: 'image/png', data: 'AA==', id: 'p' }] }] }));
  eq(dup.cells.map(c => c.source), ['1', '2', '3', '4'], 'the valid cells');
  eq(new Set(dup.cells.map(c => c.id)).size, 4, 'distinct ids');
  assert(dup.cells.every(c => /^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(c.id)), 'clean ids');
  eq(dup.cells[3].outputs, [{ k: 'display', mime: 'image/png', data: 'AA==', id: 'p' }], 'outputs cleaned');
  eq(dup.warnings.length, 2, 'warnings ' + J(dup.warnings));
  assert(/newer version/.test(dup.warnings[0]) && /2 invalid cells were skipped/.test(dup.warnings[1]), 'warnings ' + J(dup.warnings));
});

await check('fromJson: the limits of an import (5 MB, 5000 cells, 1000 outputs), and what is not a notebook', () => {
  const thrown = (text, re, opts) => {
    let e = null;
    try { Lib.fromJson(text, opts); } catch (x) { e = x; }
    assert(e && re.test(e.message), J(String(text).slice(0, 40)) + ': ' + (e && e.message));
  };
  eq(Lib.LIMITS, { file: 5 * 1024 * 1024, cells: 5000, source: 5 * 1024 * 1024, outputs: 1000 }, 'LIMITS');
  thrown('x'.repeat(5 * 1024 * 1024 + 1), /larger than 5 MB/);
  thrown('{', /not valid JSON/);
  thrown(J({ format: 'chicken-notebook', cells: [] }), /not a Pharo notebook/);
  thrown(J({ format: 'pharo-notebook' }), /not a Pharo notebook/);
  thrown(J({ format: 'pharo-notebook', cells: Array.from({ length: 5001 }, () => ({ type: 'code', source: '' })) }), /more than 5000 cells/);
  eq(Lib.fromJson(J({ format: 'pharo-notebook', cells: Array.from({ length: 5000 }, () => ({ type: 'code', source: '' })) })).cells.length,
     5000, '5000 cells');
  const outs = Array.from({ length: 1500 }, (_, i) => ({ k: 'stream', name: 'stdout', text: String(i) }));
  const cut = Lib.fromJson(J({ format: 'pharo-notebook', cells: [{ type: 'code', source: '1', outputs: outs }] }));
  const o = cut.cells[0].outputs;
  eq(o.length, 1000, 'outputs kept');
  eq([o[0].text, o[997].text, o[998].k, o[999].text], ['0', '997', 'note', '1499'], 'the first, a note, the last');
  assert(/1 cell had more than 1000 outputs/.test(cut.warnings[0]), 'warning ' + J(cut.warnings));
  eq(Lib.fromJson(J({ format: 'pharo-notebook', cells: [{ type: 'code', source: '1', outputs: outs }] }), { limits: false })
       .cells[0].outputs.length, 1500, 'no limits for the page\'s own');
  eq(Lib.limitOutputs([1, 2, 3], 3), [1, 2, 3], 'limitOutputs under the limit');
});

// ---- tidyHtml, which writes the markup that the browser's parser reads

// The elements of what tidyHtml wrote, checked: every end tag closes the
// element open last, a void or self-closed one is <x/> or <x ...></x>,
// attributes are double-quoted, no < but of a tag or an empty comment.
// Answers the depth it reaches
const WRITTEN = /<(\/?)([^\t\n\f \/>]+)((?: [^\t\n\f \/>"'<=\0]+="[^"]*")*)(\/?)>|<!---->|([^<]+)|(<)/y;
function nestsAsWritten(out) {
  const stack = [];
  let max = 0;
  WRITTEN.lastIndex = 0;
  while (WRITTEN.lastIndex < out.length) {
    const at = WRITTEN.lastIndex, m = WRITTEN.exec(out);
    if (!m || m[6]) throw new Error('not a tag at ' + at + ': ' + J(out.slice(at, at + 40)));
    if (m[5] !== undefined || m[0] === '<!---->') continue;
    if (m[1]) {
      const open = stack.pop();
      if (open !== m[2]) throw new Error('</' + m[2] + '> closes ' + open + ' at ' + at);
    } else if (!m[4]) max = Math.max(max, stack.push(m[2]));
  }
  return max;
}
// and every element and attribute written is one that rebuild may keep (or
// an element it unwraps, with no attributes), none it drops with its content
const SVG_OK = new Set(('svg g defs symbol use path rect circle ellipse line polyline polygon text tspan textpath title desc ' +
  'lineargradient radialgradient stop clippath mask pattern marker filter a switch').split(' '));
const DROPPED = new Set(('script style template noscript iframe frame frameset object embed applet form input button select ' +
  'textarea option optgroup datalist output label fieldset legend dialog math link meta base title head audio video source ' +
  'track picture canvas map area portal slot noembed noframes xmp plaintext marquee bdi').split(' '));
const ATTR = / ([^\t\n\f \/>"'<=\0]+)="/g;
function writtenOk(out) {
  const svg = [];
  for (const m of out.matchAll(/<(\/?)([^\t\n\f \/>!]+)([^>]*?)(\/?)>/g)) {
    const [, end, name, attrs, self] = m;
    if (end) { svg.pop(); continue; }
    const inSvg = name === 'svg' || svg.length > 0 && svg[svg.length - 1];
    if (inSvg) assert(SVG_OK.has(name) || /^fe/.test(name), 'an SVG element rebuild drops: ' + m[0]);
    else assert(!DROPPED.has(name), 'an element rebuild drops: ' + m[0]);
    for (const [, a] of attrs.matchAll(ATTR))
      assert(!/^on|^class$|^dir$|^name$|^value$/.test(a) || a === 'value' && /^(li|progress|meter)$/.test(name),
             'an attribute rebuild drops: ' + m[0]);
    if (!self && !/^(br|col|hr|img|image|wbr|basefont|bgsound|keygen|param)$/.test(name)) svg.push(inSvg);
  }
}
const tidyError = s => { try { Lib.tidyHtml(s); return null; } catch (e) { return e.message; } };

await check('tidyHtml: the shapes that the parser nests are refused at once: <rt>, <li><dd>, <div>, <div/>, <div></x>, ...', () => {
  let formatting = '<p>';
  for (let i = 0; i < 3000; i++) formatting += '<b id=f' + i + '>';
  const deep = {
    div: '<div>'.repeat(100000), rt: '<rt>'.repeat(100000), 'li dd': '<li><dd>'.repeat(50000), 'div/': '<div/>'.repeat(100000),
    'div /x': '<div></x>'.repeat(100000), 'div title': '<div title="</div>">'.repeat(50000),
    'div comment': '<div><!--</div>-->'.repeat(50000), 'svg area': '<svg>' + '<area>'.repeat(100000),
    'svg style': '<svg><style>' + '<div>'.repeat(100000), 'table td': '<table><td>'.repeat(50000),
  };
  for (const [k, src] of Object.entries(deep)) {
    const t = now(), e = tidyError(src), ms = now() - t;
    assert(e === 'the HTML nests more than 4096 elements deep' && ms < 300, k + ': ' + e + ' in ' + ms.toFixed(0) + ' ms');
  }
  // deep, then much at that depth: the parser's work is the sum of the depths
  const work = {
    'span hr': '<span>'.repeat(4000) + '<hr>'.repeat(1000000), 'reopened b': formatting + '<div>x</div>'.repeat(100000),
    'a large reopened b': '<p><b title="' + 'x'.repeat(1000000) + '">' + '<div>x</div>'.repeat(1000),
  };
  for (const [k, src] of Object.entries(work)) {
    const t = now(), e = tidyError(src), ms = now() - t;
    assert(e === 'the HTML is too large for how deeply it nests' && ms < 300, k + ': ' + e + ' in ' + ms.toFixed(0) + ' ms');
  }
  eq(Lib.MAX_NEST, 4096, 'MAX_NEST');
  // (rebuild shows 65 levels: what is deeper is not written)
  eq(nestsAsWritten(Lib.tidyHtml('<div>'.repeat(4096) + 'x')), 65, '4096 <div> are fine, and 65 are written');
  eq(tidyError('<div>'.repeat(4097)), 'the HTML nests more than 4096 elements deep', '4097 are not');
  eq(nestsAsWritten(Lib.tidyHtml('<p>a<li>b<td>c'.repeat(20000))), 2, 'unclosed <p>, <li> and <td> do not nest');
  eq(nestsAsWritten(Lib.tidyHtml('<ruby>' + '<rt>a'.repeat(20000))), 2, '<rt> inside a <ruby> do not nest');
  eq(nestsAsWritten(Lib.tidyHtml('<dl>' + '<dt>a<dd>b'.repeat(20000))), 2, '<dt> and <dd> inside a <dl> do not nest');
});

// Chromium's parser takes time quadratic in the children of some elements,
// however shallow: an <option> or a <fieldset> (rebuild drops them), a <bdi>
// or a dir=auto element (it works their direction out again at each child)
await check('tidyHtml: what Chromium parses in quadratic time even shallow is not written: <option>, <fieldset>, <bdi>, dir', () => {
  const flat = {
    'option br': '<option>' + 't<br>'.repeat(40000), 'option x': '<option>' + '<x>t</x>'.repeat(40000),
    'fieldset br': '<fieldset>' + 't<br>'.repeat(40000), 'option a table': '<option><a id=qK><table>t'.repeat(12000),
    'canvas': '<canvas>' + 't<!---->'.repeat(40000), 'select': '<select>' + '<option>t'.repeat(40000),
  };
  for (const [k, src] of Object.entries(flat)) {
    const t = now(), out = Lib.tidyHtml(src), ms = now() - t;
    assert(out === '' && ms < 300, k + ': ' + J(out.slice(0, 80)) + ' in ' + ms.toFixed(0) + ' ms');
  }
  const bdi = Lib.tidyHtml('<bdi>' + '<span dir=auto>t</span>'.repeat(20000));
  assert(!/<bdi|\sdir=/.test(bdi) && nestsAsWritten(bdi) === 2, 'a <bdi> and dir: ' + J(bdi.slice(0, 80)));
});

await check('tidyHtml writes the markup again: implied end tags, formatting reopened, foreign content, what it leaves out', () => {
  const cases = [
    ['<p>a<p>b', '<p>a</p><p>b'],
    ['<ul><li>a<li>b</ul>', '<ul><li>a</li><li>b</li></ul>'],
    ['<table><tr><td>1<td>2<tr><td>3</table>', '<table><tr><td>1</td><td>2</td></tr><tr><td>3</td></tr></table>'],
    ['<b><i>x</b>y</i>', '<b><i>x</i></b><i>y</i>'],
    ['<b>1<p>2</b>3</p>4', '<b>1<p>23</p></b>4'],
    ['<a><a>x', '<a></a><a>x'],
    ['<div/>x', '<div>x'],
    ['<div></x>x', '<div>x'],
    ['x</p>y', 'x<p></p>y'],
    ['<br></br>', '<br/><br/>'],
    ['<td>c<html><body>d', 'cd'],
    ['<svg><rect/><g><path d="M0"/></g></svg>z', '<svg><rect></rect><g><path d="M0"></path></g></svg>z'],
    ['<svg><div>x</div></svg>', '<svg></svg><div>x</div>'],
    ['<svg><title>t<b>u</b></title><![CDATA[a<b&c]]></svg>', '<svg><title>t</title>a&lt;b&amp;c</svg>'],
    ["<a href=x title='a\"b' c=<d>q</a>", '<a href="x" title="a&quot;b">q</a>'],
    ['<div title="</div>">x', '<div title="&lt;/div>">x'],
    ['<span =y z title id=1 id=2>', '<span title="" id="1">'],
    ['<td id="a<b" lang="x">', ''],
    // what rebuild drops with its content is left out with it (MathML, an
    // SVG element it does not know, HTML in SVG), so is what nests deeper
    // than it shows; the elements it unwraps are written without attributes,
    // and those that are ordinary ones to the parser as <nb-w>
    ['<math><mi><b>x</b></mi></math>y', 'y'],
    ['<option>a<br>b</option>c<fieldset>t<br>', 'c'],
    ['<option><b>x</option>y', '<b>y'],
    ['<p>a<form>b<input value=1>c</form>d<select><option>e</select>f', '<p>a</p>df'],
    ['<form>a<form>b</form>c</form>d', 'cd'],
    ['a&amp<option>x</option>;b', 'a&amp<!---->;b'],
    ['<svg><foreignObject><b>x</b></foreignObject><animate/><rect onclick=f width=2 viewBox=v /></svg>',
     '<svg><rect width="2" viewbox="v"></rect></svg>'],
    ['<details name=a open ontoggle=f><summary>s</summary>d</details>', '<details open=""><summary>s</summary>d</details>'],
    ['<foo bar=1>x<div>y</div></foo><listing id=l>z</listing><nobr class=c>n</nobr>',
     '<nb-w>x<div>y</div></nb-w><listing>z</listing><nobr>n</nobr>'],
    ['<p dir=rtl>x</p><bdi dir=ltr id=b>y</bdi><bdi>z</bdi>',
     '<p data-nb-dir="rtl">x</p><span data-nb-bdi="" data-nb-dir="ltr" id="b">y</span><span data-nb-bdi="">z</span>'],
    ['<div>'.repeat(70) + 'deep<p>er', '<div>'.repeat(65)],
    ['a<!-- c -->b<?pi?>c<!doctype html>d</3>e<!-->f', 'abcdef'],
    ['&am<!---->p;', '&am<!---->p;'],
    ['<style>x</style>y<script>if (a</b) "</script>"</script>z<textarea><b></textarea>t', 'y"zt'],
    ['<base href=x><p>b</p><plaintext><div>', '<p>b</p>'],
    ['a\r\nb\rc', 'a\nb\nc'],
    ['<p>1 < 2 & 3 > 2</p>', '<p>1 &lt; 2 & 3 > 2</p>'],
  ];
  for (const [src, want] of cases) eq(Lib.tidyHtml(src), want, J(src));
});

await check('tidyHtml: a fuzz of 20000 inputs: what it writes nests as written, within MAX_NEST, and tidies to itself', () => {
  const toks = ['<div>', '</div>', '<p>', '</p>', '<li>', '<dd>', '<dt>', '<rt>', '<ruby>', '</ruby>', '<b>', '</b>', '<i>', '</i>',
    '<a href=x>', '</a>', '<table>', '<tr>', '<td>', '</td>', '</tr>', '</table>', '<svg>', '</svg>', '<rect/>', '<area>', '<math>',
    '<mi>', '</mi>', '<!--', '-->', '<![CDATA[', ']]>', '<style>', '</style>', '<script>', '</script>', '<textarea>', '<', '>', '"',
    "'", '=', 'x', ' ', '&amp', '&', ';', '<div title="', '<br>', '</br>', '<plaintext>', '<select>', '<option>', '<template>',
    '</template>', '<foreignObject>', '</foreignObject>', '<button>', '<form>', '</form>', '<h1>', '<h2>', '</h1>', '<nobr>',
    '<object>', '</object>', '<x/>', '</x>', '<?', '</3', '<!doctype html>', '<caption>', '<col>', '<colgroup>', '<base href=y>',
    '<frameset>', '<body>', '\r\n', '<image>', '<font color=red>', '</font>', '<annotation-xml encoding="text/html">', '<title>',
    '</title>', '<ul>', '</ul>', '<span>', '</span>', '<bdi>', '</bdi>', '<p dir=auto>', '<fieldset>', '</fieldset>', '<legend>',
    '<details name=a open>', '<foo x=1>', '</foo>', '<animate/>', '<listing>', '<label>', '<input>', '<math>', '<desc>',
    '<rect onclick=x width=1/>', '<div dir=rtl title=t onclick=x>'];
  let seed = 11;
  const rnd = k => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % k; };
  for (let k = 0; k < 20000; k++) {
    let src = '';
    for (let j = 1 + rnd(40); j > 0; j--) src += toks[rnd(toks.length)];
    let out;
    try {
      out = Lib.tidyHtml(src);
      assert(nestsAsWritten(out) <= 65, 'depth');
      writtenOk(out);
      eq(Lib.tidyHtml(out), out, 'tidied again');
    } catch (e) { throw new Error(J(src) + ' -> ' + J(out) + ': ' + e.message); }
  }
});

await check('tidyHtml is linear: 4 MB of <, of comments, of unclosed tags, attributes and quotes; a table of 400000 tags', () => {
  const inputs = {
    '<': '<'.repeat(4000000), 'comments': '<!-- -->'.repeat(500000), 'unclosed comments': '<!--'.repeat(1000000),
    'bogus': '<?'.repeat(2000000), 'unclosed tag': '<a ' + 'b '.repeat(2000000), 'quotes': '<a b="' + '\''.repeat(4000000),
    'attributes': '<a ' + Array.from({ length: 400000 }, (_, i) => 'a' + i + '=1').join(' ') + '>',
    'text': 'word '.repeat(800000), '&': '&amp'.repeat(1000000),
    'table': '<table>' + '<tr><td>1</td><td><b>2</b></td></tr>'.repeat(50000) + '</table>',
  };
  for (const [k, src] of Object.entries(inputs)) {
    const t = now();
    let e = null;
    try { nestsAsWritten(Lib.tidyHtml(src)); } catch (x) { e = x.message; }
    const ms = now() - t;
    assert(!e && ms < 3000, k + ': ' + e + ' in ' + ms.toFixed(0) + ' ms');
  }
});

// ---- the scanner of the image

async function corpus() {
  const host = process.env.HOST_PHARO;
  if (!host || !fs.existsSync(host)) { console.log('# skip the corpus of the image: no HOST_PHARO'); return; }
  if (!imageDir) { console.log('# skip the corpus of the image: no IMAGE_DIR'); return; }
  await check('scan agrees with the scanner of the image on 5000 methods: balanced, brackets and comments where it has them', () => {
    const dir = fs.mkdtempSync(path.join(process.env.TEST_DIR || os.tmpdir(), 'nb-corpus-'));
    try {
      const names = fs.readdirSync(imageDir);
      const image = names.find(n => n.endsWith('.image')), sources = names.find(n => n.endsWith('.sources'));
      fs.copyFileSync(path.join(imageDir, image), path.join(dir, 'Pharo.image'));
      fs.copyFileSync(path.join(imageDir, image.replace(/\.image$/, '.changes')), path.join(dir, 'Pharo.changes'));
      fs.symlinkSync(path.join(imageDir, sources), path.join(dir, sources));
      const out = path.join(dir, 'corpus.jsonl');
      const t0 = now();
      const r = childProcess.spawnSync(host, ['--headless', path.join(dir, 'Pharo.image'), '--no-default-preferences', 'st',
                                              '--no-source', '--quit', path.join(here, 'st', 'nb-scan-corpus.st')],
                                       { cwd: dir, env: Object.assign({}, process.env, { NB_SCAN_OUT: out }),
                                         encoding: 'utf8', timeout: 300000 });
      const said = (r.stderr || '').split('\n').filter(l => /^nb-scan-corpus:/.test(l));
      assert(fs.existsSync(out), 'no corpus: status ' + r.status + ' ' + (r.stderr || '').slice(-400));
      const lines = fs.readFileSync(out, 'utf8').split('\n').filter(l => l.startsWith('['));
      assert(lines.length >= 4000, lines.length + ' methods');
      let unbalanced = [], brackets = [], comments = [];
      for (const l of lines) {
        const [name, source, tokens] = JSON.parse(l);
        // the code points of the source (the scanner's positions, 1-based)
        // as the indices of its UTF-16 units
        const at = [];
        for (let i = 0; i < source.length; i++) {
          at.push(i);
          const c = source.charCodeAt(i);
          if (c >= 0xd800 && c < 0xdc00 && i + 1 < source.length) i++;
        }
        const u = p => at[p - 1];
        if (!St.balance(source).ok) unbalanced.push(name);
        // the brackets of the scanner, those of literal arrays as Pharo's
        // parser reads them (only ( and ) there) and of byte arrays (only ])
        const stack = [], want = [], gotB = [], wantC = [], gotC = [];
        for (const [kind, start, stop, value] of tokens) {
          if (kind === 'comment') { wantC.push(u(stop)); continue; }
          if (kind === 'litarray') { stack.push(source[u(stop)] === '(' ? 'L' : 'B'); want.push(['open', u(stop)]); continue; }
          if (kind !== 'special' || !'()[]{}'.includes(value)) continue;
          const top = stack[stack.length - 1];
          if (top === 'B') { if (value === ']') { stack.pop(); want.push(['close', u(start)]); } continue; }
          if (top === 'L') {
            if (value === '(') { stack.push('L'); want.push(['open', u(start)]); }
            else if (value === ')') { stack.pop(); want.push(['close', u(start)]); }
            continue;
          }
          if ('([{'.includes(value)) { stack.push(value); want.push(['open', u(start)]); }
          else { stack.pop(); want.push(['close', u(start)]); }
        }
        St.scan(source, (kind, i) => {
          if (kind === 'open' || kind === 'close') gotB.push([kind, i]);
          if (kind === 'comment') gotC.push(i);
        });
        if (J(gotB) !== J(want)) brackets.push(name);
        if (J(gotC) !== J(wantC)) comments.push(name);
      }
      console.log(`#   ${lines.length} methods in ${((now() - t0) / 1000).toFixed(1)} s; ${said.join(' ')}`);
      eq(unbalanced.slice(0, 5), [], unbalanced.length + ' unbalanced methods');
      eq(brackets.slice(0, 5), [], brackets.length + ' methods with other brackets');
      eq(comments.slice(0, 5), [], comments.length + ' methods with other comments');
    } finally { fs.rmSync(dir, { recursive: true, force: true }); }
  });
}
await corpus();

console.log(`# ${passes} passed, ${failures} failed, ${passes + failures} checks`);
process.exit(failures ? 1 : 0);
