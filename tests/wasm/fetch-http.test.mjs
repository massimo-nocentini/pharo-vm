// fetch-http.test.mjs - unit tests of fetch-http.mjs: the filter of /_stats, the cap of the log, /flaky, /sse
//
// usage: node tests/wasm/fetch-http.test.mjs
//
// Starts fetch-http.mjs on an ephemeral local port, checks tag filtering,
// the closed counter, the MAX_REQUESTS log cap, array identity, /_reset, and
// the event streams of /sse: their bytes, ids, counters, ends, cuts and timers.

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

// GET of a path, with its chunks as they came: { status, headers, headAt,
// chunks: [{ at, bytes }], body (a Buffer), error, endAt }, the times in ms
// from the request.  until(answer) true aborts it.
function stream(baseUrl, path, { headers = {}, until } = {}) {
  return new Promise(resolve => {
    const t0 = performance.now(), answer = { chunks: [], error: null };
    const done = () => {
      answer.endAt = performance.now() - t0;
      answer.body = Buffer.concat(answer.chunks.map(c => c.bytes));
      resolve(answer);
    };
    const req = http.get(baseUrl + path, { headers, agent: false }, res => {
      Object.assign(answer, { status: res.statusCode, headers: res.headers, headAt: performance.now() - t0 });
      res.on('data', bytes => {
        answer.chunks.push({ at: performance.now() - t0, bytes });
        if (until && until(answer)) {
          req.destroy();
          done();
        }
      });
      res.on('end', done);
      res.on('error', e => { answer.error = e.message; done(); });
    });
    req.on('error', e => { if (!answer.status) { answer.error = e.message; done(); } });
  });
}

const tick = (k, conn, last, eol = '\n', utf8 = false) => ['id: ' + k, 'event: tick',
  'data: ' + JSON.stringify({ k, conn, last }), ...(utf8 ? ['data: caf\u00e9 \u2713 \u{1f642}'] : []), '']
  .map(line => line + eol).join('');
const sleep = ms => new Promise(r => setTimeout(r, ms));

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

  await check('/sse: the head, and the bytes of an event stream', async () => {
    const r = await stream(srv.url, '/sse?key=bytes&n=2&end=close');
    eq([r.status, r.headers['content-type'], r.headers['cache-control'], r.headers['access-control-allow-origin'], r.error],
      [200, 'text/event-stream; charset=utf-8', 'no-store', '*', null], 'head');
    eq(r.body.toString('utf8'), ': conn 0\n' + tick(1, 0, '') + tick(2, 0, ''), 'body');
  });

  await check('/sse: Last-Event-ID, until, and the connections of each key', async () => {
    const r = await stream(srv.url, '/sse?key=ids&n=2', { headers: { 'Last-Event-ID': '5' } });
    eq(r.body.toString('utf8'), ': conn 0\n' + tick(6, 0, '5') + tick(7, 0, '5'), 'from 5');
    // the UTF-8 of an id, one character a byte, as the image sends it
    const citta = await stream(srv.url, '/sse?key=ids&n=1', { headers: { 'Last-Event-ID': Buffer.from('citt\u00e0').toString('latin1') } });
    eq(citta.body.toString('utf8'), ': conn 1\n' + tick(1, 1, 'citt\u00e0'), 'a UTF-8 id echoed');
    const capped = await stream(srv.url, '/sse?key=ids&n=5&until=7', { headers: { 'Last-Event-ID': '5' } });
    eq(capped.body.toString('utf8'), ': conn 2\n' + tick(6, 2, '5') + tick(7, 2, '5'), 'no event after until');
    const over = await stream(srv.url, '/sse?key=ids&until=7', { headers: { 'Last-Event-ID': '7' } });
    eq([over.status, over.body.length, over.headers['cache-control']], [204, 0, 'no-store'], '204 at until');
    const other = await stream(srv.url, '/sse?key=other&n=1');
    eq(other.body.toString('utf8'), ': conn 0\n' + tick(1, 0, ''), 'another key counts from 0');
    eq((await request(`${srv.url}/_reset`, { method: 'POST', agent })).status, 204, '/_reset');
    const again = await stream(srv.url, '/sse?key=ids&n=1');
    eq(again.body.toString('utf8'), ': conn 0\n' + tick(1, 0, ''), '/_reset counts from 0 again');
  });

  await check('/sse: line ends, BOM, UTF-8, retry, and the ends partial and destroy', async () => {
    const r = await stream(srv.url, '/sse?key=eol&n=1&eol=crlf&bom=1&utf8=1&retry=1500');
    eq([...r.body.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'the BOM');
    eq(r.body.subarray(3).toString('utf8'), 'retry: 1500\r\n: conn 0\r\n' + tick(1, 0, '', '\r\n', true), 'CR LF, UTF-8');
    const cr = await stream(srv.url, '/sse?key=eol&n=1&eol=cr');
    eq(cr.body.toString('utf8'), ': conn 1\r' + tick(1, 1, '', '\r'), 'CR');
    const mixed = await stream(srv.url, '/sse?key=eol&n=2&eol=mixed');
    eq(mixed.body.toString('utf8'), [': conn 2\n', 'id: 1\r', 'event: tick\r\n', 'data: {"k":1,"conn":2,"last":""}\n', '\r',
      'id: 2\r\n', 'event: tick\n', 'data: {"k":2,"conn":2,"last":""}\r', '\r\n'].join(''), 'mixed: LF, CR, CR LF in turn');
    const partial = await stream(srv.url, '/sse?key=eol&n=1&end=partial');
    eq([partial.error, partial.body.toString('utf8')], [null, ': conn 3\n' + tick(1, 3, '') + 'id: 99\ndata: incomplete\n'], 'partial');
    const broken = await stream(srv.url, '/sse?key=eol&n=2&ms=100&end=destroy');
    assert(broken.error, 'destroy: the body breaks');
    eq(broken.body.toString('utf8'), ': conn 4\n' + tick(1, 4, '') + tick(2, 4, ''), 'destroy: after the events');
    const lastAt = broken.chunks[broken.chunks.length - 1].at;
    assert(broken.endAt - lastAt >= 80, `destroy: ${(broken.endAt - lastAt).toFixed(0)} ms after the last event, not ms`);
  });

  await check('/sse: split cuts the events between CR and LF and inside the bytes of é', async () => {
    const r = await stream(srv.url, '/sse?key=split&n=3&ms=60&split=1&eol=crlf');
    eq(r.body.toString('utf8'), ': conn 0\r\n' + [1, 2, 3].map(k => tick(k, 0, '', '\r\n', true)).join(''), 'the same bytes');
    assert(r.chunks.length >= 6, `${r.chunks.length} chunks, at least 2n`);
    const ends = r.chunks.map(c => c.bytes[c.bytes.length - 1]);
    assert(ends.includes(13), 'a chunk ends with CR');
    assert(ends.includes(0xc3), 'a chunk ends inside é');
    const lf = await stream(srv.url, '/sse?key=split&n=2&ms=60&split=1');
    eq(lf.body.toString('utf8'), ': conn 1\n' + tick(1, 1, '', '\n', true) + tick(2, 1, '', '\n', true), 'split implies utf8');
    assert(lf.chunks.filter(c => c.bytes[c.bytes.length - 1] === 0xc3).length === 2, 'with LF, each event cut inside é');
  });

  await check('/sse: cc, headDelay, size, status, type, and the timers of ping', async () => {
    const cached = await stream(srv.url, '/sse?key=cc&n=0&cc=max-age%3D60');
    eq([cached.headers['cache-control'], cached.body.toString('utf8')], ['max-age=60', ': conn 0\n'], 'cc');
    const late = await stream(srv.url, '/sse?key=cc&n=1&headDelay=200');
    assert(late.headAt >= 190, `the head after ${late.headAt.toFixed(0)} ms`);
    const big = await stream(srv.url, '/sse?key=cc&n=1&size=100000');
    eq(big.body.toString('utf8'), ': conn 2\nevent: big\ndata: ' + 'x'.repeat(100000) + '\n\n' + tick(1, 2, ''), 'size');
    const failed = await stream(srv.url, '/sse?key=cc&status=500');
    eq([failed.status, failed.headers['content-type'], failed.body.toString('utf8')],
      [500, 'application/json', '{"error":"status 500"}'], 'status');
    const plain = await stream(srv.url, '/sse?key=cc&n=1&type=text/plain');
    eq([plain.status, plain.headers['content-type'], plain.body.toString('utf8')], [200, 'text/plain', ': conn 4\n' + tick(1, 4, '')], 'type');
    const before = srv.timerCount;
    const pinged = await stream(srv.url, '/sse?key=cc&n=1&end=hang&ping=20',
      { until: a => Buffer.concat(a.chunks.map(c => c.bytes)).toString().split(': ping\n').length > 3 });
    const events = ': conn 5\n' + tick(1, 5, '');
    assert(pinged.body.toString('utf8').startsWith(events), 'the events first');
    eq(pinged.body.toString('utf8').slice(events.length, events.length + 21), ': ping\n: ping\n: ping\n', 'then pings');
    for (let i = 0; i < 100 && srv.timerCount !== before; i++) await sleep(10);
    eq(srv.timerCount, before, 'no timer left after the client went');
    await stream(srv.url, '/sse?key=cc&n=5&ms=1000', { until: () => true });
    for (let i = 0; i < 100 && srv.timerCount !== before; i++) await sleep(10);
    eq(srv.timerCount, before, 'none left of the events not sent');
  });
} finally {
  agent.destroy();
  await srv.close();
}

console.log(`${passes} passed, ${failures} failed`);
if (failures > 0) process.exit(1);
