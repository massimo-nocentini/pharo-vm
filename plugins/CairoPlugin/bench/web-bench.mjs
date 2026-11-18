// web-bench.mjs - the benchmark of the CairoPlugin in pharo-web, the module of
// the pages (-sMEMORY64=2), as lane 74's web step runs the tests there
// (tests/wasm/lanes/74-cairo.sh, docs/CairoPlugin.md)
//
//   node web-bench.mjs <pharo-vm> <web-dir> <image-dir> <run-bench.st>
//
// <web-dir> holds pharo-web.js and pharo-web.wasm (the web directory of a
// WebAssembly build), <image-dir> an image into which tests/load-cairo.st
// loaded the groups Cairo, CairoTests and CairoBenchmarks and which it saved
// (bench/run-bench.sh --wasm-web prepares it with the node VM).  The VM starts
// the image in console mode through packaging/emscripten/web/vm-driver.js
// (the copy in <web-dir> when there is one) with the empty placeholder
// /pharo/libcairo.so.2 in its files (vm-worker.js writes it for the pages,
// vm-driver.js does not: without it CairoLibrary finds no library and the
// plugin cannot bind), then evaluates run-bench.st in the REPL of the
// Console page with CP_BENCH_REPL set (it does not quit) and the variables
// CP_BENCH_* of this process in the environment of the VM (CP_BENCH_VM is
// wasm-web; CP_BENCH_SCENES defaults to b-rects,micro).  It prints the
// output of the evaluation as it comes, and exits with status 0 when it
// ended with 'BENCH-DONE failures 0', else 1.

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const [srcDir, webDir, imageDir, script] = process.argv.slice(2);
if (!script) {
  console.error("usage: node web-bench.mjs <pharo-vm> <web-dir> <image-dir> <run-bench.st>");
  process.exit(2);
}
const createPharoVM = require(path.join(path.resolve(webDir), "pharo-web.js"));
const ownDriver = path.join(path.resolve(webDir), "vm-driver.js");
const Driver = require(fs.existsSync(ownDriver)
  ? ownDriver
  : path.join(path.resolve(srcDir), "packaging", "emscripten", "web", "vm-driver.js"));
const only = ext => fs.readdirSync(imageDir).filter(f => f.endsWith(ext))[0];
const image = only(".image"), sources = only(".sources");
const changes = image.replace(/\.image$/, ".changes");
const env = { CP_BENCH_VM: "wasm-web", CP_BENCH_REPL: "1", CP_BENCH_SCENES: "b-rects,micro" };
for (const [name, value] of Object.entries(process.env)) {
  if (name.startsWith("CP_BENCH_") && name !== "CP_BENCH_VM" && name !== "CP_BENCH_REPL") env[name] = value;
}
const files = [
  { path: "/pharo/Pharo.image", data: fs.readFileSync(path.join(imageDir, image)) },
  { path: "/pharo/Pharo.changes", data: fs.existsSync(path.join(imageDir, changes))
      ? fs.readFileSync(path.join(imageDir, changes)) : new Uint8Array(0) },
  { path: "/pharo/st/web-repl.st", data: fs.readFileSync(path.join(srcDir, "packaging", "emscripten", "st", "web-repl.st")) },
  { path: "/pharo/run-bench.st", data: fs.readFileSync(script) },
  // (the library of CairoLibrary, as vm-worker.js writes it for the pages)
  { path: "/pharo/libcairo.so.2", data: new Uint8Array(0) },
];
if (sources) files.push({ path: "/pharo/" + sources, data: fs.readFileSync(path.join(imageDir, sources)) });

const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
let out = "", exit = null, crash = null, waiters = [], echoFrom = -1;
const notify = () => { const w = waiters; waiters = []; w.forEach(f => f()); };
const prompt = from => new Promise((resolve, reject) => {
  const check = () => {
    if (crash !== null) return reject(new Error("crash: " + crash));
    if (exit !== null) return reject(new Error("exit " + exit));
    if (out.length > from && out.endsWith("st> ")) return resolve();
    waiters.push(check);
  };
  check();
});

(async () => {
  const started = Date.now();
  const drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs("console"),
    env,
    files,
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    cancel: h => clearTimeout(h),
    canRun: () => true,
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      out += text;
      if (fd === 2) process.stderr.write(text);
      else if (echoFrom >= 0) {
        // the lines of the evaluation, as they come (CR as LF)
        const complete = out.lastIndexOf("\n") + 1;
        if (complete > echoFrom) {
          process.stdout.write(out.slice(echoFrom, complete).replace(/\r/g, "\n"));
          echoFrom = complete;
        }
      }
      notify();
    },
    onState: notify,
    onHost: () => {},
    onExit: code => { exit = code; notify(); },
    onCrash: message => { crash = message; notify(); },
    onDiag: () => {},
  });
  drv.begin();
  await prompt(0);
  const at = out.length;
  echoFrom = at;
  drv.feed("Smalltalk compiler evaluate: \x27/pharo/run-bench.st\x27 asFileReference contents\n");
  await prompt(at);
  const rest = out.slice(echoFrom).replace(/st> $/, "");
  if (rest) process.stdout.write(rest.replace(/\r/g, "\n"));
  const done = /^BENCH-DONE failures (\d+)$/m.exec(out.slice(at).replace(/\r/g, "\n"));
  console.error(`web-bench: ${Math.round((Date.now() - started) / 1000)} s, ${done ? "failures " + done[1] : "no BENCH-DONE line"}`);
  process.exit(done && done[1] === "0" ? 0 : 1);
})().catch(e => {
  process.stdout.write(out.slice(-2000));
  console.error("web-bench: " + e.message);
  process.exit(1);
});
