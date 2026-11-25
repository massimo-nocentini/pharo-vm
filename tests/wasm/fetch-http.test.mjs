// fetch-http.test.mjs - unit tests of fetch-http.mjs: the filter of /_stats, the cap of the log, /flaky
//
// usage: node tests/wasm/fetch-http.test.mjs
//
// Starts fetch-http.mjs on an ephemeral local port, checks tag filtering,
// the closed counter, the MAX_REQUESTS log cap, array identity, and /_reset.

import http from 'node:http';
import { startFetchHttp, MAX_REQUESTS } from './lib/fetch-http.mjs';

let passes = 0;
let failures = 0;

async function check(name, fn) {
  try {
    await fn();
    passes++;
    console.log('ok - ' + name);
  } catch (e) {
    failures++;
    console.log('not ok - ' + name + '\n  ' + String((e && e.stack) || e));
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg || 'assertion failed');
}

function eq(got, want, what) {
  const a = JSON.stringify(got), b = JSON.stringify(want);
  if (a !== b) throw new Error(`${what || 'mismatch'}: got ${a}, want ${b}`);
}

function request(url, { method = 'GET', agent } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request(url, { method, agent }, res => {
      const chunks = [];
      res.on('data', d => chunks.push(d));
      res.on('end', () => resolve({
        status: res.statusCode,
        body: Buffer.concat(chunks).toString('utf8'),
      }));
    });
    req.on('error', reject);
    req.end();
  });
}

async function getStats(baseUrl, tag, agent) {
  const suffix = tag === undefined ? '' : `?tag=${encodeURIComponent(tag)}`;
  const res = await request(`${baseUrl}/_stats${suffix}`, { agent });
  eq(res.status, 200, '/_stats status');
  return JSON.parse(res.body);
}

const srv = await startFetchHttp({ port: 0 });
const agent = new http.Agent({ keepAlive: true, maxSockets: 32 });

try {
  await check('/_stats?tag= filters requests by url substring and preserves total closed', async () => {
    await request(`${srv.url}/hello?alpha-1`, { agent });
    await request(`${srv.url}/hello?beta-1`, { agent });
    await request(`${srv.url}/hello?alpha-2`, { agent });

    const hang = http.get(`${srv.url}/hang?gamma-1`, { agent: false }, () => {});
    hang.on('error', () => {});
    for (let i = 0; i < 100 && !srv.requests.some(r => r.url.includes('gamma-1')); i++) {
      await new Promise(r => setTimeout(r, 10));
    }
    hang.destroy();
    for (let i = 0; i < 100 && srv.closed < 1; i++) {
      await new Promise(r => setTimeout(r, 10));
    }
    eq(srv.closed, 1, 'closed counter');

    const all = await getStats(srv.url, undefined, agent);
    eq(all.closed, 1, 'all closed');
    eq(all.requests.length, 4, 'all requests count');

    const alpha = await getStats(srv.url, 'alpha', agent);
    eq(alpha.closed, 1, 'alpha closed stays total');
    eq(alpha.requests.map(r => r.url), ['/hello?alpha-1', '/hello?alpha-2'], 'alpha urls');

    const beta = await getStats(srv.url, 'beta', agent);
    eq(beta.closed, 1, 'beta closed stays total');
    eq(beta.requests.map(r => r.url), ['/hello?beta-1'], 'beta urls');

    const none = await getStats(srv.url, 'no-such-tag', agent);
    eq(none.closed, 1, 'empty filter closed stays total');
    eq(none.requests.length, 0, 'empty filter requests');
  });

  await check('/flaky fails the first times per key', async () => {
    for (const key of ['a', 'b']) {
      const answers = [];
      for (let i = 0; i < 3; i++) {
        answers.push(await request(`${srv.url}/flaky?fail=2&key=${key}`, { agent: false }).then(r => r.body, () => 'failed'));
      }
      eq(answers, ['failed', 'failed', 'flaky ok'], 'key ' + key);
    }
  });

  await check('log keeps at most last MAX_REQUESTS entries in the same array and /_reset clears it', async () => {
    const ref = srv.requests;
    const resetRes = await request(`${srv.url}/_reset`, { method: 'POST', agent });
    eq(resetRes.status, 204, '/_reset status');
    assert(srv.requests === ref, 'same array after /_reset');
    eq(srv.requests.length, 0, 'empty after /_reset');
    eq(srv.closed, 0, 'closed 0 after /_reset');

    const extra = 25;
    const total = MAX_REQUESTS + extra;

    // First `extra` sequentially so they are guaranteed to be the oldest entries
    for (let i = 0; i < extra; i++) {
      const r = await request(`${srv.url}/204?seq=${i}-end`, { agent });
      eq(r.status, 204, '204 status');
    }
    // Middle entries in small concurrent batches
    const batchSize = 25;
    for (let i = extra; i < MAX_REQUESTS; i += batchSize) {
      const end = Math.min(i + batchSize, MAX_REQUESTS);
      const batch = [];
      for (let j = i; j < end; j++) {
        batch.push(request(`${srv.url}/204?seq=${j}-end`, { agent }));
      }
      await Promise.all(batch);
    }
    // Last `extra` sequentially so they are guaranteed to be the newest entries
    for (let i = MAX_REQUESTS; i < total; i++) {
      const r = await request(`${srv.url}/204?seq=${i}-end`, { agent });
      eq(r.status, 204, '204 status');
    }

    assert(srv.requests === ref, 'same array after cap');
    eq(srv.requests.length, MAX_REQUESTS, 'capped at MAX_REQUESTS');

    const evictedFirst = await getStats(srv.url, 'seq=0-end', agent);
    eq(evictedFirst.requests.length, 0, 'seq=0 evicted');

    const evictedLastOld = await getStats(srv.url, `seq=${extra - 1}-end`, agent);
    eq(evictedLastOld.requests.length, 0, 'oldest extra evicted');

    const keptNewest = await getStats(srv.url, `seq=${total - 1}-end`, agent);
    eq(keptNewest.requests.length, 1, 'newest kept');
    eq(keptNewest.requests[0].url, `/204?seq=${total - 1}-end`, 'newest url');

    const all = await getStats(srv.url, undefined, agent);
    eq(all.requests.length, MAX_REQUESTS, '/_stats returns capped length');

    await request(`${srv.url}/_reset`, { method: 'POST', agent });
    assert(srv.requests === ref, 'same array after second /_reset');
    eq(srv.requests.length, 0, 'empty after second /_reset');
  });
} finally {
  agent.destroy();
  await srv.close();
}

console.log(`${passes} passed, ${failures} failed`);
if (failures > 0) process.exit(1);
