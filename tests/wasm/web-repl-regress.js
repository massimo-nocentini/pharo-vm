// web-repl-regress.js - regression checks of the REPL of the Console page, in node
//
// usage: node web-repl-regress.js WEB_DIR IMAGE_DIR
//
// As vm-harness.js: WEB_DIR holds pharo-web.js and pharo-web.wasm
// (build-wasm/web), IMAGE_DIR the stock image with its .changes and .sources
// (build-wasm/image/stock), and each session boots a copy of the image in
// MEMFS with the arguments of vmArgs('console'), which file in
// packaging/emscripten/st/web-repl.st of this tree.  The checks:
//
// - a Stop that lands on an evaluation that did not start yet, queued
//   behind a process of its priority that an earlier line forked, still
//   lets the REPL go on (it used to wait for that evaluation for good);
// - a Warning that nothing handles is reported, and the evaluation goes on;
// - a syntax error says where it is, and which variable is not declared;
// - the REPL writes PHARO_WEB_WORLD_FILE, whether the image has OSWebDriver,
//   when it starts and before the image is saved.
//
// Prints every case and their count, and exits with status 1 if any fails.
// Lane 62 (tests/wasm/lanes/62-web-repl-regress.sh) runs it.

'use strict';
const fs = require('fs');
const path = require('path');

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const imageDir = path.resolve(process.argv[3] || path.join(webDir, '..', 'image', 'stock'));
const srcDir = path.join(__dirname, '..', '..');
const createPharoVM = require(path.join(webDir, 'pharo-web.js'));
const Driver = require(path.join(srcDir, 'packaging', 'emscripten', 'web', 'vm-driver.js'));
const { WAITING } = Driver;
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const replSource = fs.readFileSync(path.join(srcDir, 'packaging', 'emscripten', 'st', 'web-repl.st'));
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;
const WORLD_FILE = '/pharo/.pharo-web-world';

const only = (dir, ext) => {
  const names = fs.readdirSync(dir).filter(f => f.endsWith(ext));
  if (names.length !== 1) throw new Error(`expected one ${ext} in ${dir}, found ${names.length}`);
  return names[0];
};
const imageName = only(imageDir, '.image');
const sourcesName = only(imageDir, '.sources');
const files = [
  { path: '/pharo/Pharo.image', data: fs.readFileSync(path.join(imageDir, imageName)) },
  { path: '/pharo/Pharo.changes', data: fs.readFileSync(path.join(imageDir, imageName.replace(/\.image$/, '.changes'))) },
  { path: '/pharo/' + sourcesName, data: fs.readFileSync(path.join(imageDir, sourcesName)) },
  { path: '/pharo/st/web-repl.st', data: replSource },
];

let failures = 0, passes = 0, current = null;
const sessions = [];
const now = () => performance.now();

let waiters = [];
function notify() { const w = waiters; waiters = []; for (const f of w) f(); }
function nextEvent(ms) {
  return new Promise(resolve => {
    const timer = setTimeout(resolve, ms);
    waiters.push(() => { clearTimeout(timer); resolve(); });
  });
}
async function waitFor(what, pred, ms = 30000) {
  const t0 = now();
  for (;;) {
    const v = pred();
    if (v) return v;
    const left = ms - (now() - t0);
    if (left <= 0) throw new Error('timed out after ' + ms + ' ms waiting for ' + what);
    await nextEvent(Math.min(left, 250));
  }
}

// One VM: `all' has its output (both fds, in order), `out' and `err' that
// of fd 1 and fd 2; send() starts a new window for since(), outSince() and
// errSince()
async function session(env) {
  const s = { all: '', out: '', err: '', states: [], exit: null, crash: null, diag: '',
              markAt: 0, outAt: 0, errAt: 0, stateAt: 0 };
  const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
  s.drv = await Driver.start(createPharoVM, {
    args: Driver.vmArgs('console'),
    files,
    env: env || {},
    wasmModule,
    schedule: f => setImmediate(f),
    later: (f, ms) => setTimeout(f, ms),
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      s.all += text;
      if (fd === 2) s.err += text; else s.out += text;
      notify();
    },
    onState: st => { s.states.push(st); notify(); },
    onExit: code => { s.exit = code; notify(); },
    onCrash: message => { s.crash = message; notify(); },
    onDiag: t => { s.diag += t; },
  });
  current = s;
  sessions.push(s);
  s.mark = () => {
    s.markAt = s.all.length; s.outAt = s.out.length; s.errAt = s.err.length; s.stateAt = s.states.length;
  };
  s.since = () => s.all.slice(s.markAt);
  s.outSince = () => s.out.slice(s.outAt);
  s.errSince = () => s.err.slice(s.errAt);
  s.send = text => { s.mark(); s.drv.feed(text); };
  s.expectOut = (re, ms) => waitFor('output ' + re, () => {
    if (s.crash) throw new Error('crash: ' + s.crash);
    if (s.exit !== null) throw new Error('exit ' + s.exit);
    return re.test(s.since());
  }, ms);
  s.expectState = (st, ms) => waitFor('state ' + st, () =>
    s.states.length > s.stateAt && s.states[s.states.length - 1] === st && s.drv.state() === st, ms);
  s.prompt = async ms => { await s.expectOut(/st> $/, ms); await s.expectState(WAITING, ms); };
  s.drv.begin();
  await waitFor('the first prompt', () => s.all.length, 60000);
  await s.expectState(WAITING);
  return s;
}

async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
    if (current) console.log('  output since the last input:\n  | ' + current.since().slice(-800).split('\n').join('\n  | '));
  }
}
function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }

(async () => {
  const S = await session({ PHARO_WEB_WORLD_FILE: WORLD_FILE });
  const worldFile = () => {
    try { return new TextDecoder().decode(S.drv.FS.readFile(WORLD_FILE)); } catch (e) { return null; }
  };

  await check('1 a Stop of an evaluation that did not start yet does not wedge the REPL', async () => {
    // Line 1 forks a busy process at the priority of the evaluations, and a
    // helper above the watcher.  The REPL then forks the evaluation of line 2,
    // which waits behind the busy process.  The helper sees it pending, not
    // started, and signals the semaphore of Stop, as vm_interrupt() does;
    // later it ends the busy process.  Each later line must be evaluated.  A
    // preemption of the busy process by another one may let the evaluation
    // start first: then the case is tried again.
    const line1 = "| busy | busy := [ [ true ] whileTrue ] forkAt: Processor userSchedulingPriority. " +
      "[ | pending deadline | deadline := Time millisecondClockValue + 5000. " +
      "[ pending := WebRepl classPool at: #EvalProcess. " +
      "(pending notNil and: [ pending suspendedContext notNil and: [ pending suspendedContext sender isNil and: " +
      "[ pending suspendedContext pc = pending suspendedContext startpc ] ] ]) " +
      "or: [ Time millisecondClockValue > deadline ] ] whileFalse: [ (Delay forMilliseconds: 5) wait ]. " +
      "Smalltalk at: #RegressPending put: pending notNil. " +
      "(WebRepl classPool at: #InterruptSemaphore) signal. " +
      "(Delay forMilliseconds: 300) wait. busy terminate ] forkAt: Processor userInterruptPriority + 1. #forked";
    let pending = false;
    for (let attempt = 1; attempt <= 3 && !pending; attempt++) {
      S.send(line1 + '\n#second\n');
      await S.expectOut(/#forked\n/, 30000);
      await S.prompt(30000);
      const err = S.errSince();
      S.send('Smalltalk at: #RegressPending\n');
      await S.expectOut(/^(true|false)\nst> $/, 30000);
      pending = /^true/.test(S.since());
      await S.prompt();
      if (pending) assert(err === 'Interrupted.\n', 'stderr ' + JSON.stringify(err));
      else console.log(`#   attempt ${attempt}: the evaluation started first, which is not the case`);
      S.send('#third\n');
      await S.expectOut(/^#third\nst> $/, 10000);
      await S.prompt();
    }
    assert(pending, 'the Stop landed on an evaluation that had not started');
  });

  await check('2 a Warning that nothing handles is reported, and the evaluation goes on', async () => {
    S.send("Warning signal: 'careful'. 42\n");
    await S.prompt();
    assert(S.outSince() === '42\nst> ', 'stdout ' + JSON.stringify(S.outSince()));
    assert(S.errSince() === 'Error: Warning careful\n  UndefinedObject>>DoIt\n', 'stderr ' + JSON.stringify(S.errSince()));
    S.send("[ Warning signal: 'handled'. 1 ] on: Warning do: [ :w | w return: 2 ]\n");
    await S.prompt();
    assert(S.since() === '2\nst> ', 'a handled one ' + JSON.stringify(S.since()));
    S.send('nil foo. 3\n');
    await S.prompt();
    assert(S.outSince() === 'st> ' && /^Error: MessageNotUnderstood/.test(S.errSince()),
           'an error still ends it ' + JSON.stringify(S.since()));
  });

  await check('3 a syntax error says where it is, and which variable is not declared', async () => {
    // the class of the errors of the compiler: CodeError in Pharo 12, OCCodeError in Pharo 15
    S.send('SystemVersion current major\n');
    await S.prompt();
    const codeError = parseInt(S.since(), 10) <= 12 ? 'CodeError' : 'OCCodeError';
    const cases = [
      ['x := 3', `Error: ${codeError} Undeclared variable x (line 1, column 1)\n`],
      ['3 + ', `Error: ${codeError} Variable or expression expected (line 1, column 5)\n`],
      ['| a |\ra := 1.\rb := a', `Error: ${codeError} Undeclared variable b (line 3, column 1)\n`],
    ];
    for (const [input, report] of cases) {
      S.send(input + '\n');
      await S.prompt();
      assert(S.errSince() === report, JSON.stringify(input) + ': ' + JSON.stringify(S.errSince()));
    }
  });

  await check('4 PHARO_WEB_WORLD_FILE: whether the image has OSWebDriver, at the start and when saved', async () => {
    assert(worldFile() === 'false', 'at the start: ' + JSON.stringify(worldFile()));
    // defined and saved in one evaluation: the file is written before the save
    S.send("(Object << #OSWebDriver package: 'RegressWorld') install. Smalltalk snapshot: true andQuit: false. " +
           "(FileSystem workingDirectory / '.pharo-web-world') contents\n");
    await S.prompt(60000);
    assert(/'true'\nst> $/.test(S.since()), 'output ' + JSON.stringify(S.since()));
    assert(worldFile() === 'true', 'after the save: ' + JSON.stringify(worldFile()));
    // without the variable, nothing is written
    const N = await session({});
    try {
      assert(!N.drv.FS.analyzePath(WORLD_FILE).exists, 'no file without PHARO_WEB_WORLD_FILE');
    } finally { current = S; }
  });

  for (const s of sessions) {
    if (s.crash || ENGINE_ERRORS.test(s.diag)) {
      failures++;
      console.log('not ok - a session crashed or the engine failed: ' + (s.crash || s.diag.slice(0, 400)));
    }
  }
  console.log(`# web-repl-regress: ${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})().catch(e => { console.log('not ok - ' + ((e && e.stack) || e)); process.exit(1); });
