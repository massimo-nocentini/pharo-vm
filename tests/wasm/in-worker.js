// in-worker.js - run the node CLI of the Pharo VM on wasm inside a worker thread
//
// usage: node in-worker.js STACK_MB build-wasm/node/pharo.js [ARG ...]
//
// Browser workers get a much smaller native stack than node's main thread
// (about 1 MB).  This runs pharo.js in a worker_threads Worker limited to
// STACK_MB megabytes of stack, with ARG ... as its command line, and exits
// with the VM's exit status.  The VM keeps the real file descriptors
// (NODERAWFS), so its output is not relayed through the worker's streams.

'use strict';
const { Worker } = require('worker_threads');
const path = require('path');

const [stackMb, tool, ...args] = process.argv.slice(2);
if (!tool) {
  console.error('usage: node in-worker.js STACK_MB TOOL.js [ARG ...]');
  process.exit(2);
}

const code = `
  const { workerData } = require('worker_threads');
  process.argv = [process.execPath, workerData.tool, ...workerData.args];
  require(workerData.tool);
`;
const w = new Worker(code, {
  eval: true,
  workerData: { tool: path.resolve(tool), args },
  resourceLimits: { stackSizeMb: Number(stackMb) || 1 },
});
w.on('error', e => { console.error('in-worker: ' + ((e && e.stack) || e)); process.exitCode = 1; });
w.on('exit', status => { if (process.exitCode === undefined || process.exitCode === 0) process.exitCode = status; });
