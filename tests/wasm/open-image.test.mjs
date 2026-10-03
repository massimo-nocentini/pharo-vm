// open-image.test.mjs - unit tests of packaging/emscripten/web/open-image.js
//
// usage: node open-image.test.mjs
//
// open-image.js is what the pages' Open runs, in node here: it reads the
// zips that tests/wasm/lib/zip.mjs writes (deflated and stored entries,
// data descriptors, UTF-8 names, directories, the files that the Finder
// adds, zip64 records), pairs the .image with its .changes and .sources,
// inflates them with DecompressionStream as the browsers do and checks
// them; and it refuses what it cannot open, each time with a message that
// says why: not a zip, a zip cut short, damaged or encrypted entries,
// another method, a split zip, no image or several, a 32-bit image or none.
// Then the drops of files on a page, on an EventTarget.  With WASM_DIR, it
// also zips the stock image of the build (WASM_DIR/image/stock) as
// files.pharo.org does, and opens it; and so every zip of WASM_DIR/downloads
// and of OPEN_ZIPS (paths separated by colons, such as the Pharo 12 and 15
// downloads of files.pharo.org).  Prints every check and their count, and
// exits with status 1 if any fails.  Lane 72
// (tests/wasm/lanes/72-open-image.sh) runs it.

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { zip } from './lib/zip.mjs';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const O = require(path.join(here, '..', '..', 'packaging', 'emscripten', 'web', 'open-image.js'));

let failures = 0, passes = 0;
async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.stack) || e).split('\n').slice(0, 6).join('\n  '));
  }
}
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }
function eq(got, want, what) {
  if (JSON.stringify(got) !== JSON.stringify(want))
    throw new Error(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
}
// f must reject with a message that re matches
async function refuses(f, re, what) {
  let e = null;
  try { await f(); } catch (x) { e = x; }
  assert(e, what + ': no error');
  assert(re.test(e.message), what + ': the message ' + JSON.stringify(e.message) + ' does not match ' + re);
  return e.message;
}
const file = (data, name) => new File([data], name);
const bytesOf = async blob => Buffer.from(await blob.arrayBuffer());
const sha = data => crypto.createHash('sha256').update(data).digest('hex');

// An image: the header of a 64-bit Spur image (format 68021), then bytes
// that deflate somewhat
function image(size = 300000, format = 68021) {
  const b = Buffer.alloc(size);
  for (let i = 0; i < size; i++) b[i] = (i * 7 + (i >> 9)) & 0xff ^ (i % 13 === 0 ? 0x5a : 0);
  b.writeUInt32LE(format, 0);
  b.writeUInt32LE(128, 4);
  return b;
}
const IMAGE = image(), CHANGES = Buffer.from('"changes"\r'.repeat(500)), SOURCES = Buffer.from('"sources"\r'.repeat(20000));

// A zip as files.pharo.org gives them: <name>.image, <name>.changes, the
// .sources of the version, pharo.version
const pharoZip = (dir = '', o = {}) => zip([
  { name: dir + 'Pharo15.0-SNAPSHOT-64bit-4e572fed79.changes', data: CHANGES, method: 0 },
  { name: dir + 'Pharo15.0-SNAPSHOT-64bit-4e572fed79.image', data: IMAGE, descriptor: o.descriptor },
  { name: dir + 'Pharo15.0-64bit-4e572fe.sources', data: SOURCES, descriptor: o.descriptor },
  { name: dir + 'pharo.version', data: '150', method: 0 },
], o);

// The checks that the choice and the files opened are those of pharoZip
async function opensPharoZip(buffer, name) {
  const c = await O.choose([file(buffer, name)]);
  eq([c.name, c.from, c.changes && c.changes.name, c.sources && c.sources.name],
     ['Pharo15.0-SNAPSHOT-64bit-4e572fed79.image', name, 'Pharo15.0-SNAPSHOT-64bit-4e572fed79.changes',
      'Pharo15.0-64bit-4e572fe.sources'], 'the choice');
  const seen = [];
  const o = await O.load(c, (loaded, total) => seen.push([loaded, total]));
  const total = IMAGE.length + CHANGES.length + SOURCES.length;
  assert(seen.length > 1 && seen.every(([l, t], i) => t === total && (!i || l >= seen[i - 1][0])), 'progress ' + JSON.stringify(seen.slice(0, 3)));
  eq(seen[seen.length - 1], [total, total], 'the last progress');
  assert(o.name === c.name && o.sources.name === 'Pharo15.0-64bit-4e572fe.sources', 'names ' + o.name + ' ' + o.sources.name);
  assert((await bytesOf(o.image)).equals(IMAGE), 'the image');
  assert((await bytesOf(o.changes)).equals(CHANGES), 'the .changes');
  assert((await bytesOf(o.sources.data)).equals(SOURCES), 'the .sources');
  assert(o.image instanceof Blob && o.changes instanceof Blob && o.sources.data instanceof Blob, 'Blobs');
  return c;
}

await check('a Pharo zip: the image, its .changes and the .sources, inflated and stored, with the progress', async () => {
  await opensPharoZip(pharoZip(), 'latest-64.zip');
});

await check('a Pharo zip in a directory, with a directory entry and what the Finder adds', async () => {
  const buffer = zip([
    { name: 'Pharo 15/' },
    { name: '__MACOSX/Pharo 15/._Pharo15.0-SNAPSHOT-64bit-4e572fed79.image', data: 'resource fork' },
    { name: 'Pharo 15/._Pharo15.0-SNAPSHOT-64bit-4e572fed79.image', data: 'resource fork' },
    { name: 'Pharo 15/Pharo15.0-SNAPSHOT-64bit-4e572fed79.image', data: IMAGE },
    { name: 'Pharo 15/Pharo15.0-SNAPSHOT-64bit-4e572fed79.changes', data: CHANGES },
    { name: 'Pharo 15/Pharo15.0-64bit-4e572fe.sources', data: SOURCES },
    // another .changes and .sources, elsewhere
    { name: 'notes/other.changes', data: 'x' },
    { name: 'notes/other.sources', data: 'x' },
  ]);
  await opensPharoZip(buffer, 'Pharo15.zip');
});

await check('a zip that was streamed (data descriptors), and one with zip64 records', async () => {
  await opensPharoZip(pharoZip('', { descriptor: true }), 'streamed.zip');
  await opensPharoZip(pharoZip('', { zip64: true }), 'zip64.zip');
  await opensPharoZip(pharoZip('dir/', { zip64: true, descriptor: true, comment: 'a comment' }), 'both.zip');
});

await check('names in UTF-8; the .changes and .sources of another name when they are the only ones', async () => {
  const c = await O.choose([file(zip([
    { name: 'Phàro ünïcode.image', data: IMAGE }, { name: 'Phàro.changes', data: CHANGES }, { name: 'any.sources', data: SOURCES },
  ]), 'u.zip')]);
  eq([c.name, c.changes.name, c.sources.name], ['Phàro ünïcode.image', 'Phàro.changes', 'any.sources'], 'names');
  // two of them, neither of its name: none
  const d = await O.choose([file(zip([
    { name: 'X.image', data: IMAGE }, { name: 'A.changes', data: 'a' }, { name: 'B.changes', data: 'b' },
  ]), 'two.zip')]);
  assert(d.changes === null && d.sources === null, 'no .changes, no .sources');
  eq(O.describe(d), 'X.image from two.zip', 'describe');
});

await check('loose files: paired by base name; the only one; none; what else was chosen is ignored', async () => {
  const img = file(IMAGE, 'My.image'), ch = file(CHANGES, 'My.changes'), src = file(SOURCES, 'Pharo12.0-64bit-7d5f14c.sources');
  let c = await O.choose([src, ch, img, file('150', 'pharo.version')]);
  eq([c.name, c.from, c.changes.name, c.sources.name], ['My.image', null, 'My.changes', 'Pharo12.0-64bit-7d5f14c.sources'], 'all three');
  eq(O.describe(c), 'My.image with My.changes and Pharo12.0-64bit-7d5f14c.sources', 'describe');
  let o = await O.load(c);
  assert(o.image === img && o.changes === ch && o.sources.data === src, 'the Files themselves');
  c = await O.choose([img, file(CHANGES, 'Other.changes'), file(CHANGES, 'My.changes')]);
  eq(c.changes.name, 'My.changes', 'of its base name first');
  c = await O.choose([img, file(CHANGES, 'Other.changes')]);
  eq([c.changes.name, c.sources], ['Other.changes', null], 'the only one');
  c = await O.choose([img]);
  eq(O.describe(c), 'My.image, without a .changes', 'describe');
  o = await O.load(c);
  assert(o.changes instanceof Blob && o.changes.size === 0 && o.sources === undefined, 'an empty .changes, no .sources');
});

await check('what cannot be chosen: no image, several, an empty one, several zips, a zip and files', async () => {
  await refuses(() => O.choose([]), /^Choose a Pharo \.zip, or an \.image/, 'nothing');
  await refuses(() => O.choose([file('x', 'notes.txt'), file(CHANGES, 'My.changes')]), /^Choose a Pharo \.zip/, 'no image');
  await refuses(() => O.choose([file(IMAGE, 'A.image'), file(IMAGE, 'B.image')]), /^You chose 2 images \(A\.image, B\.image\): open one at a time\.$/, 'two');
  await refuses(() => O.choose([file('', 'A.image')]), /^A\.image is empty\.$/, 'empty');
  await refuses(() => O.choose([file(pharoZip(), 'a.zip'), file(pharoZip(), 'b.zip')]), /^Open one \.zip at a time\.$/, 'two zips');
  await refuses(() => O.choose([file(pharoZip(), 'a.zip'), file(IMAGE, 'A.image')]), /^Open a \.zip alone/, 'a zip and an image');
  await refuses(() => O.choose([file(zip([{ name: 'README', data: 'x' }, { name: 'a.changes', data: 'x' }]), 'none.zip')]),
                /^none\.zip holds no \.image file\.$/, 'a zip without an image');
  await refuses(() => O.choose([file(zip([{ name: 'a/X.image', data: IMAGE }, { name: 'b/Y.image', data: IMAGE }]), 'two.zip')]),
                /^two\.zip holds 2 images \(X\.image, Y\.image\): open one at a time\.$/, 'a zip of two images');
});

await check('not a zip, or not a whole one; a split zip', async () => {
  const z = pharoZip();
  const notZip = /^x\.zip is not a zip file, or not a whole one/;
  await refuses(() => O.choose([file(crypto.randomBytes(5000), 'x.zip')]), notZip, 'random bytes');
  await refuses(() => O.choose([file('', 'x.zip')]), notZip, 'empty');
  await refuses(() => O.choose([file(z.subarray(0, z.length >> 1), 'x.zip')]), notZip, 'the first half');
  await refuses(() => O.choose([file(z.subarray(0, z.length - 30), 'x.zip')]), notZip, 'the end cut');
  // its second half: its directory says that it starts before the file
  await refuses(() => O.choose([file(z.subarray(z.length >> 1), 'x.zip')]), notZip, 'the start cut');
  const split = zip([{ name: 'X.image', data: IMAGE }], { disk: 1 });
  await refuses(() => O.choose([file(split, 'x.zip')]), /^x\.zip is one part of a zip split into several files/, 'split');
});

await check('damaged entries: bad deflate data, a wrong CRC, a wrong size; encrypted; another method', async () => {
  const open = async buffer => O.load(await O.choose([file(buffer, 'x.zip')]));
  const deflated = zip([{ name: 'X.image', data: IMAGE }]);
  // bytes changed in the middle of the deflate stream
  const bad = Buffer.from(deflated);
  for (let i = 0; i < 64; i++) bad[2000 + i * 31] ^= 0xa5;
  const msg = await refuses(() => open(bad), /^x\.zip: X\.image is damaged \(/, 'bad deflate data');
  console.log('#   ' + msg);
  await refuses(() => open(zip([{ name: 'X.image', data: IMAGE, method: 0, crc: 12345 }])),
                /^x\.zip: X\.image is damaged \(its CRC-32 does not match\)$/, 'a stored entry with a wrong CRC');
  // a deflate stream that ends early: the directory says more bytes
  const short = zip([{ name: 'X.image', data: IMAGE, body: (await import('node:zlib')).deflateRawSync(IMAGE.subarray(0, 1000)),
                       crc: 0 }]);
  await refuses(() => open(short), /^x\.zip: X\.image is damaged \(it inflates to 1000 bytes, not 300000\)$/, 'too short');
  await refuses(() => open(zip([{ name: 'X.image', data: IMAGE, flags: 1 }])), /^x\.zip: X\.image is encrypted/, 'encrypted');
  await refuses(() => open(zip([{ name: 'X.image', data: IMAGE, method: 12 }])),
                /^x\.zip: X\.image is compressed with bzip2, which this page cannot inflate/, 'bzip2');
  await refuses(() => open(zip([{ name: 'X.image', data: IMAGE, method: 7 }])), /compressed with method 7,/, 'method 7');
});

await check('images the VM cannot load: 32-bit, another format, too short', async () => {
  const open = async (data, name = 'X.image') => O.load(await O.choose([file(data, name)]));
  await refuses(() => open(image(1000, 6521)), /^X\.image is a 32-bit image: this VM runs 64-bit images/, '32-bit');
  await refuses(() => open(Buffer.from('<html>not an image</html>')), /^X\.image is not a Pharo image that this VM can run \(its header says format \d+, not 68021\)\.$/, 'html');
  await refuses(() => open(Buffer.from('ab')), /^X\.image is not a Pharo image \(it is too short\)\.$/, 'two bytes');
  // in a zip, after the inflation
  await refuses(async () => O.load(await O.choose([file(zip([{ name: 'X.image', data: image(5000, 6505) }]), 'old.zip')])),
                /^X\.image is a 32-bit image/, '32-bit, zipped');
  const swapped = image(1000);
  swapped.writeUInt32BE(68021, 0);
  await open(swapped);                  // byte-swapped, as the VM reads it too
});

await check('drops: files dragged over show, leave hides, a drop opens them; a text drag is left alone', async () => {
  const target = new EventTarget(), shown = [], opened = [];
  let enabled = true;
  O.drops(target, { enabled: () => enabled, show: on => shown.push(on), open: files => opened.push(files.map(f => f.name)) });
  const fire = (type, types, files = []) => {
    const e = new Event(type, { cancelable: true });
    const dataTransfer = { types, files, dropEffect: 'none' };
    Object.defineProperty(e, 'dataTransfer', { value: dataTransfer });
    target.dispatchEvent(e);
    return { prevented: e.defaultPrevented, effect: dataTransfer.dropEffect };
  };
  const files = [file(IMAGE, 'X.image'), file(CHANGES, 'X.changes')];
  assert(fire('dragenter', ['Files']).prevented, 'dragenter prevented');
  fire('dragenter', ['Files']);         // into a child
  fire('dragleave', ['Files']);         // out of the child
  eq(shown, [true], 'shown once, still shown');
  const over = fire('dragover', ['Files']);
  assert(over.prevented && over.effect === 'copy', 'dragover allows the drop: ' + JSON.stringify(over));
  assert(fire('drop', ['Files'], files).prevented, 'drop prevented');
  eq([shown, opened], [[true, false], [['X.image', 'X.changes']]], 'hidden, opened');
  fire('dragenter', ['Files']);
  fire('dragleave', ['Files']);
  eq(shown, [true, false, true, false], 'left the page');
  const text = fire('dragover', ['text/plain']);
  assert(!text.prevented && text.effect === 'none', 'a text drag');
  enabled = false;
  assert(fire('dragover', ['Files']).effect === 'none', 'no drop while Open is off');
  fire('drop', ['Files'], files);
  eq(opened.length, 1, 'nothing opened while Open is off');
});

// Real images: the stock image of the build, zipped as files.pharo.org
// does, and the zips given
const wasmDir = process.env.WASM_DIR;
const stock = wasmDir && path.join(wasmDir, 'image', 'stock');
if (stock && fs.existsSync(path.join(stock, 'Pharo.image'))) await check('the stock image of the build, zipped as files.pharo.org does, opens whole', async () => {
  const sources = fs.readdirSync(stock).find(f => f.endsWith('.sources'));
  const files = { image: fs.readFileSync(path.join(stock, 'Pharo.image')), changes: fs.readFileSync(path.join(stock, 'Pharo.changes')),
                  sources: fs.readFileSync(path.join(stock, sources)) };
  const t0 = performance.now();
  const buffer = zip([
    { name: 'Pharo12.0-SNAPSHOT-64bit-stock.changes', data: files.changes },
    { name: 'Pharo12.0-SNAPSHOT-64bit-stock.image', data: files.image },
    { name: sources, data: files.sources },
    { name: 'pharo.version', data: '120', method: 0 },
  ]);
  const t1 = performance.now();
  const c = await O.choose([file(buffer, 'stock.zip')]);
  const o = await O.load(c);
  const t2 = performance.now();
  eq([c.name, c.changes.name, c.sources.name], ['Pharo12.0-SNAPSHOT-64bit-stock.image', 'Pharo12.0-SNAPSHOT-64bit-stock.changes', sources], 'names');
  assert(sha(await bytesOf(o.image)) === sha(files.image) && sha(await bytesOf(o.sources.data)) === sha(files.sources) &&
         sha(await bytesOf(o.changes)) === sha(files.changes), 'the files');
  console.log(`#   ${(buffer.length / 1e6).toFixed(1)} MB zipped in ${(t1 - t0).toFixed(0)} ms, ` +
              `${((o.image.size + o.sources.data.size) / 1e6).toFixed(1)} MB opened in ${(t2 - t1).toFixed(0)} ms`);
});
else console.log('# skip: no WASM_DIR/image/stock/Pharo.image');

const zips = [];
if (wasmDir && fs.existsSync(path.join(wasmDir, 'downloads')))
  for (const f of fs.readdirSync(path.join(wasmDir, 'downloads'))) if (f.endsWith('.zip')) zips.push(path.join(wasmDir, 'downloads', f));
for (const f of (process.env.OPEN_ZIPS || '').split(':').filter(Boolean)) zips.push(path.resolve(f));
for (const z of zips) await check('a Pharo zip: ' + z, async () => {
  const t0 = performance.now();
  const c = await O.choose([file(fs.readFileSync(z), path.basename(z))]);
  assert(c.changes && c.sources, 'a .changes and a .sources: ' + O.describe(c));
  const o = await O.load(c);
  assert(o.image.size === c.image.size && o.changes.size === c.changes.size && o.sources.data.size === c.sources.size, 'sizes');
  console.log(`#   ${O.describe(c)}, with ${c.sources.name}: ${((o.image.size + o.sources.data.size) / 1e6).toFixed(1)} MB in ` +
              `${(performance.now() - t0).toFixed(0)} ms`);
});

console.log(`# ${passes} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
