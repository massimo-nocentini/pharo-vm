// vm-harness.js - headless tests of the web VM and its REPL through vm-driver.js
//
// usage: node [v8 flags] vm-harness.js WEB_DIR IMAGE_DIR
//
// WEB_DIR holds pharo-web.js and pharo-web.wasm (build-wasm/web), IMAGE_DIR
// the stock image with its .changes and .sources (build-wasm/image/stock).
// Every session boots a copy of the image in MEMFS as the Console page does:
// /pharo/Pharo.image with the arguments of vmArgs(), which file in
// packaging/emscripten/st/web-repl.st of this tree.  Prints every case and
// their count, and exits with status 1 if any fails.  Lane 60
// (tests/wasm/lanes/60-vm-harness.sh) runs it once in default node and once
// with "--liftoff-only --stack-size=900".

'use strict';
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const webDir = path.resolve(process.argv[2] || 'build-wasm/web');
const imageDir = path.resolve(process.argv[3] || path.join(webDir, '..', 'image', 'stock'));
const srcDir = path.join(__dirname, '..', '..');
const createPharoVM = require(path.join(webDir, 'pharo-web.js'));
const Driver = require(path.join(srcDir, 'packaging', 'emscripten', 'web', 'vm-driver.js'));
const { WAITING, BUSY, EXITED, SLEEPING, HOST_IMAGE_SAVED } = Driver;
const wasmModule = new WebAssembly.Module(fs.readFileSync(path.join(webDir, 'pharo-web.wasm')));
const replSource = fs.readFileSync(path.join(srcDir, 'packaging', 'emscripten', 'st', 'web-repl.st'));
const ENGINE_ERRORS = /RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall/;

const only = (dir, ext) => {
  const names = fs.readdirSync(dir).filter(f => f.endsWith(ext));
  if (names.length !== 1) throw new Error(`expected one ${ext} in ${dir}, found ${names.length}`);
  return names[0];
};
const imageName = only(imageDir, '.image');
const stock = {
  image: fs.readFileSync(path.join(imageDir, imageName)),
  changes: fs.readFileSync(path.join(imageDir, imageName.replace(/\.image$/, '.changes'))),
  sourcesName: only(imageDir, '.sources'),
};
stock.sources = fs.readFileSync(path.join(imageDir, stock.sourcesName));

// The files of the Console page, in MEMFS
function consoleFiles(image = stock.image, changes = stock.changes,
                      sourcesName = stock.sourcesName, sources = stock.sources) {
  return [
    { path: '/pharo/Pharo.image', data: image },
    { path: '/pharo/Pharo.changes', data: changes },
    { path: '/pharo/' + sourcesName, data: sources },
    { path: '/pharo/st/web-repl.st', data: replSource },
  ];
}

let failures = 0, passes = 0;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const now = () => performance.now();
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');

// The callbacks wake the waiters, which therefore cost no CPU while the VM
// sleeps (the sleep case measures it).
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

// One VM.  All output (both fds, in order) goes to `all', stderr also to
// `err'; send(), interrupt() and mark() start a new window for since().
// `scheduled' and `cancels' count the calls of schedule() and cancel() as
// the driver makes them; `onWrite', if set, is called inside the slice after
// every write.  throwNext[name] = n makes the next n calls of the callback
// name throw, once they did their work, as a buggy host would.
async function session(opts = {}) {
  const s = {
    all: '', err: '', states: [], pumps: [], host: [], exit: null, crash: null, diag: '',
    markAt: 0, errAt: 0, stateAt: 0, pumpAt: 0, stateOut: [],
    unacked: 0, high: Infinity, scheduled: 0, cancels: 0, onWrite: null, throwNext: {},
  };
  const dec = { 1: new TextDecoder(), 2: new TextDecoder() };
  const thrower = name => {
    if (s.throwNext[name] > 0) { s.throwNext[name]--; throw new Error('injected ' + name); }
  };
  s.drv = await Driver.start(createPharoVM, {
    args: opts.args || Driver.vmArgs(),
    files: opts.files || consoleFiles(),
    sliceMs: opts.sliceMs,
    wasmModule: opts.noPrecompiled ? undefined : wasmModule,
    schedule: f => { s.scheduled++; setImmediate(() => { s.pumps.push({ t: now(), later: 0 }); f(); }); },
    later: (f, ms) => setTimeout(() => { s.pumps.push({ t: now(), later: ms }); f(); }, ms),
    cancel: h => { s.cancels++; clearTimeout(h); },
    canRun: () => { thrower('canRun'); return s.unacked < s.high; },
    onOutput: (fd, bytes) => {
      const text = dec[fd].decode(bytes, { stream: true });
      s.all += text;
      if (fd === 2) s.err += text;
      s.unacked += bytes.length;
      if (s.onWrite) s.onWrite();
      notify();
    },
    // with the length of the output then
    onState: st => { s.states.push(st); s.stateOut.push(s.all.length); notify(); thrower('onState'); },
    onHost: (kind, text) => { s.host.push({ kind, text }); notify(); thrower('onHost'); },
    onExit: code => { s.exit = { code, hostBefore: s.host.length }; notify(); thrower('onExit'); },
    onCrash: (message, stack, stacks) => { s.crash = { message, stack, stacks }; notify(); thrower('onCrash'); },
    onDiag: t => { s.diag += t; },
  });
  current = s;
  sessions.push(s);
  s.mark = () => {
    s.markAt = s.all.length; s.errAt = s.err.length; s.stateAt = s.states.length; s.pumpAt = s.pumps.length;
  };
  s.since = () => s.all.slice(s.markAt);
  s.errSince = () => s.err.slice(s.errAt);
  s.statesSince = () => s.states.slice(s.stateAt);
  s.send = text => { s.mark(); s.drv.feed(text); };
  s.interrupt = () => { s.mark(); return s.drv.interrupt(); };
  s.expectOut = (re, ms) => waitFor('output ' + re, () => {
    if (s.crash) throw new Error('crash: ' + s.crash.message);
    if (s.exit) throw new Error('exit ' + s.exit.code);
    return re.test(s.since());
  }, ms);
  // a state reported after the mark, which is the state now
  s.expectState = (st, ms) => waitFor('state ' + st, () =>
    s.states.length > s.stateAt && s.states[s.states.length - 1] === st && s.drv.state() === st, ms);
  // the next prompt after the mark, then WAITING
  s.prompt = async ms => { await s.expectOut(/st> $/, ms); await s.expectState(WAITING, ms); };
  s.ended = ms => waitFor('exit or crash', () => s.exit || s.crash, ms);
  if (s.drv) s.drv.begin();
  return s;
}

let current = null;                     // the session shown on failure
const sessions = [];                    // every session, for the engine error check

async function check(name, f) {
  try {
    await f();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.message) || e));
    if (current) console.log('  output since the last input:\n  | ' +
                             current.since().slice(-600).split('\n').join('\n  | '));
  }
}

function assert(c, msg) { if (!c) throw new Error('assertion failed: ' + msg); }
// a result line, right after the input or after the prompt of an earlier line
const val = v => new RegExp('(^|> )' + v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\n', 'm');

(async () => {
  const t0 = now();
  let S, saved;

  await check('1 the first prompt, st> without a newline, and nothing else', async () => {
    S = await session();
    assert(S.drv, 'start answered null: ' + JSON.stringify(S.crash));
    await waitFor('the first prompt', () => S.all.length, 30000);
    await S.expectState(WAITING);
    assert(S.all === 'st> ', 'output ' + JSON.stringify(S.all));
    // web-repl.st is filed in at every boot: not into the .changes (st --no-source)
    const grown = S.drv.FS.stat('/pharo/Pharo.changes').size - stock.changes.length;
    assert(grown < 1024, `the .changes grew by ${grown} bytes`);
    console.log(`#   prompt after ${(now() - t0).toFixed(0)} ms, the .changes grew by ${grown} bytes`);
  });
  if (!S || !S.drv || S.exit || S.crash) { console.log('cannot continue'); process.exit(1); }

  await check('2 3+4 gives 7, then WAITING', async () => {
    S.send('3+4\n');
    await S.prompt();
    assert(S.since() === '7\nst> ', 'output ' + JSON.stringify(S.since()));
  });

  await check('3 a syntax error on stderr, then the REPL goes on', async () => {
    S.send('3 +\n');
    await S.prompt();
    assert(/^Error: CodeError .+\n$/.test(S.errSince()), 'stderr ' + JSON.stringify(S.errSince()));
    S.send('6 * 7\n');
    await S.expectOut(val('42'));
    await S.prompt();
  });

  await check('4 a CR-joined multi-line chunk gives 7', async () => {
    S.send('| a |\ra := 3.\ra + 4\n');
    await S.prompt();
    assert(S.since() === '7\nst> ', 'output ' + JSON.stringify(S.since()));
  });

  await check('5 a line in two feeds is reassembled', async () => {
    S.send('100 fact');
    await S.expectState(WAITING);
    await sleep(100);
    assert(S.since() === '', 'nothing before the end of the line: ' + JSON.stringify(S.since()));
    S.send('orial printString size\n');
    await S.expectOut(val('158'));
    await S.prompt();
  });

  await check('6 two lines in one feed: two results, two prompts', async () => {
    S.send('1 + 1\n2 + 2\n');
    await S.expectOut(/^2\nst> 4\nst> $/);
    await S.expectState(WAITING);
  });

  // Lines of exactly `width' bytes; 5000 bytes is more than one read of
  // web-repl.st (4096) and less than a stdio buffer more
  const paste = async (lines, width) => {
    let text = '', expected = '';
    for (let i = 0; i < lines; i++) {
      const line = `${i} + 1. "`;
      text += line + '-'.repeat(width - line.length - 2) + '"\n';
      expected += `${i + 1}\nst> `;
    }
    assert(text.length === lines * width, 'paste size');
    S.send(text);
    await S.expectOut(new RegExp(`^1\\nst> [^]*${lines}\\nst> $`), 60000);
    await S.expectState(WAITING);
    assert(S.since() === expected, 'output ' + JSON.stringify(S.since().slice(-200)));
  };

  await check('6b a paste of 5000 bytes, more than one read, gives all its results', async () => {
    await paste(125, 40);
  });

  await check('6c a paste of 2000 lines (90 KB) gives 2000 results', async () => {
    await paste(2000, 45);
  });

  await check('7 an error: its class, message and frames on stderr; Halt does not quit', async () => {
    S.send('nil foo\n');
    await S.prompt();
    assert(S.errSince() === 'Error: MessageNotUnderstood receiver of "foo" is nil\n' +
           '  UndefinedObject(Object)>>doesNotUnderstand: #foo\n  UndefinedObject>>DoIt\n',
           'stderr ' + JSON.stringify(S.errSince()));
    S.send('#(1 2) collect: [ :x | x / 0 ]\n');
    await S.prompt();
    assert(/^Error: ZeroDivide\n  SmallInteger>>\/\n  \[\] in UndefinedObject>>DoIt\n/.test(S.errSince()),
           'stderr ' + JSON.stringify(S.errSince()));
    assert(S.errSince().split('\n').length <= 7, 'at most 5 frames');
    S.send('self halt. 3\n');
    await S.prompt();
    assert(/^Error: Halt\n/.test(S.errSince()), 'stderr ' + JSON.stringify(S.errSince()));
    S.send('Warning signal: \'careful\'. 4\n');
    await S.prompt();
    assert(/^Error: Warning careful\n/.test(S.errSince()), 'stderr ' + JSON.stringify(S.errSince()));
  });

  await check('7b an error in another process is reported on stderr; only that process ends', async () => {
    // the headless image would quit (UIManager): WebRepl is the default
    // error handler of every process.  A plain fork would race the REPL: at
    // its priority, 40, the timer may preempt the forked process, which then
    // queues behind the REPL's loop (the image yields on preemption), so the
    // report comes before the next prompt or after it.  Here the order is
    // fixed: the evaluation waits until the forked process ended, or that
    // process runs at userBackgroundPriority, once the REPL waits at its
    // prompt
    const frames = '  [] in UndefinedObject>>DoIt\n  [] in FullBlockClosure(BlockClosure)>>newProcess\n';
    const bg = ' forkAt: Processor userBackgroundPriority. ';
    // during the evaluation: the report, and no prompt of its own
    S.send('| s | s := Semaphore new. [ [ 1/0 ] ensure: [ s signal ] ] fork. s wait. 42\n');
    await S.expectOut(/^Error[^]*\n42\nst> $/);
    await S.expectState(WAITING);
    assert(S.since() === 'Error: ZeroDivide\n  SmallInteger>>/\n  [] in UndefinedObject>>DoIt\n' +
           '  FullBlockClosure(BlockClosure)>>ensure:\n' + frames + '42\nst> ', 'output ' + JSON.stringify(S.since()));
    // while the REPL waits at its prompt, which the report pushed up: it
    // prompts again
    S.send('[ 1/0 ]' + bg + '43\n');
    await S.expectOut(/^43\nst> Error[^]*\nst> $/);
    await S.expectState(WAITING);
    assert(S.since() === '43\nst> Error: ZeroDivide\n  SmallInteger>>/\n' + frames + 'st> ',
           'output ' + JSON.stringify(S.since()));
    // a Delay wakes it
    S.send('[ (Delay forMilliseconds: 100) wait. nil foo ]' + bg + '#forked\n');
    await S.expectOut(/^#forked\nst> Error[^]*\nst> $/, 5000);
    await S.expectState(WAITING);
    assert(S.since() === '#forked\nst> Error: MessageNotUnderstood receiver of "foo" is nil\n' +
           '  UndefinedObject(Object)>>doesNotUnderstand: #foo\n' + frames + 'st> ', 'output ' + JSON.stringify(S.since()));
    // a Halt too; a Warning is reported and its process goes on
    S.send("[ self halt. Transcript show: 'not shown'; cr ]" + bg + '1\n');
    await S.expectOut(/^1\nst> Error[^]*\nst> $/);
    await S.expectState(WAITING);
    assert(S.since() === '1\nst> Error: Halt\n  UndefinedObject(Object)>>halt\n' + frames + 'st> ',
           'output ' + JSON.stringify(S.since()));
    S.send("[ Warning signal: 'careful'. Transcript show: 'went on'; cr ]" + bg + '2\n');
    await S.expectOut(/^2\nst> Error[^]*\nst> went on\n$/);
    await S.expectState(WAITING);
    assert(S.since() === '2\nst> Error: Warning careful\n' + frames + 'st> went on\n', 'output ' + JSON.stringify(S.since()));
    S.send('3 + 4\n');
    await S.prompt();
    assert(S.since() === '7\nst> ', 'the REPL goes on: ' + JSON.stringify(S.since()));
  });

  await check('8 UTF-8 in and out; Transcript goes to stdout', async () => {
    S.send("'héllo €' reversed\n");
    await S.expectOut(val("'€ olléh'"));
    await S.prompt();
    S.send("Transcript show: 'hi'; cr. 3\n");
    await S.prompt();
    assert(S.since() === 'hi\n3\nst> ', 'output ' + JSON.stringify(S.since()));
  });

  // Stop: interrupt() signals WebRepl's semaphore; its watcher terminates
  // the evaluation and says 'Interrupted.' on stderr
  const stop = async (expr, when) => {
    const st = when === 'SLEEPING' ? SLEEPING : BUSY;
    S.send(expr + '\n');
    // the slice that reported st is over, and the driver has planned the next
    await waitFor(when, () => S.statesSince().filter(s => s === st).length >= 2 && S.drv.state() === st);
    const t = now(), scheduled = S.scheduled, cancels = S.cancels;
    assert(S.interrupt() === true, 'interrupt() answers true');
    // a sleeping VM is not left to its timer, which the image would end
    // within 16 ms anyway: the next slice comes at once
    if (st === SLEEPING) {
      assert(S.scheduled === scheduled + 1 && S.cancels === cancels + 1,
             `interrupt() scheduled ${S.scheduled - scheduled} slice(s), cancelled ${S.cancels - cancels} timer(s)`);
    }
    await S.expectOut(/^Interrupted\.\n/, 5000);
    const ms = now() - t;
    assert(ms < 500, `Interrupted. after ${ms.toFixed(0)} ms`);
    await S.prompt();
    S.send('3 + 4\n');
    await S.expectOut(val('7'));
    await S.prompt();
    return ms;
  };

  await check('9 Stop of [true] whileTrue within 500 ms, then the REPL goes on', async () => {
    console.log(`#   ${(await stop('[true] whileTrue', 'BUSY')).toFixed(0)} ms`);
  });

  await check('10 Stop of a send-free counted loop', async () => {
    console.log(`#   ${(await stop('1 to: SmallInteger maxVal do: [ :i | ]', 'BUSY')).toFixed(0)} ms`);
  });

  await check('11 Stop of a loop of sends', async () => {
    console.log(`#   ${(await stop('[ 30 factorial. true ] whileTrue', 'BUSY')).toFixed(0)} ms`);
  });

  await check('12 Stop during a 30 s Delay cuts the sleep short', async () => {
    console.log(`#   ${(await stop('(Delay forSeconds: 30) wait', 'SLEEPING')).toFixed(0)} ms`);
  });

  await check('13 a stale Stop (no evaluation running) prints nothing', async () => {
    S.send('(1 to: 1000) inject: 0 into: [ :a :b | a + b ]\n');
    await S.expectOut(val('500500'));
    await S.prompt();
    const errAt = S.err.length;
    assert(S.interrupt() === true, 'interrupt() answers true');
    await S.expectState(WAITING);
    await sleep(100);
    S.send('1 + 1\n');
    await S.expectOut(val('2'));
    await S.prompt();
    assert(S.err.length === errAt, 'stderr ' + JSON.stringify(S.err.slice(errAt)));
    assert(S.since() === '2\nst> ', 'output ' + JSON.stringify(S.since()));
  });

  await check('13b a Stop right after the result was written prints no Interrupted.', async () => {
    // The interrupt semaphore is signalled inside the slice that wrote the
    // result (vm_interrupt() is async-signal-safe): the watcher runs before
    // the REPL's loop gets back from the evaluation.  The evaluation is over
    // then, and has nothing to stop
    for (let i = 0; i < 3; i++) {
      let signalled = 0;
      S.onWrite = () => {
        if (!/^#r\n$/.test(S.since())) return;
        S.onWrite = null;
        signalled = S.drv.module._vm_interrupt();
      };
      S.send('#r\n');
      try { await S.prompt(); } finally { S.onWrite = null; }
      assert(signalled === 1, 'the semaphore was signalled after the result');
      assert(S.since() === '#r\nst> ', 'output ' + JSON.stringify(S.since()));
    }
  });

  await check('13c a read of stdin with nothing fed answers no bytes, and counts as waiting for input', async () => {
    // the read fails with EAGAIN (not 0, the end of the input): the image
    // goes on, and the slices that follow say WAITING, not SLEEPING
    S.send('| r | r := Stdio stdin next: 10. (Delay forMilliseconds: 300) wait. r\n');
    await S.prompt(5000);
    assert(S.since() === '#[]\nst> ', 'output ' + JSON.stringify(S.since()));
    const before = S.stateOut.slice(S.stateAt).map((n, i) => [n, S.states[S.stateAt + i]]).filter(([n]) => n === S.markAt);
    assert(before.length >= 5 && before.slice(-5).every(([, st]) => st === WAITING),
           'states during the Delay: ' + JSON.stringify(before.map(([, st]) => st)));
    S.send('3 + 4\n');
    await S.prompt();
    assert(S.since() === '7\nst> ', 'the REPL goes on: ' + JSON.stringify(S.since()));
  });

  await check('14 a 1 s Delay sleeps: SLEEPING, >= 10 timed wakeups, CPU/wall < 0.5, then WAITING', async () => {
    const cpu0 = process.cpuUsage(), t = now();
    S.send('(Delay forSeconds: 1) wait. #woke\n');
    await S.expectOut(val('#woke'), 5000);
    const wall = now() - t, cpu = process.cpuUsage(cpu0);
    const load = (cpu.user + cpu.system) / 1000 / wall;
    await S.prompt();
    const timed = S.pumps.slice(S.pumpAt).filter(p => p.later).length;
    const gaps = S.pumps.slice(S.pumpAt).map((p, i, a) => i && p.later ? p.t - a[i - 1].t : null)
      .filter(g => g !== null).sort((a, b) => a - b);
    assert(wall >= 990 && wall <= 2000, `woke after ${wall.toFixed(0)} ms`);
    assert(S.statesSince().includes(SLEEPING), 'SLEEPING reported');
    assert(timed >= 10, `${timed} timed wakeups`);
    assert(load < 0.5, `CPU/wall ${load.toFixed(2)}`);
    console.log(`#   ${wall.toFixed(0)} ms, ${timed} timed wakeups (median gap ` +
                `${gaps[gaps.length >> 1].toFixed(1)} ms), CPU/wall ${load.toFixed(2)}`);
  });

  await check('15 backpressure: 5 MB with canRun() false pauses, and acks resume it', async () => {
    S.high = 1 << 20;
    S.unacked = 0;
    S.send('1 to: 50000 do: [ :i | Stdio stdout nextPutAll: (String new: 99 withAll: $x); lf ]. #done\n');
    await waitFor('blocked on unacked output', () => S.unacked >= S.high && S.drv.state() === BUSY, 60000);
    const n = S.pumps.length;
    await sleep(300);
    assert(S.pumps.length === n, 'no slice while blocked');
    assert(S.drv.state() === BUSY, 'still BUSY');
    assert(!/#done/.test(S.since()), 'not done while blocked');
    // ack everything, as the page does after rendering
    const ack = () => { S.unacked = 0; S.drv.resumeOutput(); };
    const timer = setInterval(ack, 20);
    ack();
    try {
      await S.expectOut(/#done\nst> $/, 120000);
      await S.expectState(WAITING);
    } finally { clearInterval(timer); S.high = Infinity; }
    const lines = S.since().split('\n');
    assert(lines.length === 50002 && lines.slice(0, 50000).every(l => l.length === 99),
           `${lines.length - 2} lines of 99`);
  });

  await check('16 a save: onHost(HOST_IMAGE_SAVED, /pharo/Pharo.image) and a new image in MEMFS', async () => {
    const before = sha256(S.drv.FS.readFile('/pharo/Pharo.image'));
    assert(before === sha256(stock.image), 'MEMFS holds the stock image');
    S.send('Smalltalk at: #WebReplMarker put: 4242. Smalltalk snapshot: true andQuit: false. #saved\n');
    await S.expectOut(val('#saved'), 60000);
    await S.prompt();
    assert(S.host.length === 1 && S.host[0].kind === HOST_IMAGE_SAVED &&
           S.host[0].text === '/pharo/Pharo.image', 'onHost ' + JSON.stringify(S.host));
    const image = S.drv.FS.readFile('/pharo/Pharo.image');
    assert(image.length > 1e7 && sha256(image) !== before, 'the image in MEMFS changed');
    saved = { image, changes: S.drv.FS.readFile('/pharo/Pharo.changes') };
    // the snapshot gave the processes the UIManager as their error handler
    // again: the REPL is theirs once more by its next prompt (the error
    // comes after that prompt, at userBackgroundPriority, as in case 7b)
    S.send('[ 1/0 ] forkAt: Processor userBackgroundPriority. #after\n');
    await S.expectOut(/^#after\nst> Error[^]*\nst> $/);
    await S.expectState(WAITING);
    assert(/^#after\nst> Error: ZeroDivide\n  SmallInteger>>\/\n[^]*\nst> $/.test(S.since()),
           'output ' + JSON.stringify(S.since()));
  });

  await check('17 a VM booted from the saved image shows exactly one prompt', async () => {
    assert(saved, 'no saved image');
    const X = await session({ files: consoleFiles(saved.image, saved.changes) });
    await waitFor('the first prompt', () => X.all.length, 30000);
    await X.expectState(WAITING);
    await sleep(500);
    assert(X.all === 'st> ', 'output ' + JSON.stringify(X.all));
    X.send('Smalltalk at: #WebReplMarker\n');
    await X.expectOut(val('4242'));
    await X.prompt();
    X.send('3+4\n');
    await X.prompt();
    assert(X.since() === '7\nst> ', 'output ' + JSON.stringify(X.since()));
    X.drv.eof();
    await X.ended();
    current = S;
  });

  await check('18 after a memory growth, input and output still work', async () => {
    const M = S.drv.module, view = M.HEAPU8, size = view.length;
    // old space ends at 1 GiB (memoryEmscripten.c, PHARO_WASM_OLD_SPACE_BASE)
    S.send('(ByteArray new: 128 * 1024 * 1024) size\n');
    await S.expectOut(val('134217728'), 60000);
    await S.prompt();
    assert(M.HEAPU8.length > size, `memory ${size} -> ${M.HEAPU8.length}`);
    assert(view.length === 0, 'the old view is detached');
    S.send("'abc' reversed , 'déf'\n");
    await S.expectOut(val("'cbadéf'"));
    await S.prompt();
    console.log(`#   linear memory ${size} -> ${M.HEAPU8.length} bytes`);
  });

  await check('19 EOF: onExit(0) after a last newline, the driver is dead', async () => {
    S.mark();
    S.drv.eof();
    await S.ended();
    assert(S.exit && S.exit.code === 0, 'onExit(0), got ' + JSON.stringify(S.exit) + ' ' + JSON.stringify(S.crash));
    assert(S.since() === '\n', 'output ' + JSON.stringify(S.since()));
    assert(S.drv.dead && S.drv.state() === EXITED, 'driver dead');
    S.drv.feed('3+4\n');                // ignored, must not throw
    assert(S.drv.interrupt() === false, 'interrupt() of a dead driver');
  });

  await check('20 with 1 ms slices the same results; Smalltalk exitFailure: onExit(1)', async () => {
    const X = await session({ sliceMs: 1 });
    await X.prompt();
    // at least 100 ms of work, whatever the speed of the engine: 1 ms slices
    // make it about 100 BUSY slices, the default 20 ms about 5
    X.send('| t | t := Time millisecondClockValue + 100. [ Time millisecondClockValue < t ] whileTrue. ' +
           '(1 to: 300000) inject: 0 into: [ :a :b | a + (b \\\\ 7) ]\n');
    await X.prompt();
    assert(X.since() === '899998\nst> ', 'output ' + JSON.stringify(X.since()));
    const busy = X.statesSince().filter(st => st === BUSY).length;
    assert(busy >= 20, `${busy} BUSY slices`);
    X.send('Smalltalk exitFailure\n');
    await X.ended();
    assert(X.exit && X.exit.code === 1, 'onExit(1), got ' + JSON.stringify(X.exit) + ' ' + JSON.stringify(X.crash));
  });

  await check('21 snapshot: true andQuit: true: the save is notified before onExit(0)', async () => {
    const X = await session();
    await X.prompt();
    X.send('Smalltalk snapshot: true andQuit: true\n');
    await X.ended(60000);
    assert(X.exit && X.exit.code === 0, 'onExit(0), got ' + JSON.stringify(X.exit) + ' ' + JSON.stringify(X.crash));
    assert(X.exit.hostBefore === 1 && X.host[0].text === '/pharo/Pharo.image', 'onHost first: ' + JSON.stringify(X.host));
  });

  await check('22 a truncated image: onExit(non-zero) or onCrash, with a message', async () => {
    const X = await session({ files: consoleFiles(stock.image.subarray(0, 1 << 20)), noPrecompiled: true });
    X.crashExpected = true;
    await X.ended();
    assert((X.exit && X.exit.code !== 0) || X.crash, 'exit ' + JSON.stringify(X.exit));
    const message = (X.err + (X.crash ? X.crash.message : '')).trim();
    assert(message.length > 0, 'a message');
    console.log('#   ' + (X.crash ? 'crash' : `exit ${X.exit.code}`) + ': ' + message.split('\n').join(' / '));
  });

  await check('23 --version: the exit of the first slice is onExit(0), with the version', async () => {
    const X = await session({ args: ['--version'], files: [] });
    await X.ended();
    assert(X.exit && X.exit.code === 0, 'onExit(0), got ' + JSON.stringify(X.exit) + ' ' + JSON.stringify(X.crash));
    assert(/\d+\.\d+/.test(X.all), 'version ' + JSON.stringify(X.all));
  });

  await check('24 interrupt() answers false when the image registered no semaphore', async () => {
    const X = await session({ args: ['--headless', '/pharo/Pharo.image', '--no-default-preferences',
                                     'eval', '(Delay forMilliseconds: 500) wait. 3 + 4'] });
    await waitFor('a slice', () => X.states.length);
    assert(X.drv.interrupt() === false, 'interrupt() answers false');
    await X.ended();
    assert(X.exit && X.exit.code === 0 && X.all === '7\n', 'eval: ' + JSON.stringify(X.all) + ' ' + JSON.stringify(X.exit));
  });

  await check('25 an exception inside a slice is a crash: onCrash(message, stack, stacks)', async () => {
    const X = await session();
    X.crashExpected = true;
    await X.prompt();
    // the next write to stderr throws, as a host bug or a trap would
    const ops = X.drv.FS.getStream(2).stream_ops, write = ops.write;
    ops.write = (...args) => { ops.write = write; throw new TypeError('injected'); };
    X.throwNext.onCrash = 1;            // reported with onDiag
    X.send('1/0\n');
    await X.ended();
    assert(X.crash && X.crash.message === 'injected' && /injected/.test(X.crash.stack),
           'onCrash ' + JSON.stringify(X.crash));
    assert(X.diag.includes('vm-driver: onCrash: Error: injected onCrash'), 'diag ' + JSON.stringify(X.diag.slice(0, 300)));
    assert(/WebRepl/.test(X.crash.stacks), 'the Smalltalk stacks: ' + JSON.stringify(X.crash.stacks.slice(0, 200)));
    assert(X.drv.dead && X.drv.state() === EXITED, 'driver dead');
  });

  await check('26 the image of the page (manifest.json) runs the REPL', async () => {
    const manifest = JSON.parse(fs.readFileSync(path.join(webDir, 'manifest.json'), 'utf8'));
    const files = manifest.files.map(f => {
      const data = zlib.gunzipSync(fs.readFileSync(path.join(webDir, f.url)));
      assert(data.length === f.size && sha256(data) === f.sha256, 'manifest entry ' + f.path);
      return { path: '/pharo/' + f.path, data };
    });
    files.push({ path: '/pharo/st/web-repl.st', data: replSource });
    const X = await session({ args: Driver.vmArgs('/pharo/' + manifest.image), files });
    await X.prompt();
    X.send('3 + 4\n');
    await X.prompt();
    assert(X.since() === '7\nst> ', 'output ' + JSON.stringify(X.since()));
    X.drv.eof();
    await X.ended();
    assert(X.exit && X.exit.code === 0, 'onExit(0)');
  });

  await check('27 a callback of the host that throws goes to onDiag; the VM goes on', async () => {
    // uncaught, it would leave the VM without its next slice, or end node
    const X = await session();
    await X.prompt();
    X.throwNext.onState = 1;
    X.send('3+4\n');
    await X.prompt();
    assert(X.since() === '7\nst> ', 'after onState threw: ' + JSON.stringify(X.since()));
    // a save: onHost, then BUSY slices (canRun) and more states
    X.throwNext = { onHost: 1, canRun: 1, onState: 2 };
    X.send('Smalltalk snapshot: true andQuit: false. 5\n');
    await X.expectOut(val('5'), 60000);
    await X.prompt();
    assert(X.host.length === 1 && X.host[0].kind === HOST_IMAGE_SAVED, 'onHost ' + JSON.stringify(X.host));
    X.throwNext = { onExit: 1 };
    X.drv.eof();
    await X.ended();
    assert(X.exit && X.exit.code === 0, 'onExit(0), got ' + JSON.stringify(X.exit) + ' ' + JSON.stringify(X.crash));
    for (const name of ['onState', 'onHost', 'canRun', 'onExit'])
      assert(X.diag.includes('vm-driver: ' + name + ': Error: injected ' + name),
             name + ' reported: ' + JSON.stringify(X.diag.slice(0, 400)));
    assert(!Object.values(X.throwNext).some(n => n > 0), 'every injected throw happened: ' + JSON.stringify(X.throwNext));
  });

  // the crashes asked for excepted
  sessions.forEach((s, i) => {
    const text = s.all + s.diag + (s.crash ? s.crash.message : '');
    const found = !s.crashExpected && text.match(ENGINE_ERRORS);
    if (found) {
      failures++;
      console.log(`not ok - engine error in session ${i + 1}: ` + found[0]);
    }
  });
  console.log(`# ${passes} passed, ${failures} failed, ${passes + failures} cases in ${((now() - t0) / 1000).toFixed(1)} s`);
  // exit() of the VM sets process.exitCode in node (emscripten's quit_)
  process.exit(failures ? 1 : 0);
})();
