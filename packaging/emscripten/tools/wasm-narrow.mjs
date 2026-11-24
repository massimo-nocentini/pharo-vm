#!/usr/bin/env node
// wasm-narrow.mjs - narrows the address arithmetic of a module with a 32-bit memory
//
//   wasm-narrow.mjs IN.wasm OUT.wasm
//       writes OUT (which may be IN): IN with its addresses computed in 32 bits.
//
// A MEMORY64=2 module computes its addresses in 64 bits, and wraps them with
// i32.wrap_i64 at every load and store.  Wrapping goes before an add, sub,
// and, or, xor, mul or shl whose last operand is a constant or a local, since
// the low half of the result needs only the low halves of the operands; the
// constants and the operation are then 32-bit, and the engine folds them into
// its addressing modes.  Nothing else changes: the functions keep their
// indices, and the other sections are copied as they are.
// (WASM_TUNE=NARROW, docs/WebAssembly.md: Faster interpreters)
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

// --- LEB128
function uleb(n) { const o = []; do { let x = n % 128; n = Math.floor(n / 128); if (n) x |= 128; o.push(x); } while (n); return o; }
function sleb(n) { const o = []; n = BigInt(n); for (;;) { const x = Number(n & 127n); n >>= 7n; if ((n === 0n && !(x & 64)) || (n === -1n && x & 64)) { o.push(x); return o; } o.push(x | 128); } }
class Reader {
  constructor(b, p = 0) { this.b = b; this.p = p; }
  byte() { if (this.p >= this.b.length) throw new Error('wasm-narrow: the module is truncated'); return this.b[this.p++]; }
  u() { let r = 0, m = 1, x; do { x = this.byte(); r += (x & 127) * m; m *= 128; } while (x & 128); return r; }
  s() { let r = 0n, s = 0n, x; do { x = this.byte(); r |= BigInt(x & 127) << s; s += 7n; } while (x & 128); return x & 64 ? r - (1n << s) : r; }
  skip(n) { if (this.p + n > this.b.length) throw new Error('wasm-narrow: the module is truncated'); const q = this.p; this.p += n; return this.b.subarray(q, this.p); }
}

// --- the code of a function: [{ op, a, x }], x its bytes, a its constant or index; and where it starts in the body
const INDEX_OPS = new Set([0x07, 0x08, 0x09, 0x18, 0x0c, 0x0d, 0x10, 0x12, 0x14, 0x15, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0xd2, 0xd5, 0xd6]);
const PLAIN = op => op <= 1 || op === 5 || op === 0x0a || op === 0x0b || op === 0x0f || op === 0x19 || op === 0x1a || op === 0x1b
  || (op >= 0x45 && op <= 0xc4) || op === 0xd1 || op === 0xd3 || op === 0xd4;
function decode(body) {
  const r = new Reader(body), code = [];
  for (let n = r.u(); n > 0; n--) { r.u(); r.byte(); }
  const start = r.p;
  while (r.p < body.length) {
    const o = r.p, op = r.byte(), i = { op };
    if (op === 2 || op === 3 || op === 4 || op === 6) r.s();
    else if (op === 0x1f) { r.s(); for (let n = r.u(); n > 0; n--) { if (r.byte() < 2) r.u(); r.u(); } }
    else if (INDEX_OPS.has(op)) i.a = r.u();
    else if (op === 0x0e) for (let n = r.u(); n >= 0; n--) r.u();
    else if (op === 0x11 || op === 0x13) { r.u(); r.u(); }
    else if (op === 0x1c) r.skip(r.u());
    else if (op >= 0x28 && op <= 0x3e) { if (r.u() >= 64) throw new Error('wasm-narrow: a load or store of another memory'); r.u(); }
    else if (op === 0x3f || op === 0x40) r.byte();
    else if (op === 0x41 || op === 0x42) i.a = r.s();
    else if (op === 0x43) r.skip(4);
    else if (op === 0x44) r.skip(8);
    else if (op === 0xd0) r.s();
    else if (op === 0xfc) {
      const s = r.u();
      if (s === 8 || s === 10 || s === 12 || s === 14) { r.u(); r.u(); } else if (s === 9 || s === 11 || s === 13 || (s > 14 && s < 18)) r.u(); else if (s > 17) throw new Error(`wasm-narrow: opcode fc ${s}`);
    } else if (op === 0xfd) {
      const s = r.u();
      if (s <= 11 || s === 92 || s === 93) { r.u(); r.u(); } else if (s === 12 || s === 13) r.skip(16); else if (s >= 21 && s <= 34) r.byte(); else if (s >= 84 && s <= 91) { r.u(); r.u(); r.byte(); }
    } else if (!PLAIN(op)) throw new Error(`wasm-narrow: opcode ${op.toString(16)}`);
    i.x = body.subarray(o, r.p); code.push(i);
  }
  return { start, code };
}

// --- the 32-bit operations of the i64 add, sub, and, or, xor and mul
const WRAP = 0xa7, TO32 = { 0x7c: 0x6a, 0x7d: 0x6b, 0x83: 0x71, 0x84: 0x72, 0x85: 0x73, 0x7e: 0x6c };
const W = { op: WRAP, x: Buffer.from([WRAP]) };
const const32 = a => { const c = BigInt.asIntN(32, a); return { op: 0x41, a: c, x: Buffer.from([0x41, ...sleb(c)]) }; };
const plain = op => ({ op, x: Buffer.from([op]) });

// the body narrowed, and the number of rewrites
function narrowBody(body) {
  const { start, code: l } = decode(body);
  let n = 0;
  for (let k = 1; k < l.length; k++) {
    while (k >= 1 && l[k].op === WRAP) {
      const p = l[k - 1], q = l[k - 2];
      if (p.op === 0xac || p.op === 0xad) { l.splice(k - 1, 2); n++; k = Math.max(k - 2, 0); continue; }   // i64.extend_i32_s, _u
      if (!q) break;
      let r;
      if (q.op === 0x42 && p.op === 0x86) { if (q.a >= 0n && q.a < 32n) r = [W, const32(q.a), plain(0x74)]; }   // i64.shl by a constant
      else if (q.op === 0x42 && TO32[p.op]) r = [W, const32(q.a), plain(TO32[p.op])];
      else if (q.op === 0x20 && TO32[p.op]) r = [W, q, W, plain(TO32[p.op])];
      if (!r) break;
      l.splice(k - 2, 3, ...r); n++; k -= 2;
    }
  }
  return [n ? Buffer.concat([body.subarray(0, start), ...l.map(i => i.x)]) : body, n];
}

export function narrow(module) {
  if (module.length < 8 || module.readUInt32LE(0) !== 0x6d736100 || module.readUInt32LE(4) !== 1) throw new Error('wasm-narrow: not a wasm module');
  const r = new Reader(module, 8), out = [module.subarray(0, 8)];
  let rewrites = 0;
  while (r.p < module.length) {
    const id = r.byte();
    let data = r.skip(r.u());
    if (id === 10) {
      const c = new Reader(data), bodies = [];
      for (let n = c.u(); n > 0; n--) { const [b, k] = narrowBody(c.skip(c.u())); rewrites += k; bodies.push(Buffer.from(uleb(b.length)), b); }
      data = Buffer.concat([Buffer.from(uleb(bodies.length / 2)), ...bodies]);
    }
    out.push(Buffer.from([id, ...uleb(data.length)]), data);
  }
  return { module: Buffer.concat(out), rewrites };
}

if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) {
  if (process.argv.length !== 4) { console.error('usage: wasm-narrow.mjs IN.wasm OUT.wasm'); process.exit(2); }
  const [, , from, to] = process.argv, { module, rewrites } = narrow(fs.readFileSync(from));
  // (at once: a build that is stopped leaves IN, or all of OUT)
  fs.writeFileSync(to + '.tmp', module);
  fs.renameSync(to + '.tmp', to);
  console.log(`wasm-narrow: ${rewrites} operations of ${to} narrowed to 32 bits`);
}
