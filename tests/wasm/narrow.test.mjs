// narrow.test.mjs - unit tests of packaging/emscripten/tools/wasm-narrow.mjs
//
// usage: node narrow.test.mjs [SEED]
//
// Modules of random functions of three i64s, whose i32 results are wrapped
// i64 expressions (additions, subtractions, products, shifts, bits, loads
// and stores at wrapped addresses, blocks, branches and calls), answer the
// same before and after the narrowing, for random arguments and those at
// the edges of 32 bits; the narrowing does narrow, leaves the other sections
// as they are, and refuses what is no module, a truncated one and an opcode
// it does not know.  Lane 78 runs it.
import { narrow } from '../../packaging/emscripten/tools/wasm-narrow.mjs';

let seed = Number(process.argv[2] || 20261124), checks = 0;
const random = n => { seed = (Math.imul(seed, 1103515245) + 12345) >>> 0; return Math.floor(seed / 4294967296 * n); };
const ok = (condition, what) => { checks++; if (!condition) { console.log('FAIL ' + what); process.exit(1); } };
const u = n => { const o = []; do { let x = n % 128; n = Math.floor(n / 128); if (n) x |= 128; o.push(x); } while (n); return o; };
const s = n => { const o = []; n = BigInt.asIntN(64, BigInt(n)); for (;;) { const x = Number(n & 127n); n >>= 7n; if ((n === 0n && !(x & 64)) || (n === -1n && x & 64)) { o.push(x); return o; } o.push(x | 128); } };
const section = (id, bytes) => [id, ...u(bytes.length), ...bytes];
const EDGES = [0n, 1n, -1n, 8n, 31n, 32n, 0x7fffffffn, 0x80000000n, 0xffffffffn, 0x100000000n, -0x80000000n, 0x7fffffffffffffffn, -0x8000000000000000n, 65528n];
const number = () => (random(3) ? EDGES[random(EDGES.length)] : BigInt.asIntN(64, (BigInt(random(4294967296)) << 32n) | BigInt(random(4294967296))));

// the code of a random i64 expression, and of an i32 one (a wrapped i64)
function e64(d) {
  const r = d <= 0 ? random(2) : random(14);
  if (r === 0) return [0x20, random(3)];
  if (r === 1) return [0x42, ...s(number())];
  if (r < 8) return [...e64(d - 1), ...e64(random(2) ? 0 : d - 1), [0x7c, 0x7d, 0x7e, 0x83, 0x84, 0x85][random(6)]];
  if (r === 8) return [...e64(d - 1), 0x42, ...s(random(80)), [0x86, 0x87, 0x88][random(3)]];
  if (r === 9) return [...e32(d - 1), [0xac, 0xad][random(2)]];
  if (r === 10) return [...address(d - 1), 0x29, 3, ...u(random(64))];                       // i64.load
  if (r === 11) return [0x02, 0x7e, ...e64(d - 1), ...e32(d - 1), 0x0d, 0, 0x1a, ...e64(d - 1), 0x0b];   // block, br_if, drop
  if (r === 12) return [...e32(d - 1), 0x04, 0x7e, ...e64(d - 1), 0x05, ...e64(d - 1), 0x0b];   // if
  return [...e64(d - 1), 0x22, random(3)];                                                   // local.tee
}
function e32(d) { return [...e64(d), 0xa7]; }
function address(d) { return [...e64(d), 0x42, ...s(0xff8), 0x83, 0xa7]; }                   // in the memory
function module(functions) {
  const body = f => [...u(f.length + 2), 0, ...f, 0x0b];
  return Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0,
    ...section(0, [4, 0x6e, 0x6f, 0x74, 0x65, 1, 2, 3]),
    ...section(1, [1, 0x60, 3, 0x7e, 0x7e, 0x7e, 1, 0x7f]),
    ...section(3, [...u(functions.length), ...functions.map(() => 0)]),
    ...section(5, [1, 0, 1]),
    ...section(7, [...u(functions.length), ...functions.flatMap((f, i) => { const n = Buffer.from('f' + i); return [n.length, ...n, 0, ...u(i)]; })]),
    ...section(10, [...u(functions.length), ...functions.flatMap(body)])]);
}
const sections = m => { const r = []; let p = 8; while (p < m.length) { const id = m[p++]; let n = 0, k = 1, x; do { x = m[p++]; n += (x & 127) * k; k *= 128; } while (x & 128); r.push([id, m.subarray(p, p + n).toString('hex')]); p += n; } return r; };

let narrowed = 0, calls = 0;
for (let round = 0; round < 40; round++) {
  const functions = Array.from({ length: 40 }, () => (random(4) ? [] : [...address(2), ...e64(3), 0x37, 3, ...u(random(64))]).concat(e32(5)));
  const before = module(functions);
  ok(WebAssembly.validate(before), 'the test makes a module that is not valid');
  const { module: after, rewrites } = narrow(before);
  narrowed += rewrites;
  ok(WebAssembly.validate(after), 'the narrowed module is not valid');
  ok(JSON.stringify(sections(before).filter(x => x[0] !== 10)) === JSON.stringify(sections(after).filter(x => x[0] !== 10)), 'another section has changed');
  ok(narrow(after).module.length <= after.length, 'the narrowing again makes it larger');
  for (let n = 0; n < 12; n++) {
    const args = [number(), number(), number()];
    const a = new WebAssembly.Instance(new WebAssembly.Module(before)).exports, b = new WebAssembly.Instance(new WebAssembly.Module(after)).exports;
    for (let i = 0; i < functions.length; i++) { calls++; ok(a['f' + i](...args) === b['f' + i](...args), `round ${round}, f${i}(${args}): ${b['f' + i](...args)}, not ${a['f' + i](...args)}`); }
  }
}
ok(narrowed > 1000, `only ${narrowed} operations were narrowed`);
const refuses = (m, what) => { let e = null; try { narrow(m); } catch (x) { e = x; } ok(e && /wasm-narrow/.test(e.message), what + ': ' + (e && e.message)); };
refuses(Buffer.from('not a module'), 'no module');
const whole = module([[0x41, 1]]);
refuses(whole.subarray(0, whole.length - 2), 'a truncated module');
refuses(module([[0xff, 0x41, 1]]), 'an unknown opcode');
console.log(`ok   wasm-narrow.mjs: ${calls} calls of ${narrowed} narrowed operations, ${checks} checks`);
