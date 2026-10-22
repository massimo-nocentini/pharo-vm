// web-bench.js - the SimdJSON benchmark in pharo-web.js (the MEMORY64=2 VM of
// the pages), in node
//
// usage: node web-bench.js WEB_DIR IMAGE_DIR CORPUS_DIR ST [NAME=VALUE ...]
//
// Boots the image of IMAGE_DIR (its only .image, with its .changes and the
// .sources) in MEMFS through WEB_DIR/vm-driver.js, as tests/wasm/vm-harness.js
// does, with the .json and .ndjson files of CORPUS_DIR (subdirectories too)
// under /corpus and ST as /pharo/run.st, runs `st --quit /pharo/run.st`, and
// copies the output of the VM to stdout and stderr.  The environment of the
// image is SJ_CORPUS=/corpus, the SJ_ variables of this process, then the
// NAME=VALUE arguments.  Exits with the status of the VM, 3 on a crash.
'use strict';
const fs = require('fs');
const path = require('path');

const [webDir, imageDir, corpusDir, st, ...settings] = process.argv.slice(2);
if (!st) {
  process.stderr.write('usage: node web-bench.js WEB_DIR IMAGE_DIR CORPUS_DIR ST [NAME=VALUE ...]\n');
  process.exit(2);
}
const createPharoVM = require(path.resolve(webDir, 'pharo-web.js'));
const Driver = require(path.resolve(webDir, 'vm-driver.js'));

const only = ext => {
  const names = fs.readdirSync(imageDir).filter(f => f.endsWith(ext));
  if (names.length !== 1) throw new Error(`web-bench: expected one ${ext} in ${imageDir}`);
  return names[0];
};
const image = only('.image');
const files = [
  { path: '/pharo/Pharo.image', data: fs.readFileSync(path.join(imageDir, image)) },
  { path: '/pharo/Pharo.changes', data: fs.readFileSync(path.join(imageDir, image.replace(/\.image$/, '.changes'))) },
  { path: '/pharo/' + only('.sources'), data: fs.readFileSync(path.join(imageDir, only('.sources'))) },
  { path: '/pharo/run.st', data: fs.readFileSync(st) },
];
const corpus = dir => {
  for (const entry of fs.readdirSync(path.join(corpusDir, dir), { withFileTypes: true })) {
    const name = dir ? dir + '/' + entry.name : entry.name;
    if (entry.isDirectory()) corpus(name);
    else if (/\.(json|ndjson)$/.test(entry.name))
      files.push({ path: '/corpus/' + name, data: fs.readFileSync(path.join(corpusDir, name)) });
  }
};
corpus('');

const env = { SJ_CORPUS: '/corpus' };
for (const [name, value] of Object.entries(process.env))
  if (name.startsWith('SJ_') && name !== 'SJ_CORPUS') env[name] = value;
for (const setting of settings) {
  const i = setting.indexOf('=');
  env[setting.slice(0, i)] = setting.slice(i + 1);
}

(async () => {
  const t0 = performance.now();
  const drv = await Driver.start(createPharoVM, {
    args: ['--headless', '/pharo/Pharo.image', '--no-default-preferences', 'st', '--quit', '/pharo/run.st'],
    files,
    env,
    wasmModule: new WebAssembly.Module(fs.readFileSync(path.resolve(webDir, 'pharo-web.wasm'))),
    schedule: f => setImmediate(f),
    onOutput: (fd, bytes) => (fd === 2 ? process.stderr : process.stdout).write(Buffer.from(bytes)),
    onExit: code => {
      process.stderr.write(`web-bench: exit ${code} after ${((performance.now() - t0) / 1000).toFixed(1)} s\n`);
      process.exit(code);
    },
    onCrash: (message, stack) => {
      process.stderr.write(`web-bench: crash ${message}\n${stack || ''}\n`);
      process.exit(3);
    },
  });
  if (!drv) process.exit(3);
  drv.begin();
})();
