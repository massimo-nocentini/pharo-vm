#!/usr/bin/env node
// wasm-tune.mjs - tunes a linked VM module for engines with branch hinting
//
//   wasm-tune.mjs tune IN.wasm OUT.wasm [PROFILE.json] [--narrow]
//       writes OUT: IN with the branch hints of the profile (the custom section
//       metadata.code.branch_hint), its cold bytecode cases marked, and, with
//       --narrow (a MEMORY64=2 module), its addresses computed in 32 bits.
//   wasm-tune.mjs instrument IN.wasm OUT.wasm SITES.json [--narrow]
//       writes OUT: IN that counts its branches and bytecode cases, in globals
//       that it exports as __bh0, __bh1, ...
//   wasm-tune.mjs profile SITES.json COUNTS.json... PROFILE.json
//       makes the profile of the counts (arrays of the values of __bh*, as
//       tools/wasm-tune-capture.cjs writes them).
//
// A function is found in the profile by the hash of its code without the
// constants, offsets and indices that change from link to link; one that is
// not found gets no hints.  So the profile outlives a relink, not a change of
// the code: then make it again (docs/WebAssembly.md, Branch hints).
//
// Why: an engine compiles `interpret' with every local in a register until a
// call, and spills them around all the calls it thinks may be reached.  Told
// which branches are cold, it moves the spills (and the code) there.
// br_table has no hints: a case that is hardly ever entered starts with an
// always-taken br_if that is hinted unlikely, over a trap.
import fs from 'node:fs';
import crypto from 'node:crypto';

const UNLIKELY = 0.2;      // a branch taken less often than this is unlikely (likely: 1 - it)
const MIN_COUNT = 100;     // branches executed fewer times get no hint
const COLD_CASE = 1e-5;    // a case with less than this part of the entries is cold
const SWITCH = 200;        // the br_tables with this many entries or more: the bytecodes

// --- LEB128
function uleb(n) { const o = []; do { let x = n % 128; n = Math.floor(n / 128); if (n) x |= 128; o.push(x); } while (n); return o; }
function sleb(n) { const o = []; n = BigInt(n); for (;;) { const x = Number(n & 127n); n >>= 7n; if ((n === 0n && !(x & 64)) || (n === -1n && x & 64)) { o.push(x); return o; } o.push(x | 128); } }
class Reader {
  constructor(b, p = 0) { this.b = b; this.p = p; }
  byte() { return this.b[this.p++]; }
  u() { let r = 0, m = 1, x; do { x = this.b[this.p++]; r += (x & 127) * m; m *= 128; } while (x & 128); return r; }
  s() { let r = 0n, s = 0n, x; do { x = this.b[this.p++]; r |= BigInt(x & 127) << s; s += 7n; } while (x & 128); return x & 64 ? r - (1n << s) : r; }
  skip(n) { const q = this.p; this.p += n; return this.b.subarray(q, this.p); }
}

// --- the module
class Module {
  constructor(file) {
    const b = fs.readFileSync(file), r = new Reader(b, 8);
    this.sections = [];
    while (r.p < b.length) { const id = r.byte(), n = r.u(); this.sections.push({ id, data: r.skip(n) }); }
    let t = new Reader(this.data(1)); this.params = [];
    for (let n = t.u(); n > 0; n--) {
      if (t.byte() !== 0x60) throw new Error(`${file}: a type that is not a function`);
      const c = t.u(); t.skip(c); t.skip(t.u()); this.params.push(c);
    }
    this.funcTypes = []; this.importedFunctions = 0; this.importedGlobals = 0;
    t = new Reader(this.data(2));
    const limits = () => { const f = t.byte(); t.u(); if (f & 1) t.u(); };
    for (let n = t.u(); n > 0; n--) {
      t.skip(t.u()); t.skip(t.u());
      switch (t.byte()) {
        case 0: this.funcTypes.push(t.u()); this.importedFunctions++; break;
        case 1: t.byte(); limits(); break;
        case 2: limits(); break;
        case 3: t.skip(2); this.importedGlobals++; break;
        case 4: t.byte(); t.u(); break;
      }
    }
    t = new Reader(this.data(3)); for (let n = t.u(); n > 0; n--) this.funcTypes.push(t.u());
    t = new Reader(this.data(10)); this.bodies = []; for (let n = t.u(); n > 0; n--) this.bodies.push(t.skip(t.u()));
  }
  data(id) { const s = this.sections.find(x => x.id === id); if (!s) throw new Error(`no section ${id}`); return s.data; }
  set(id, data) { this.sections.find(x => x.id === id).data = data; }
  // appends n entries to the vector of section id; answers how many it had
  append(id, n, bytes) { const r = new Reader(this.data(id)), c = r.u(); this.set(id, Buffer.concat([Buffer.from(uleb(c + n)), this.data(id).subarray(r.p), bytes])); return c; }
  write(file, hints) {
    this.set(10, Buffer.concat([Buffer.from(uleb(this.bodies.length)), ...this.bodies.flatMap(b => [Buffer.from(uleb(b.length)), b])]));
    const out = [Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0])];
    for (const { id, data } of this.sections) {
      if (id === 10 && hints && hints.size) {
        const name = Buffer.from('metadata.code.branch_hint'), h = [...uleb(name.length), ...name, ...uleb(hints.size)];
        for (const [f, l] of [...hints].sort((a, b) => a[0] - b[0])) {
          h.push(...uleb(f), ...uleb(l.length));
          for (const [offset, v] of l.sort((a, b) => a[0] - b[0])) h.push(...uleb(offset), 1, v);
        }
        out.push(Buffer.from([0, ...uleb(h.length), ...h]));
      }
      out.push(Buffer.from([id, ...uleb(data.length)]), data);
    }
    fs.writeFileSync(file, Buffer.concat(out));
  }
}

// --- function bodies: { groups: [[n, type]], start, code: [{ o, e, op, a, b }] }
const INDEX_OPS = new Set([0x07, 0x08, 0x09, 0x18, 0x0c, 0x0d, 0x10, 0x12, 0x14, 0x15, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0xd2, 0xd5, 0xd6]);
function decode(body) {
  const r = new Reader(body), groups = [], code = [];
  for (let n = r.u(); n > 0; n--) { const c = r.u(); groups.push([c, r.byte()]); }
  const start = r.p;
  while (r.p < body.length) {
    const i = { o: r.p, op: r.byte() }, op = i.op;
    if (op === 2 || op === 3 || op === 4 || op === 6) i.a = r.s();
    else if (op === 0x1f) { i.a = r.s(); i.t = []; for (let n = r.u(); n > 0; n--) { const k = r.byte(); if (k < 2) r.u(); i.t.push(r.u()); } }
    else if (INDEX_OPS.has(op)) i.a = r.u();
    else if (op === 0x0e) { i.t = []; for (let n = r.u(); n >= 0; n--) i.t.push(r.u()); }
    else if (op === 0x11 || op === 0x13) { i.a = r.u(); i.b = r.u(); }
    else if (op === 0x1c) r.skip(r.u());
    else if (op >= 0x28 && op <= 0x3e) { i.a = r.u(); i.b = r.u(); }
    else if (op === 0x3f || op === 0x40) r.byte();
    else if (op === 0x41 || op === 0x42) i.a = r.s();
    else if (op === 0x43) r.skip(4);
    else if (op === 0x44) r.skip(8);
    else if (op === 0xd0) r.s();
    else if (op === 0xfc) {
      const s = i.a = r.u();
      if (s === 8 || s === 10 || s === 12 || s === 14) { r.u(); r.u(); } else if (s === 9 || s === 11 || s === 13 || (s > 14 && s < 18)) r.u(); else if (s > 17) throw new Error(`opcode fc ${s}`);
    } else if (op === 0xfd) {
      const s = i.a = r.u();
      if (s <= 11 || s === 92 || s === 93) { r.u(); r.u(); } else if (s === 12 || s === 13) r.skip(16); else if (s >= 21 && s <= 34) r.byte(); else if (s >= 84 && s <= 91) { r.u(); r.u(); r.byte(); }
    } else if (op >= 0xd1 && op !== 0xd1 && op !== 0xd3 && op !== 0xd4) throw new Error(`opcode ${op.toString(16)}`);
    i.e = r.p; code.push(i);
  }
  return { groups, start, code };
}

// the ends of the blocks that the big br_tables jump to, as indices in code: the starts of the bytecodes
function caseEnds(code) {
  const open = [], targets = new Set(), end = new Map();
  code.forEach((i, k) => {
    if (i.op === 2 || i.op === 3 || i.op === 4 || i.op === 0x1f) open.push(k);
    else if (i.op === 0x0b) { if (open.length) end.set(open.pop(), k); }
    else if (i.op === 0x0e && i.t.length >= SWITCH)
      for (const d of new Set(i.t)) if (d < open.length && code[open[open.length - 1 - d]].op === 2) targets.add(open[open.length - 1 - d]);
  });
  return [...targets].map(k => end.get(k)).sort((a, b) => a - b);
}
// the sites of a function: its branches, then its cases
function sitesOf(code) { return { branches: code.flatMap((i, k) => i.op === 4 || i.op === 13 ? [k] : []), cases: caseEnds(code) }; }

// the code without what changes from link to link: constants, memory offsets, indices of functions, globals and types
function shape(code) {
  const h = crypto.createHash('sha256');
  for (const i of code) {
    const op = i.op;
    if (op >= 2 && op <= 4) h.update(`${op}:${i.a >= 0n ? 't' : i.a};`);
    else if (op === 0x0c || op === 0x0d || (op >= 0x20 && op <= 0x22)) h.update(`${op}:${i.a};`);
    else if (op === 0x0e) h.update(`${op}:${i.t};`);
    else if (op === 0xfc || op === 0xfd) h.update(`${op}:${i.a};`);
    else h.update(`${op};`);
  }
  return h.digest('hex').slice(0, 20);
}

// --- addresses in 32 bits: i32.wrap_i64 goes before an add, sub, and, or, xor, mul or shl whose last operand is a constant or a local
const TO32 = { 0x7c: 0x6a, 0x7d: 0x6b, 0x83: 0x71, 0x84: 0x72, 0x85: 0x73, 0x7e: 0x6c };
function narrow(m) {
  let n = 0;
  m.bodies.forEach((body, f) => {
    const { start, code } = decode(body);
    if (!code.some(i => i.op === 0xa7)) return;
    const l = code.map(i => ({ op: i.op, a: i.a, x: body.subarray(i.o, i.e) })), W = { op: 0xa7, x: Buffer.from([0xa7]) };
    const const32 = a => { const c = BigInt.asIntN(32, a); return { op: 0x41, a: c, x: Buffer.from([0x41, ...sleb(c)]) }; }, plain = op => ({ op, x: Buffer.from([op]) });
    let changed = false;
    for (let k = 1; k < l.length; k++) {
      while (k >= 1 && l[k].op === 0xa7) {
        const p = l[k - 1], q = l[k - 2];
        if (p.op === 0xac || p.op === 0xad) { l.splice(k - 1, 2); n++; changed = true; k = Math.max(k - 2, 0); continue; }
        if (!q) break;
        let r;
        if (q.op === 0x42 && p.op === 0x86) { if (q.a >= 0n && q.a < 32n) r = [W, const32(q.a), plain(0x74)]; }
        else if (q.op === 0x42 && TO32[p.op]) r = [W, const32(q.a), plain(TO32[p.op])];
        else if (q.op === 0x20 && TO32[p.op]) r = [W, q, W, plain(TO32[p.op])];
        if (!r) break;
        l.splice(k - 2, 3, ...r); n++; changed = true; k -= 2;
      }
    }
    if (changed) m.bodies[f] = Buffer.concat([body.subarray(0, start), ...l.map(i => i.x)]);
  });
  return n;
}

function instrument(inFile, outFile, sitesFile, opt) {
  const m = new Module(inFile); if (opt.narrow) narrow(m);
  const first = m.importedGlobals + new Reader(m.data(6)).u(), sites = []; let n = 0;
  m.bodies.forEach((body, k) => {
    const { groups, start, code } = decode(body), s = sitesOf(code);
    if (!s.branches.length && !s.cases.length) return;
    const tmp = m.params[m.funcTypes[k + m.importedFunctions]] + groups.reduce((a, g) => a + g[0], 0), counter = new Map();
    for (const i of s.branches) counter.set(i, n++);
    for (const i of s.cases) counter.set(i, n++);
    sites.push({ shape: shape(code), branches: s.branches.length, cases: s.cases.length, at: n - s.branches.length - s.cases.length });
    const out = [Buffer.from([...uleb(groups.length + 1), ...groups.flatMap(([c, t]) => [...uleb(c), t]), 1, 0x7f])]; let p = start;
    const isCase = new Set(s.cases);
    for (const [i, c] of [...counter].sort((a, b) => a[0] - b[0])) {
      const { o, e } = code[i], g = first + 2 * c, add = (ix, what) => [...what, 0x23, ...uleb(ix), 0x7c, 0x24, ...uleb(ix)];
      if (isCase.has(i)) { out.push(body.subarray(p, e), Buffer.from(add(g, [0x42, 1]))); p = e; }
      else { out.push(body.subarray(p, o), Buffer.from([0x22, ...uleb(tmp), ...add(g, [0x20, ...uleb(tmp), 0x45, 0x45, 0xad]), ...add(g + 1, [0x42, 1])])); p = o; }
    }
    m.bodies[k] = Buffer.concat([...out, body.subarray(p)]);
  });
  const zero = Buffer.from([0x7e, 1, 0x42, 0, 0x0b]);
  m.append(6, 2 * n, Buffer.concat(Array(2 * n).fill(zero)));
  m.append(7, 2 * n, Buffer.concat(Array.from({ length: 2 * n }, (_, i) => { const nm = Buffer.from(`__bh${i}`); return Buffer.from([...uleb(nm.length), ...nm, 3, ...uleb(first + i)]); })));
  m.write(outFile); fs.writeFileSync(sitesFile, JSON.stringify({ counters: 2 * n, functions: sites }));
  console.log(`wasm-tune: ${n} sites in ${sites.length} functions`);
}

function makeProfile(sitesFile, countFiles, outFile) {
  const { counters, functions } = JSON.parse(fs.readFileSync(sitesFile)), sum = new Array(counters).fill(0);
  for (const f of countFiles) { const c = JSON.parse(fs.readFileSync(f)); if (c.length !== counters) throw new Error(`${f}: ${c.length} counters, not ${counters}`); c.forEach((x, i) => { sum[i] += x; }); }
  const by = new Map();
  for (const f of functions) {   // functions of the same shape count together
    const c = sum.slice(2 * f.at, 2 * (f.at + f.branches + f.cases));
    const g = by.get(f.shape); if (g) c.forEach((x, i) => { g.c[i] += x; }); else by.set(f.shape, { ...f, c });
  }
  const out = {};
  for (const [s, { branches, cases, c }] of by) {
    let hints = '', cold = '';
    for (let i = 0; i < branches; i++) { const t = c[2 * i], n = c[2 * i + 1]; hints += n <= MIN_COUNT ? '-' : t <= UNLIKELY * n ? '0' : t >= (1 - UNLIKELY) * n ? '1' : '-'; }
    const entries = Array.from({ length: cases }, (_, i) => c[2 * (branches + i)]), total = entries.reduce((a, b) => a + b, 0);
    if (total > MIN_COUNT) cold = entries.map(x => x < COLD_CASE * total ? '1' : '0').join('');
    if (/[01]/.test(hints) || /1/.test(cold)) out[s] = cold ? { hints, cold } : { hints };
  }
  const keys = Object.keys(out).sort();
  fs.writeFileSync(outFile, '{\n' + keys.map(k => `${JSON.stringify(k)}: ${JSON.stringify(out[k])}`).join(',\n') + '\n}\n');
  console.log(`wasm-tune: ${keys.length} functions in the profile`);
}

function tune(inFile, outFile, profileFile, opt) {
  const m = new Module(inFile), rewrites = opt.narrow ? narrow(m) : 0, profile = profileFile ? JSON.parse(fs.readFileSync(profileFile)) : {}, hints = new Map();
  let always = -1, matched = 0, coldCases = 0, interpreter = profileFile ? 'not in the profile: make it again' : 'without a profile';
  m.bodies.forEach((body, k) => {
    const { code } = decode(body), p = profile[shape(code)], s = sitesOf(code); if (!p) return;
    if (p.hints.length !== s.branches.length || (p.cold && p.cold.length !== s.cases.length)) return;
    matched++; let l = [], at = [];
    if (s.cases.length > 100 && code.length > 10000) interpreter = 'hinted';
    s.branches.forEach((i, j) => { if (p.hints[j] !== '-') l.push([code[i].o, +p.hints[j]]); });
    if (p.cold) s.cases.forEach((i, j) => { if (p.cold[j] === '1') at.push(code[i].e); });
    if (at.length) {
      if (always < 0) always = m.importedGlobals + m.append(6, 1, Buffer.from([0x7f, 1, 0x41, 1, 0x0b]));
      // block; global.get always; br_if 0 (unlikely); unreachable; end
      const seq = Buffer.from([0x02, 0x40, 0x23, ...uleb(always), 0x0d, 0, 0x00, 0x0b]), br = 3 + uleb(always).length, out = []; let p0 = 0;
      const shifted = o => o + seq.length * at.filter(e => e <= o).length;
      l = l.map(([o, v]) => [shifted(o), v]);
      at.forEach((e, n) => { out.push(body.subarray(p0, e), seq); l.push([e + n * seq.length + br, 0]); p0 = e; });
      m.bodies[k] = Buffer.concat([...out, body.subarray(p0)]); coldCases += at.length;
    }
    if (l.length) hints.set(k + m.importedFunctions, l);
  });
  m.write(outFile, hints);
  const n = [...hints.values()].reduce((a, l) => a + l.length, 0);
  console.log(`wasm-tune: ${n} hints in ${hints.size} functions, ${coldCases} cold cases, ${rewrites} narrowed operations; interpreter ${interpreter}`);
}

const opt = { narrow: process.argv.includes('--narrow') }, [cmd, ...a] = process.argv.slice(2).filter(x => !x.startsWith('--'));
if (cmd === 'tune' && (a.length === 2 || a.length === 3)) tune(a[0], a[1], a[2], opt);
else if (cmd === 'instrument' && a.length === 3) instrument(...a, opt);
else if (cmd === 'profile' && a.length >= 3) makeProfile(a[0], a.slice(1, -1), a[a.length - 1]);
else { console.error('usage: wasm-tune.mjs tune IN OUT [PROFILE] [--narrow] | instrument IN OUT SITES [--narrow] | profile SITES COUNTS... PROFILE'); process.exit(2); }
