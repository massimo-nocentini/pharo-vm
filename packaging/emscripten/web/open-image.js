// open-image.js - open an image of your own in the pages of the Pharo VM for WebAssembly
//
// UMD: the global PharoOpen in the pages (index.html, world.html), a CommonJS
// module in node (tests/wasm/open-image.test.mjs).  But for drops(), which
// listens to the drag events of a page, it uses no DOM: Blobs, streams and
// DecompressionStream only.  No dependencies.
//
//   const choice = await PharoOpen.choose(files);
//   PharoOpen.describe(choice)             // e.g. 'X.image from X.zip'
//   const opened = await PharoOpen.load(choice, onProgress);
//   PharoOpen.drops(target, {enabled, show, open})
//
// choose(files) takes what the user chose or dropped, Files (or Blobs with
// a name): a Pharo distribution .zip as files.pharo.org gives it (such as
// https://files.pharo.org/image/150/latest-64.zip, which holds <name>.image,
// <name>.changes, a .sources and pharo.version, possibly in a directory),
// or the files themselves, an .image, its .changes and optionally a
// .sources.  Of a zip it reads only the central directory.  The image is
// the only .image (several are an error); its .changes and its .sources are
// those of its base name, else the only one of their kind, those of its
// directory first.  Anything else is ignored, as are the __MACOSX/ and ._
// files of the zips of macOS.  It answers {name, from, image, changes,
// sources}: the name of the image, that of the zip it is in (or null), and
// the files, each {name, size} (changes and sources may be null).
//
// load(choice, onProgress) answers {name, image, changes, sources}, the
// files as Blobs: image, changes (an empty Blob when there was none), and
// sources as {name, data}, or undefined.  It inflates the entries of a zip,
// deflated ones through DecompressionStream('deflate-raw') and stored ones
// as they are, checks their sizes and CRC-32, and calls onProgress(loaded,
// total) with the bytes inflated so far.  Then it checks that the image is
// one the VM loads: a 64-bit Spur image.
//
// Both reject with an Error whose message is meant for the user, such as
// "Pharo.zip is not a zip file, or not a whole one".  Zip64 archives are
// read; a zip split into several files, an encrypted entry and methods
// other than deflate and store are refused, with a message that says so.
//
// drops(target, o) makes files dropped on target (the window of a page)
// open as o.open(files) does, unless o.enabled() says no; o.show(on) shows
// or hides what the page shows while files are dragged over it.  Only drags
// of files count: a text dragged into an input is left alone.

(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.PharoOpen = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  // The signatures of the records of a zip
  const EOCD = 0x06054b50, ZIP64_LOCATOR = 0x07064b50, ZIP64_EOCD = 0x06064b50;
  const CENTRAL = 0x02014b50, LOCAL = 0x04034b50;
  const STORED = 0, DEFLATED = 8;
  const METHODS = { 9: 'Deflate64', 12: 'bzip2', 14: 'LZMA', 93: 'Zstandard', 95: 'XZ', 98: 'PPMd', 99: 'AES' };
  // The image formats that the VM loads (AbstractImageAccess), and those
  // of 32-bit images
  const FORMATS = [68021, 68002], FORMATS_32 = [6521, 6505, 6504, 6502];
  const NOT_A_ZIP = 'is not a zip file, or not a whole one (was the download complete?)';

  const fail = message => { throw new Error(message); };
  const ext = name => (/\.[^./]*$/.exec(name.toLowerCase()) || [''])[0];
  const baseName = path => path.slice(path.lastIndexOf('/') + 1);
  const dirName = path => path.slice(0, path.lastIndexOf('/') + 1);
  const stem = name => name.replace(/\.[^.]*$/, '');
  // what the Finder adds to a zip: the resource forks of its files
  const junk = path => /^__MACOSX\//.test(path) || /^\._/.test(baseName(path));

  // ---- reading a zip

  const view = bytes => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u16 = (d, at) => d.getUint16(at, true);
  const u32 = (d, at) => d.getUint32(at, true);
  // a 64-bit field, which must fit in a double without loss
  function u64(d, at) {
    const high = u32(d, at + 4);
    if (high > 0x1fffff) fail('a zip64 size beyond 2^53');
    return high * 0x100000000 + u32(d, at);
  }
  async function bytesAt(blob, start, end) {
    return new Uint8Array(await blob.slice(start, end).arrayBuffer());
  }

  // The entries of the zip blob, named zipName, from its central directory:
  // [{name, size, compressedSize, method, crc, offset, flags}]
  async function entries(blob, zipName) {
    const bad = what => new Error(zipName + ' ' + what);
    // the end of central directory record, after which only its comment comes
    const tailStart = Math.max(0, blob.size - 22 - 0xffff);
    const tail = await bytesAt(blob, tailStart, blob.size), t = view(tail);
    let at = -1;
    for (let i = tail.length - 22; i >= 0; i--)
      if (u32(t, i) === EOCD && i + 22 + u16(t, i + 20) <= tail.length) { at = i; break; }
    if (at < 0) throw bad(NOT_A_ZIP);
    const eocd = tailStart + at;
    let disk = u16(t, at + 4), cdDisk = u16(t, at + 6), count = u16(t, at + 10);
    let cdSize = u32(t, at + 12), cdOffset = u32(t, at + 16);
    if (disk === 0xffff || cdDisk === 0xffff || count === 0xffff || cdSize === 0xffffffff || cdOffset === 0xffffffff) {
      // zip64: its locator comes just before, and says where its record is
      const loc = eocd >= 20 ? view(await bytesAt(blob, eocd - 20, eocd)) : null;
      if (!loc || u32(loc, 0) !== ZIP64_LOCATOR) throw bad('is a damaged zip64 file (no zip64 locator)');
      const recAt = u64(loc, 8), rec = view(await bytesAt(blob, recAt, recAt + 56));
      if (rec.byteLength < 56 || u32(rec, 0) !== ZIP64_EOCD) throw bad('is a damaged zip64 file (no zip64 directory record)');
      disk = u32(rec, 16);
      cdDisk = u32(rec, 20);
      count = u64(rec, 32);
      cdSize = u64(rec, 40);
      cdOffset = u64(rec, 48);
    }
    if (disk !== 0 || cdDisk !== 0) throw bad('is one part of a zip split into several files, which this page cannot read');
    if (cdOffset + cdSize > eocd) throw bad(NOT_A_ZIP);
    const cd = await bytesAt(blob, cdOffset, cdOffset + cdSize), d = view(cd);
    const names = new TextDecoder();
    const list = [];
    for (let p = 0, k = 0; k < count; k++) {
      if (p + 46 > cd.length || u32(d, p) !== CENTRAL) throw bad('is damaged (its central directory is cut short)');
      const flags = u16(d, p + 8), method = u16(d, p + 10), crc = u32(d, p + 16);
      let compressedSize = u32(d, p + 20), size = u32(d, p + 24), offset = u32(d, p + 42);
      const nameLength = u16(d, p + 28), extraLength = u16(d, p + 30), commentLength = u16(d, p + 32);
      const end = p + 46 + nameLength + extraLength;
      if (end + commentLength > cd.length) throw bad('is damaged (its central directory is cut short)');
      // (names are UTF-8 or CP437, which are the same in ASCII)
      const name = names.decode(cd.subarray(p + 46, p + 46 + nameLength));
      // the zip64 extra field: the 64-bit values of the fields that say
      // 0xffffffff, in this order
      for (let x = p + 46 + nameLength; x + 4 <= end; x += 4 + u16(d, x + 2)) {
        if (u16(d, x) !== 1) continue;
        let y = x + 4;
        const next = () => { if (y + 8 > end) throw bad('is damaged (a zip64 field is cut short)'); y += 8; return u64(d, y - 8); };
        if (size === 0xffffffff) size = next();
        if (compressedSize === 0xffffffff) compressedSize = next();
        if (offset === 0xffffffff) offset = next();
        break;
      }
      list.push({ name, size, compressedSize, method, crc, offset, flags });
      p = end + commentLength;
    }
    return list;
  }

  // CRC-32 (ISO-HDLC, as zip has it), a table at a time
  let table = null;
  function crc32(crc, bytes) {
    if (!table) {
      table = new Int32Array(256);
      for (let n = 0; n < 256; n++) {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        table[n] = c;
      }
    }
    for (let i = 0; i < bytes.length; i++) crc = table[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
    return crc;
  }

  // The entry e of the zip blob, inflated, as a Blob; onBytes(n) is called
  // with the bytes of every chunk
  async function inflate(blob, zipName, e, onBytes) {
    const where = zipName + ': ' + e.name;
    if (e.flags & 0x41) throw new Error(where + ' is encrypted, which this page cannot read');
    if (e.method !== STORED && e.method !== DEFLATED)
      throw new Error(where + ' is compressed with ' + (METHODS[e.method] || 'method ' + e.method) +
                      ', which this page cannot inflate: zip it with deflate, or open the files themselves');
    const h = view(await bytesAt(blob, e.offset, e.offset + 30));
    if (h.byteLength < 30 || u32(h, 0) !== LOCAL) throw new Error(zipName + ' is damaged (' + e.name + ' is not where its directory says)');
    const start = e.offset + 30 + u16(h, 26) + u16(h, 28);
    const raw = blob.slice(start, start + e.compressedSize);
    if (raw.size !== e.compressedSize) throw new Error(zipName + ' ' + NOT_A_ZIP);
    let stream = raw.stream();
    if (e.method === DEFLATED) {
      let inflater;
      try { inflater = new DecompressionStream('deflate-raw'); }
      catch (x) { throw new Error('This browser cannot inflate zip files (it has no deflate-raw DecompressionStream): unzip ' + zipName + ', and open its files'); }
      stream = stream.pipeThrough(inflater);
    }
    const reader = stream.getReader(), chunks = [];
    let n = 0, crc = -1;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        n += value.length;
        if (n > e.size) break;
        crc = crc32(crc, value);
        if (e.method === DEFLATED) chunks.push(value);
        onBytes(value.length);
      }
    } catch (x) {
      // (node says why in the cause)
      const why = (x && (x.message || (x.cause && x.cause.message) || x.name)) || x;
      throw new Error(where + ' is damaged (' + String(why).replace(/\.$/, '') + ')');
    } finally {
      reader.cancel().catch(() => {});
    }
    if (n !== e.size) throw new Error(where + ' is damaged (it inflates to ' + (n > e.size ? 'more' : n) + ' bytes, not ' + e.size + ')');
    if ((crc ^ -1) >>> 0 !== e.crc) throw new Error(where + ' is damaged (its CRC-32 does not match)');
    return e.method === STORED ? raw : new Blob(chunks);
  }

  // ---- choosing the files

  // Of the files list, those of extension x for the image: of its base
  // name, else the only one, those of its directory first
  function pick(list, x, image) {
    const all = list.filter(f => ext(f.name) === x);
    const here = all.filter(f => dirName(f.path) === dirName(image.path));
    const named = l => l.find(f => stem(f.name) === stem(image.name));
    return named(here) || named(all) || (here.length === 1 ? here[0] : all.length === 1 ? all[0] : null);
  }

  async function choose(files) {
    files = Array.from(files || []);
    const zips = files.filter(f => ext(f.name) === '.zip');
    if (zips.length > 1) fail('Open one .zip at a time.');
    if (zips.length && files.length > 1) fail('Open a .zip alone, or the .image, .changes and .sources files without it.');
    const zip = zips[0] || null;
    let list;
    if (zip) {
      list = (await entries(zip, zip.name)).filter(e => !e.name.endsWith('/') && !junk(e.name))
        .map(e => ({ name: baseName(e.name), path: e.name, size: e.size, entry: e }));
    } else {
      list = files.filter(f => !junk(f.name)).map(f => ({ name: f.name, path: f.name, size: f.size, file: f }));
    }
    const images = list.filter(f => ext(f.name) === '.image');
    if (!images.length)
      fail(zip ? zip.name + ' holds no .image file.' : 'Choose a Pharo .zip, or an .image with its .changes (and its .sources).');
    if (images.length > 1)
      fail((zip ? zip.name + ' holds ' : 'You chose ') + images.length + ' images (' +
           images.map(f => f.name).join(', ') + '): open one at a time.');
    const image = images[0];
    if (!image.size) fail(image.name + ' is empty.');
    return { name: image.name, from: zip ? zip.name : null, image, changes: pick(list, '.changes', image),
             sources: pick(list, '.sources', image), zip };
  }

  // Whether the image is one the VM loads: the format number of its header
  async function checkImage(blob, name) {
    const head = await bytesAt(blob, 0, 4);
    if (head.length < 4) fail(name + ' is not a Pharo image (it is too short).');
    const le = view(head).getUint32(0, true), be = view(head).getUint32(0, false);
    if (FORMATS.includes(le) || FORMATS.includes(be)) return;
    if (FORMATS_32.includes(le) || FORMATS_32.includes(be))
      fail(name + ' is a 32-bit image: this VM runs 64-bit images (the -64 downloads of files.pharo.org).');
    fail(name + ' is not a Pharo image that this VM can run (its header says format ' + le + ', not 68021).');
  }

  // What is opened: the image, and where it comes from or what comes with it
  function describe(c) {
    if (c.from) return c.name + ' from ' + c.from;
    const others = [c.changes, c.sources].filter(Boolean).map(f => f.name);
    return c.name + (others.length ? ' with ' + others.join(' and ') : '') + (c.changes ? '' : ', without a .changes');
  }

  async function load(choice, onProgress) {
    const { zip, from } = choice;
    const total = zip ? [choice.image, choice.changes, choice.sources].reduce((n, f) => n + (f ? f.size : 0), 0) : 0;
    let loaded = 0;
    const read = f => !f ? null : !zip ? f.file
      : inflate(zip, from, f.entry, n => { loaded += n; if (onProgress) onProgress(loaded, total); });
    const image = await read(choice.image);
    await checkImage(image, choice.name);
    const changes = (await read(choice.changes)) || new Blob([]);
    const sources = await read(choice.sources);
    return { name: choice.name, image, changes, sources: sources ? { name: choice.sources.name, data: sources } : undefined };
  }

  // ---- dropping files on a page

  function drops(target, o) {
    let depth = 0;                      // dragenter and dragleave come per element
    const files = e => !!e.dataTransfer && Array.from(e.dataTransfer.types || []).includes('Files');
    target.addEventListener('dragenter', e => {
      if (!files(e)) return;
      e.preventDefault();
      if (depth++ === 0) o.show(true);
    });
    target.addEventListener('dragleave', e => {
      if (!files(e)) return;
      if (--depth <= 0) { depth = 0; o.show(false); }
    });
    target.addEventListener('dragover', e => {
      if (!files(e)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = o.enabled() ? 'copy' : 'none';
    });
    target.addEventListener('drop', e => {
      if (!files(e)) return;
      e.preventDefault();
      depth = 0;
      o.show(false);
      if (o.enabled()) o.open(Array.from(e.dataTransfer.files));
    });
  }

  return { choose, describe, load, drops, entries, inflate, crc32 };
});
