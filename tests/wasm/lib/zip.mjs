// zip.mjs - write zip files, for the tests of the pages' Open (open-image.js)
//
//   import { zip, imageZip } from './lib/zip.mjs';
//   const buffer = zip([{ name, data, method, ... }, ...], { zip64, disk, comment });
//   const buffer = imageZip(dir, { base, folder, sourcesTail, version });
//
// Each entry is a file (data, a Buffer or a string) or, with a name ending
// in /, a directory.  method 8 (the default) deflates it with zlib, at
// level (default 1), and 0 stores it.  descriptor writes its CRC and sizes
// after the data (flag bit 3), with zeros in its local header, as zippers
// that stream do; flags, method and crc may say what is not so (an
// encrypted entry, another method, a wrong CRC), and body replaces the
// data written, for the tests of damaged zips.  zip64 writes every entry
// with the zip64 extra field and 0xffffffff in its 32-bit fields, and the
// zip64 end of central directory record and its locator, as an archive of
// files over 4 GiB has them; disk is the number of the disk that the end
// record says it is (not 0 in a zip split into several files).  Names are
// UTF-8 (flag bit 11).  Answers the whole zip as a Buffer.
//
// imageZip(dir, o) zips the image of the directory dir (its one .image, the
// .changes of that and its one .sources) as files.pharo.org does: as
// <o.base>.image and <o.base>.changes, deflated, with the .sources, stored,
// and a pharo.version saying o.version, in the directory o.folder of the
// zip, if any, with the resource fork that the Finder adds; o.sourcesTail
// is appended to the .sources, which so becomes another one of that name.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const u16 = n => { const b = Buffer.alloc(2); b.writeUInt16LE(n); return b; };
const u32 = n => { const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b; };
const u64 = n => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return b; };
const MAX32 = 0xffffffff;
const DATE = 0x5321, TIME = 0x6000;     // 2021-09-01 12:00

// the zip64 extended information extra field of these 64-bit values
const zip64Extra = values => Buffer.concat([u16(1), u16(8 * values.length), ...values.map(u64)]);

export function zip(entries, { zip64 = false, disk = 0, comment = '' } = {}) {
  const parts = [], central = [];
  let offset = 0;
  for (const e of entries) {
    const name = Buffer.from(e.name, 'utf8'), dir = e.name.endsWith('/');
    const data = Buffer.from(e.data || '');
    const method = e.method !== undefined ? e.method : dir ? 0 : 8;
    const body = e.body || (method === 8 ? zlib.deflateRawSync(data, { level: e.level || 1 }) : data);
    const crc = e.crc !== undefined ? e.crc : zlib.crc32(data);
    const flags = (e.flags || 0) | 0x800 | (e.descriptor ? 8 : 0);
    const version = zip64 ? 45 : 20;
    const localExtra = zip64 ? zip64Extra([e.descriptor ? 0 : data.length, e.descriptor ? 0 : body.length]) : Buffer.alloc(0);
    const local = Buffer.concat([
      u32(0x04034b50), u16(version), u16(flags), u16(method), u16(TIME), u16(DATE),
      u32(e.descriptor ? 0 : crc),
      u32(zip64 ? MAX32 : e.descriptor ? 0 : body.length), u32(zip64 ? MAX32 : e.descriptor ? 0 : data.length),
      u16(name.length), u16(localExtra.length), name, localExtra]);
    const descriptor = !e.descriptor ? Buffer.alloc(0)
      : zip64 ? Buffer.concat([u32(0x08074b50), u32(crc), u64(body.length), u64(data.length)])
      : Buffer.concat([u32(0x08074b50), u32(crc), u32(body.length), u32(data.length)]);
    parts.push(local, body, descriptor);
    const centralExtra = zip64 ? zip64Extra([data.length, body.length, offset]) : Buffer.alloc(0);
    central.push(Buffer.concat([
      u32(0x02014b50), u16(0x031e), u16(version), u16(flags), u16(method), u16(TIME), u16(DATE), u32(crc),
      u32(zip64 ? MAX32 : body.length), u32(zip64 ? MAX32 : data.length),
      u16(name.length), u16(centralExtra.length), u16(0), u16(0), u16(0),
      u32(dir ? 0x41ed0010 : 0x81a40000), u32(zip64 ? MAX32 : offset), name, centralExtra]));
    offset += local.length + body.length + descriptor.length;
  }
  const cd = Buffer.concat(central), n = entries.length, tail = [];
  if (zip64) {
    tail.push(u32(0x06064b50), u64(44), u16(45), u16(45), u32(disk), u32(disk), u64(n), u64(n),
              u64(cd.length), u64(offset));
    tail.push(u32(0x07064b50), u32(disk), u64(offset + cd.length), u32(disk + 1));
  }
  const text = Buffer.from(comment, 'utf8');
  tail.push(u32(0x06054b50), u16(zip64 ? 0xffff : disk), u16(zip64 ? 0xffff : disk),
            u16(zip64 ? 0xffff : n), u16(zip64 ? 0xffff : n), u32(zip64 ? MAX32 : cd.length),
            u32(zip64 ? MAX32 : offset), u16(text.length), text);
  return Buffer.concat([...parts, cd, ...tail]);
}

export function imageZip(dir, { base = 'Pharo', folder = '', sourcesTail = '', version = '120' } = {}) {
  const names = fs.readdirSync(dir), image = names.find(f => f.endsWith('.image'));
  const sources = names.find(f => f.endsWith('.sources'));
  const read = name => fs.readFileSync(path.join(dir, name));
  const prefix = folder ? folder.replace(/\/?$/, '/') : '';
  return zip([
    ...(folder ? [{ name: prefix }, { name: '__MACOSX/' + prefix + '._' + base + '.image', data: 'a resource fork' }] : []),
    { name: prefix + base + '.changes', data: read(image.replace(/\.image$/, '.changes')) },
    { name: prefix + base + '.image', data: read(image) },
    { name: prefix + sources, data: Buffer.concat([read(sources), Buffer.from(sourcesTail)]), method: 0 },
    { name: prefix + 'pharo.version', data: version, method: 0 },
  ]);
}
