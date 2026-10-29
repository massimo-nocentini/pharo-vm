// storage.test.mjs - unit tests of packaging/emscripten/web/vm-storage.js and catalog.js
//
// usage: node storage.test.mjs
//
// Runs the slots of vm-storage.js (PharoStorage) on its in-memory backend
// (PharoStorage.memory, the store of the worker harness): list, last and
// setLast; a new slot is never main, not even in an empty store, so that a
// page of a build of before, which saves into the keys of main without a
// guard, overwrites no image made since; two pages saving at once make a
// slot each; a save, a load and a sync per slot, the guards
// of syncChanges and annotate per slot (a save of one slot leaves the
// other's sync alone); rename, which leaves the meta as it is; remove, of
// the six keys of a slot and of 'last' when it names it; a meta of before
// the slots (no slot) is main's; the tree of a slot (an empty directory, a
// symbolic link, modes, times, a path in Unicode) and its paths
// (checkPath refuses '..'); an overwrite keeps the createdAt and origin of
// its slot, without kept too (a page superseded), and gives a slot of
// before none; a save without a tree keeps the one stored,
// and with null deletes it; a backend without keys() lists main only.
// Then the choice of a git proxy (git-proxy.js, PharoGitProxy.parse and
// describe): a proxy typed without its // is written as the browser writes
// it, which the worker takes; the notices of the first request through the
// default, of the first credentials through a proxy (once per proxy), of
// credentials refused through the site's default, and of the failures (a
// 403 of the proxy, not of the host).
// Then the catalog of catalog.js (PharoCatalog) on a manifest with images
// and on one of before them.  Prints every check and their count, and
// exits with status 1 if any fails.  Lane 70
// (tests/wasm/lanes/70-worker-harness.sh) runs it.

import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const web = path.join(here, '..', '..', 'packaging', 'emscripten', 'web');
const PharoStorage = require(path.join(web, 'vm-storage.js'));
const PharoCatalog = require(path.join(web, 'catalog.js'));
const { MAIN, SLOT_ID, keyOf } = PharoStorage;

let failures = 0, passes = 0;
async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.stack) || e));
  }
}
function eq(got, want, what) {
  if (JSON.stringify(got) !== JSON.stringify(want))
    throw new Error(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
}
function assert(c, what) { if (!c) throw new Error('assertion failed: ' + what); }

const bytes = (n, fill = 0) => new Uint8Array(n).fill(fill);
const fresh = () => { const m = PharoStorage.memory(); return { m, st: PharoStorage.open(m) }; };
const text = async blob => new TextDecoder().decode(new Uint8Array(await blob.arrayBuffer()));
const sids = m => [...m.map.keys()].map(k => /^slot\/([^/]+)\/meta$/.exec(k)).filter(Boolean).map(x => x[1]);

await check('keyOf: main has the keys of before, a slot its prefix; SLOT_ID', async () => {
  eq([keyOf(MAIN, 'meta'), keyOf(MAIN, 'Pharo.image'), keyOf('a-1-b', 'Pharo.tree')],
     ['meta', 'Pharo.image', 'slot/a-1-b/Pharo.tree'], 'keys');
  const id = PharoStorage.newId(Date.now());
  assert(SLOT_ID.test(id) && SLOT_ID.test(MAIN) && id !== MAIN, 'a new id is a slot id: ' + id);
  for (const bad of ['', 'Main', 'a/b', '../meta', 'a-b', 'x'.repeat(13) + '-1-1'])
    assert(!SLOT_ID.test(bad), 'not a slot id: ' + bad);
});

await check('an empty store: no slot, no last, no meta, load answers null', async () => {
  const { st } = fresh();
  eq(await st.list(), [], 'list');
  eq(await st.last(), null, 'last');
  eq(await st.meta(), null, 'meta');
  eq(await st.load(), null, 'load');
});

await check('a new slot is never main, not in an empty store either; list newest first; names', async () => {
  const { m, st } = fresh();
  const a = await st.save(bytes(3), bytes(2), { build: 'x', pharo: '14.0', origin: 'fresh:p14' }, null, null,
                          { slot: null, name: 'Pharo 14' });
  assert(a.slot !== MAIN && SLOT_ID.test(a.slot) && a.image === 'slot/' + a.slot + '/Pharo.image' &&
         a.changes === 'slot/' + a.slot + '/Pharo.changes', 'a slot of its own: ' + a.slot);
  eq([...m.map.keys()].filter(k => !k.startsWith('slot/')), [], 'no key of main');
  eq(m.map.get(keyOf(a.slot, 'Pharo.name')), 'Pharo 14', 'its name');
  await new Promise(r => setTimeout(r, 3));
  const b = await st.save(bytes(4), bytes(2), { build: 'x', pharo: '13.1', origin: 'fresh:p13' }, null, null,
                          { slot: null, name: 'Mine' });
  assert(b.slot !== MAIN && SLOT_ID.test(b.slot) && b.image === 'slot/' + b.slot + '/Pharo.image', 'a slot: ' + b.slot);
  // without target or kept: a new slot too
  await new Promise(r => setTimeout(r, 3));
  const c = await st.save(bytes(5), bytes(2), { build: 'x' });
  assert(c.slot !== MAIN && c.slot !== b.slot && c.slot !== a.slot, 'another slot: ' + c.slot);
  const l = await st.list();
  eq(l.map(s => s.slot), [c.slot, b.slot, a.slot], 'newest first');
  eq(l.map(s => s.name), ['Saved image', 'Mine', 'Pharo 14'], 'names');
  eq([l[1].pharo, l[1].origin, l[1].imageSize, l[1].changesSize, l[1].sourcesSize, l[1].treeFiles],
     ['13.1', 'fresh:p13', 4, 2, 0, 0], 'info');
  assert(l[1].createdAt === l[1].savedAt, 'createdAt of a new slot');
});

await check('a new slot without a name: the label of its version, else Saved image', async () => {
  const { st } = fresh();
  const r = await st.save(bytes(1), bytes(1), { pharo: null }, null, null, { slot: null });
  const s = await st.save(bytes(1), bytes(1), { pharo: '15.0', origin: 'fresh:p15' }, null, null, { slot: null });
  const t = await st.save(bytes(1), bytes(1), { pharo: '13.1', origin: 'fresh:p13' }, null, null, { slot: null });
  const names = Object.fromEntries((await st.list()).map(i => [i.slot, i.name]));
  eq([names[r.slot], names[s.slot], names[t.slot]], ['Saved image', 'Pharo 15', 'Pharo 13.1'], 'names');
});

await check('two pages make a new slot at once: a slot each, neither main', async () => {
  const { m, st } = fresh();
  const other = PharoStorage.open(m);
  const [x, y] = await Promise.all([st.save(bytes(3), bytes(1), {}, null, null, { slot: null, name: 'X' }),
                                    other.save(bytes(4), bytes(1), {}, null, null, { slot: null, name: 'Y' })]);
  assert(x.slot !== y.slot && x.slot !== MAIN && y.slot !== MAIN, 'two slots: ' + x.slot + ' ' + y.slot);
  eq((await st.list()).map(s => s.name).sort(), ['X', 'Y'], 'both listed');
  eq(m.map.get(keyOf(x.slot, 'Pharo.image')).size + m.map.get(keyOf(y.slot, 'Pharo.image')).size, 7,
     'both images there');
  // the guard of the backend (syncChanges, annotate and the kept .sources use it)
  const xm = keyOf(x.slot, 'meta');
  assert(!(await m.write([[xm, { id: 'q' }]], { key: xm, id: null })), 'refused while there is a meta');
  assert(await m.write([['nothing', 1]], { key: 'absent', id: null }), 'passes while there is none');
});

await check('a page of a build of before saves into main: no image made here is overwritten', async () => {
  const { m, st } = fresh();
  const a = await st.save(bytes(3, 1), bytes(2), { pharo: '14.0', origin: 'fresh:p14' }, null, null,
                          { slot: null, name: 'Pharo 14' });
  // what store.save of the build of before writes: the keys of main, no guard
  await m.write([['Pharo.image', new Blob([bytes(5, 9)])], ['Pharo.changes', new Blob([bytes(1)])],
                 ['meta', { id: 'old', image: 'Pharo.image', changes: 'Pharo.changes', imageSize: 5, changesSize: 1,
                            savedAt: Date.now() + 10, build: 'old' }], ['Pharo.sources', undefined]]);
  const l = await st.load(null, a.slot);
  eq([l.name, l.image[0], l.image.length], ['Pharo 14', 1, 3], 'the image made here is intact');
  const all = await st.list();
  eq(all.map(s => [s.slot, s.name]), [[MAIN, 'Saved image'], [a.slot, 'Pharo 14']], 'main is the old page\'s');
});

await check('last: setLast while its slot is there, else the newest slot; load(undefined) boots it', async () => {
  const { m, st } = fresh();
  const a = await st.save(bytes(3), bytes(1), {}, null, null, { slot: null });
  await new Promise(r => setTimeout(r, 3));
  const b = await st.save(bytes(4), bytes(1), {}, null, null, { slot: null });
  eq(await st.last(), b.slot, 'the newest');
  await st.setLast(a.slot);
  eq(m.map.get('last').slot, a.slot, 'stored');
  eq(await st.last(), a.slot, 'the one set');
  eq((await st.load()).image.length, 3, 'load boots it');
  await st.remove(a.slot);
  assert(!m.map.has('last'), 'remove deletes the last that names it');
  eq(await st.last(), b.slot, 'then the newest');
  await st.setLast(b.slot);
  await st.remove(a.slot);                // (already gone)
  eq(m.map.get('last').slot, b.slot, 'last stays when another slot goes');
  let e = null;
  try { await st.setLast('../x'); } catch (x) { e = x; }
  assert(e, 'no slot id refused');
});

await check('save, load and overwrite per slot; the latest save wins; createdAt and origin stay', async () => {
  const { m, st } = fresh();
  const a = await st.save(bytes(3, 1), bytes(2, 1), { origin: 'fresh:p15' }, null, null, { slot: null, name: 'A' });
  const b = await st.save(bytes(4, 2), bytes(2, 2), { origin: 'fresh:p13' }, null, null, { slot: null, name: 'B' });
  const la = await st.load(null, a.slot), lb = await st.load(null, b.slot);
  eq([la.image[0], la.image.length, la.name, lb.image[0], lb.image.length, lb.name], [1, 3, 'A', 2, 4, 'B'], 'loaded');
  await new Promise(r => setTimeout(r, 3));
  const b2 = await st.save(bytes(6, 3), bytes(2), {}, null, lb.meta, { slot: b.slot });
  eq([b2.slot, b2.createdAt === b.createdAt, b2.origin, b2.id !== b.id], [b.slot, true, 'fresh:p13', true], 'overwritten');
  eq((await st.load(null, b.slot)).image[0], 3, 'its image');
  eq((await st.load(null, a.slot)).image[0], 1, 'A untouched');
  eq(m.map.get(keyOf(b.slot, 'Pharo.name')), 'B', 'the name stays');
  // an overwrite with a stale kept (another page saved meanwhile): written anyway
  const b3 = await st.save(bytes(7, 4), bytes(2), {}, null, lb.meta, { slot: b.slot });
  eq((await st.meta(b.slot)).id, b3.id, 'the latest wins');
  // an overwrite without kept (a page superseded): the createdAt and origin stored stay
  const b4 = await st.save(bytes(7, 5), bytes(2), {}, null, null, { slot: b.slot });
  eq([b4.createdAt === b.createdAt, b4.origin], [true, 'fresh:p13'], 'without kept');
  const a2 = await st.save(bytes(3), bytes(2), { pharo: '15.0' }, null, null, { slot: a.slot });
  await st.rename(a.slot, '');
  eq([a2.createdAt === a.createdAt, a2.origin, (await st.list()).find(x => x.slot === a.slot).name],
     [true, 'fresh:p15', 'Pharo 15'], 'A keeps its origin: no Saved image');
  // an overwrite of a slot deleted meanwhile recreates it
  await st.remove(b.slot);
  await st.save(bytes(8), bytes(2), {}, null, b3, { slot: b.slot });
  eq((await st.load(null, b.slot)).image.length, 8, 'recreated');
  eq(await st.load(null, 'zzz-1-1'), null, 'a slot that is not there');
});

await check('syncChanges and annotate are guarded per slot: a save of one slot leaves the other alone', async () => {
  const { m, st } = fresh();
  const a = await st.save(bytes(3), bytes(2), {}, null, null, { slot: null });
  const b = await st.save(bytes(3), bytes(2), {}, null, null, { slot: null });
  await st.save(bytes(3), bytes(2), {}, null, a, { slot: a.slot });       // another page saves A
  const sb = await st.syncChanges(bytes(9), b);
  assert(sb && sb.changesSize === 9 && m.map.get(keyOf(b.slot, 'Pharo.changes')).size === 9, 'B synced');
  eq(await st.syncChanges(bytes(5), a), null, 'A superseded');
  const nb = await st.annotate(sb, { sourcesName: 'Pharo13.1-64bit-d7c6f76.sources', pharo: '13.1', imageId: 'p13' });
  assert(nb && nb.id === sb.id && nb.pharo === '13.1', 'annotated, same id');
  eq((await st.meta(b.slot)).imageId, 'p13', 'stored');
  const again = await st.syncChanges(bytes(10), nb);
  eq([again.pharo, again.changesSize], ['13.1', 10], 'a sync keeps the annotation');
  eq(await st.annotate(a, { pharo: '1.0' }), null, 'annotate of a superseded meta');
});

await check('rename writes the name only; remove deletes the six keys of a slot', async () => {
  const { m, st } = fresh();
  const tree = { entries: [['f', 'f', 0o100644, 1, 0, 1, null]], data: bytes(1), size: 1, files: 1 };
  const a = await st.save(bytes(3), bytes(2), { tree }, { name: 'Other.sources', data: bytes(4) }, null,
                          { slot: null, name: 'A' });
  const b = await st.save(bytes(3), bytes(2), { tree }, { name: 'Other.sources', data: bytes(4) }, null,
                          { slot: null, name: 'B' });
  await st.rename(b.slot, 'Renamed');
  eq([(await st.meta(b.slot)).id, await st.name(b.slot)], [b.id, 'Renamed'], 'the id stays');
  await st.rename(b.slot, '');
  eq(await st.name(b.slot), null, 'no name');
  const keysOf = s => [...m.map.keys()].filter(k => s === MAIN ? !k.startsWith('slot/') : k.startsWith('slot/' + s + '/'));
  eq(keysOf(b.slot).length, 5, 'B has five keys (no name)');
  eq(keysOf(a.slot).length, 6, 'A has six');
  await st.setLast(a.slot);
  await st.remove(a.slot);
  eq(keysOf(a.slot), [], 'A gone: ' + keysOf(a.slot));
  assert(!m.map.has('last'), 'and last');
  eq(keysOf(b.slot).length, 5, 'B stays');
  eq(keysOf(MAIN), [], 'nothing of main');
  await st.reset();
  eq(m.map.size, 0, 'reset deletes everything');
});

await check('a meta of before the slots (no slot, no origin) is main, listed as Saved image', async () => {
  const { m, st } = fresh();
  await m.write([['Pharo.image', new Blob([bytes(5)])], ['Pharo.changes', new Blob([bytes(1)])],
                 ['meta', { id: 'old', image: 'Pharo.image', changes: 'Pharo.changes', imageSize: 5, changesSize: 1,
                            savedAt: 1, prepared: true }]]);
  const l = await st.list();
  eq(l.map(s => [s.slot, s.name, s.pharo, s.webPackage]), [[MAIN, 'Saved image', null, 1]], 'listed');
  eq(await st.last(), MAIN, 'last');
  const got = await st.load();
  eq([got.meta.id, got.name, got.image.length, got.tree], ['old', 'Saved image', 5, null], 'loaded');
  const s = await st.syncChanges(bytes(3), got.meta);
  assert(s && m.map.get('meta').changesSize === 3, 'synced under the guard of main');
  const n = await st.annotate(s, { sourcesName: 'Pharo15.0-64bit-4e572fe.sources', pharo: '15.0' });
  assert(n && !('slot' in n) && m.map.get('meta').pharo === '15.0', 'annotated: ' + JSON.stringify(n));
  eq((await st.list())[0].name, 'Saved image', 'still Saved image (no origin)');
  // a new slot does not take main
  const b = await st.save(bytes(1), bytes(1), {}, null, null, { slot: null });
  assert(b.slot !== MAIN && m.map.get('meta').id === 'old', 'main stays: ' + b.slot);
  // an overwrite of it, with kept or without, gives it no createdAt (nor origin)
  const o1 = await st.save(bytes(2), bytes(1), {}, null, n, { slot: MAIN });
  const o2 = await st.save(bytes(2), bytes(1), {}, null, null, { slot: MAIN });
  eq([o1.createdAt, o1.origin, o2.createdAt, o2.origin, (await st.list()).find(x => x.slot === MAIN).name],
     [undefined, undefined, undefined, undefined, 'Saved image'], 'overwritten as of before');
});

await check('the tree: an empty directory, a link, modes, times and a Unicode path round trip; checkPath', async () => {
  const { m, st } = fresh();
  const enc = new TextEncoder();
  const one = enc.encode('first file'), two = enc.encode('ünïcødé');
  const entries = [
    ['pharo-local', 'd', 0o40755, 1000, 0, 0, null],
    ['pharo-local/iceberg/r/.git/refs', 'd', 0o40700, 2000, 0, 0, null],
    ['pharo-local/x', 'f', 0o100600, 1700000000123, 0, one.length, null],
    ['dir é/文件.txt', 'f', 0o100644, 3000, one.length, two.length, null],
    ['link', 'l', 0o120777, 4000, 0, 0, 'pharo-local/x'],
  ];
  const data = new Uint8Array(one.length + two.length);
  data.set(one);
  data.set(two, one.length);
  const s = await st.save(bytes(3), bytes(2), { tree: { entries, data, size: data.length, files: entries.length } },
                          null, null, { slot: null, name: 'T' });
  eq([s.tree, s.treeSize, s.treeFiles], [keyOf(s.slot, 'Pharo.tree'), data.length, 5], 'meta');
  const v = m.map.get(keyOf(s.slot, 'Pharo.tree'));
  assert(v.v === 1 && v.data instanceof Blob && v.data.size === data.length, 'stored as one Blob');
  const l = await st.load();
  eq(l.tree.entries, entries, 'entries');
  eq(new TextDecoder().decode(l.tree.data.subarray(entries[3][4], entries[3][4] + entries[3][5])), 'ünïcødé', 'bytes');
  eq((await st.list())[0].treeFiles, 5, 'listed');
  for (const p of ['a', 'a/b', '.git/x', 'dir é/f', 'a..b']) assert(PharoStorage.checkPath(p), 'a path: ' + p);
  for (const p of ['', '/a', 'a/', 'a//b', '.', '..', 'a/../b', '../x', 'a/./b', 'a\0b', 7, null])
    assert(!PharoStorage.checkPath(p), 'not a path: ' + JSON.stringify(p));
});

await check('a save without a tree keeps the one stored; null deletes it; a new slot has none', async () => {
  const { m, st } = fresh();
  const tree = { entries: [['k', 'f', 0o100644, 1, 0, 2, null]], data: bytes(2, 7), size: 2, files: 1 };
  const a = await st.save(bytes(3), bytes(2), { tree }, null, null, { slot: null });
  const T = keyOf(a.slot, 'Pharo.tree');
  const a2 = await st.save(bytes(4), bytes(2), {}, null, a, { slot: a.slot });
  eq([a2.tree, a2.treeSize, a2.treeFiles], [T, 2, 1], 'kept, from kept');
  const a3 = await st.save(bytes(4), bytes(2), { tree: undefined }, null, null, { slot: a.slot });
  eq([a3.tree, a3.treeFiles], [T, 1], 'kept, from the stored meta');
  eq((await st.load()).tree.data[0], 7, 'still loaded');
  const copy = await st.save(bytes(4), bytes(2), {}, null, a3, { slot: null, name: 'copy' });
  assert(!copy.tree && !m.map.has(keyOf(copy.slot, 'Pharo.tree')), 'a new slot without a tree has none');
  const a4 = await st.save(bytes(4), bytes(2), { tree: null }, null, a3, { slot: a.slot });
  assert(!a4.tree && !m.map.has(T), 'null deletes it');
  eq((await st.load(null, a.slot)).tree, null, 'no tree');
});

await check('the .sources of a slot: kept as stored by a save of that slot, written for a new slot', async () => {
  const { m, st } = fresh();
  const src = { name: 'Other.sources', data: new Blob([bytes(11)]) };
  const a = await st.save(bytes(3), bytes(2), {}, src, null, { slot: null });
  const stored = m.map.get(keyOf(a.slot, 'Pharo.sources'));
  const a2 = await st.save(bytes(4), bytes(2), {}, src, a, { slot: a.slot });
  assert(m.map.get(keyOf(a.slot, 'Pharo.sources')) === stored && a2.sourcesSize === 11, 'kept as stored');
  const b = await st.save(bytes(4), bytes(2), {}, src, a2, { slot: null, name: 'save as' });
  assert(b.slot !== a.slot && m.map.get(keyOf(b.slot, 'Pharo.sources')).size === 11, 'written for the new slot');
  eq((await st.load(null, b.slot)).sources.name, 'Other.sources', 'loaded');
  eq((await st.list()).find(s => s.slot === b.slot).sourcesSize, 11, 'its own, listed');
  m.map.delete(keyOf(b.slot, 'Pharo.sources'));
  let e = null;
  try { await st.load(null, b.slot); } catch (x) { e = x; }
  assert(e && /\.sources of the saved image is missing/.test(e.message), 'a missing .sources: ' + e);
});

await check('a backend without keys(): only main is listed', async () => {
  const { m } = fresh();
  const plain = { get: m.get, write: m.write, clear: m.clear };
  const st = PharoStorage.open(plain);
  await m.write([['Pharo.image', new Blob([bytes(1)])], ['Pharo.changes', new Blob([bytes(1)])],
                 ['meta', { id: 'old', image: 'Pharo.image', changes: 'Pharo.changes', imageSize: 1, changesSize: 1 }]]);
  const b = await st.save(bytes(1), bytes(1), {}, null, null, { slot: null });
  assert(b.slot !== MAIN, 'saved: ' + b.slot);
  eq((await st.list()).map(s => s.slot), [MAIN], 'main only');
  eq((await PharoStorage.open(m).list()).length, 2, 'both with keys()');
});

await check('persist and persisted without navigator.storage: false', async () => {
  const { st } = fresh();
  eq([await st.persist(), await st.persisted()], [false, false], 'false');
});

// ---- git-proxy.js

const PharoGitProxy = require(path.join(web, 'git-proxy.js'));

await check('PharoGitProxy.parse: the URL as the browser writes it, which the worker takes', async () => {
  const takes = p => /^https?:\/\//i.test(p);       // vm-worker.js proxyOf
  for (const [typed, value, origin] of [
    ['https:proxy.example/git', 'https://proxy.example/git', 'https://proxy.example'],
    ['https:/proxy.example/p/', 'https://proxy.example/p/', 'https://proxy.example'],
    ['HTTPS://Proxy.Example:443/a?b=c', 'https://proxy.example/a?b=c', 'https://proxy.example'],
    ['https://cors.isomorphic-git.org', 'https://cors.isomorphic-git.org/', 'https://cors.isomorphic-git.org'],
    [' http://127.0.0.1:9/ ', 'http://127.0.0.1:9/', 'http://127.0.0.1:9'],
  ]) {
    const p = PharoGitProxy.parse(typed);
    eq([p.value, p.origin], [value, origin], typed);
    assert(takes(p.value), 'the worker takes ' + p.value);
  }
  eq(PharoGitProxy.parse('').value, '', 'none');
  for (const bad of ['ftp://x/', 'https://u:p@x/', 'https://x/#f', 'nope'])
    assert(PharoGitProxy.parse(bad).error, 'refused: ' + bad);
});

await check('PharoGitProxy.describe: the first request through the default once, and failures', async () => {
  const req = { event: 'request', host: 'github.com', proxy: 'https://cors.example', status: 200 };
  assert(/public CORS proxy https:\/\/cors\.example, which sees what you clone/.test(PharoGitProxy.describe(req)), 'told');
  eq(PharoGitProxy.describe(req), '', 'once');
  eq(PharoGitProxy.describe({ event: 'request', host: 'h', proxy: '', status: 200 }), '', 'no proxy: nothing');
  assert(/blocked git's request to github\.com \(CORS\)/.test(
    PharoGitProxy.describe({ event: 'failed', host: 'github.com', proxy: '', status: -1 })), 'CORS');
  assert(/did not answer/.test(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p', status: -2 })),
         'a proxy down');
  assert(/refused the request to h\./.test(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p',
                                                                    status: 403 })), '403');
  eq(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p', status: 404 }), '', '404: the error says it');
  eq(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p', status: 403, proxyRefused: false }), '',
     "a host's 403 through the proxy: the error says it");
  assert(/refused the request to h\./.test(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p',
                                                                    status: 403, proxyRefused: true })), "the proxy's 403");
});

await check('PharoGitProxy.describe: credentials refused through the default, and the first ones through a proxy once', async () => {
  const refused = PharoGitProxy.describe({ event: 'failed', host: 'github.com', proxy: 'https://cors.example', status: 401,
                                           credentials: 'refused' });
  assert(/^github\.com asks for a user name and token, .*the public proxy https:\/\/cors\.example, would see them/.test(refused),
         'refused: ' + refused);
  eq(PharoGitProxy.describe({ event: 'failed', host: 'h', proxy: 'https://p', status: 401 }), '', 'a 401: the error says it');
  const sent = { event: 'credentials', host: 'github.com', proxy: 'https://mine.example', status: 200 };
  assert(/github\.com asked for go through the git proxy https:\/\/mine\.example, which can read them/.test(PharoGitProxy.describe(sent)),
         'told');
  eq(PharoGitProxy.describe(sent), '', 'once');
  eq(PharoGitProxy.describe({ event: 'credentials', host: 'h', proxy: '', status: 200 }), '', 'no proxy: nothing');
  assert(PharoGitProxy.describe({ event: 'credentials', host: 'h', proxy: 'https://other.example', status: 200 }), 'another proxy: told');
  const w = PharoGitProxy.useForCredentials('https://cors.example');
  eq([w.mode, w.value, PharoGitProxy.initValue()], ['custom', 'https://cors.example/', 'https://cors.example/'], 'My proxy');
  PharoGitProxy.write('default');
});

// ---- catalog.js

const file = (p, url, size = 10) => ({ path: p, url, size, gzSize: size / 2, sha256: 'x' });
const p15 = [file('Pharo.changes', 'image/Pharo.changes.gz'), file('Pharo.image', 'image/Pharo.image.gz', 100),
             file('Pharo15.0-64bit-b85960f.sources', 'image/Pharo15.0-64bit-b85960f.sources.gz', 50)];
const p13 = [file('Pharo.changes', 'image/p13/Pharo.changes.gz'), file('Pharo.image', 'image/p13/Pharo.image.gz', 90),
             file('Pharo13.1-64bit-d7c6f76.sources', 'image/p13/Pharo13.1-64bit-d7c6f76.sources.gz', 40)];
const legacy = file('Pharo15.0-64bit-4e572fe.sources', 'image/sources/Pharo15.0-64bit-4e572fe.sources.gz', 46);
const catalog = {
  build: 'b', files: p15, world: true, webPackage: 5, imageId: 'p15', extraSources: [legacy],
  images: [{ id: 'p13', label: 'Pharo 13.1', pharo: '13.1', pharoBuild: 749, files: p13, world: true, webPackage: 5 },
           { id: 'p15', label: 'Pharo 15', pharo: '15.0', pharoBuild: 50, files: p15, world: true, webPackage: 5,
             default: true }],
};

await check('PharoCatalog: images, defaultEntry, find, sourcesOf, sourcesFile, entryOfFile, downloadSize', async () => {
  eq(PharoCatalog.images(catalog).map(e => e.id), ['p13', 'p15'], 'images');
  eq(PharoCatalog.defaultEntry(catalog).id, 'p15', 'default');
  eq(PharoCatalog.find(catalog, 'p13').label, 'Pharo 13.1', 'find');
  eq(PharoCatalog.find(catalog, 'p99'), null, 'not found');
  eq(PharoCatalog.sourcesOf(PharoCatalog.find(catalog, 'p13')).url, 'image/p13/Pharo13.1-64bit-d7c6f76.sources.gz', 'sourcesOf');
  eq(PharoCatalog.sourcesFile(catalog, 'Pharo13.1-64bit-d7c6f76.sources').size, 40, 'an entry\'s');
  eq(PharoCatalog.sourcesFile(catalog, 'Pharo15.0-64bit-4e572fe.sources').url, legacy.url, 'an extra one');
  eq(PharoCatalog.sourcesFile(catalog, 'Pharo12.0-64bit-7d5f14c.sources'), null, 'not served');
  eq(PharoCatalog.entryOfFile(catalog, p13[2]).id, 'p13', 'entryOfFile');
  eq(PharoCatalog.entryOfFile(catalog, legacy), null, 'an extra one has none');
  eq(PharoCatalog.downloadSize(PharoCatalog.find(catalog, 'p13')), 70, 'downloadSize');
  assert(PharoCatalog.ID.test('p13') && !PharoCatalog.ID.test('-x') && !PharoCatalog.ID.test('P13'), 'ID');
});

await check('PharoCatalog: pharoOf, labelOf; a manifest of before the catalog has one entry', async () => {
  eq(['Pharo13.1-64bit-d7c6f76.sources', 'Pharo14.0-64bit-74f3a1b.sources', 'Pharo12.0-32bit-7d5f14c.sources',
      'Pharo.sources', 'Pharo13.1-64bit-d7c6f76', null].map(PharoCatalog.pharoOf),
     ['13.1', '14.0', '12.0', null, null, null], 'pharoOf');
  eq(['13.1', '14.0', '15.0', null, 'x'].map(PharoCatalog.labelOf), ['Pharo 13.1', 'Pharo 14', 'Pharo 15', 'Pharo', 'Pharo'],
     'labelOf');
  const old = { files: p15, world: true, webPackage: 4 };
  const all = PharoCatalog.images(old);
  eq(all.map(e => [e.id, e.default, e.label, e.pharo, e.pharoBuild, e.world, e.webPackage, e.files === p15]),
     [['default', true, 'Pharo 15', '15.0', null, true, 4, true]], 'synthesized');
  eq(PharoCatalog.defaultEntry(old).id, 'default', 'its default');
  eq(PharoCatalog.images({ files: p13, world: false, imageId: 'p13' }).map(e => [e.id, e.label, e.webPackage]),
     [['p13', 'Pharo 13.1', 0]], 'with imageId, no world');
  eq(PharoCatalog.sourcesFile(old, 'Pharo15.0-64bit-b85960f.sources').size, 50, 'its .sources');
});

console.log(`# ${passes} passed, ${failures} failed, ${passes + failures} checks`);
process.exit(failures ? 1 : 0);
