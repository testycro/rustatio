import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DEMO_PID, DEMO_RULES, createDemoDaemon, formatLogTime } from './demoBackend.js';
import { parseLogLines } from './logLines.js';

function makeDaemon() {
  const clock = { time: new Date(2026, 10, 2, 12, 0, 0).getTime() };
  const daemon = createDemoDaemon({ now: () => clock.time });
  return { daemon, clock, call: (path, options) => daemon.request(path, options) };
}

test('the demo daemon starts running', async () => {
  const { call } = makeDaemon();
  const { ok, data } = await call('/status');
  assert.equal(ok, true);
  assert.deepEqual(data, { running: true, pid: DEMO_PID });
});

test('stop / start / restart follow the real endpoints', async () => {
  const { call } = makeDaemon();

  assert.deepEqual((await call('/daemon/stop', { method: 'POST' })).data, { success: true });
  assert.deepEqual((await call('/status')).data, { running: false });
  assert.equal((await call('/watcher/status')).data.running, false);

  assert.deepEqual((await call('/daemon/start', { method: 'POST' })).data, { success: true });
  assert.equal((await call('/status')).data.running, true);
  assert.equal((await call('/daemon/restart', { method: 'POST' })).data.success, true);

  const unknown = await call('/daemon/explode', { method: 'POST' });
  assert.equal(unknown.data.success, false);
});

test('rules can be read, saved and read back', async () => {
  const { call } = makeDaemon();
  assert.equal((await call('/rules')).data.content, DEMO_RULES);

  await call('/rules', { method: 'POST', body: { content: 'stats.ratio > 2 | stop |' } });
  assert.equal((await call('/rules')).data.content, 'stats.ratio > 2 | stop |');
});

test('the default rules are valid rule lines', () => {
  const lines = DEMO_RULES.split('\n').filter(line => line.trim());
  assert.ok(lines.length > 10);
  for (const line of lines) assert.equal(line.split('|').length, 3, line);
});

test('logs: parsable, grow while running, can be limited, cleared and archived', async () => {
  const { call, clock } = makeDaemon();

  const initial = (await call('/logs?lines=1000')).data.content;
  const parsedInitial = parseLogLines(initial);
  assert.ok(parsedInitial.length > 5);
  assert.ok(parsedInitial.every(line => line.time !== ''));

  clock.time += 5 * 60 * 1000;
  const later = parseLogLines((await call('/logs?lines=1000')).data.content);
  assert.ok(later.length > parsedInitial.length);

  const last3 = parseLogLines((await call('/logs?lines=3')).data.content);
  assert.equal(last3.length, 3);

  const archive = await call('/logs/archive', { method: 'POST' });
  assert.equal(archive.data.success, true);
  assert.match(archive.data.filename, /^rustatio_daemon_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}\.log$/);

  await call('/logs/clear', { method: 'POST' });
  assert.equal((await call('/logs')).data.content, '');
});

test('logs do not grow while the daemon is stopped', async () => {
  const { call, clock } = makeDaemon();
  await call('/daemon/stop', { method: 'POST' });
  const before = (await call('/logs?lines=1000')).data.content;

  clock.time += 10 * 60 * 1000;
  const after = (await call('/logs?lines=1000')).data.content;
  assert.equal(after, before);
});

test('watcher: entries, actions that need the daemon, and clear & resume', async () => {
  const { call } = makeDaemon();

  const status = (await call('/watcher/status')).data;
  assert.deepEqual(status, { running: true, status: 'ok', pid: DEMO_PID });

  const entries = Object.entries((await call('/watcher/state')).data.state);
  assert.ok(entries.length >= 2);
  const [tag, info] = entries[0];
  assert.equal(typeof info.total_strikes, 'number');
  assert.ok(['paused', 'announce', 'observing'].includes(info.status));

  assert.equal((await call('/watcher/stop', { method: 'POST' })).data.success, true);
  assert.equal((await call('/watcher/status')).data.running, false);
  assert.equal((await call('/watcher/start', { method: 'POST' })).data.success, true);
  assert.equal((await call('/watcher/nope', { method: 'POST' })).data.success, false);

  const missing = await call('/watcher/clear_resume', { method: 'POST', body: {} });
  assert.equal(missing.status, 400);

  await call('/watcher/clear_resume', { method: 'POST', body: { tag } });
  assert.equal(tag in (await call('/watcher/state')).data.state, false);

  await call('/daemon/stop', { method: 'POST' });
  const refused = await call('/watcher/start', { method: 'POST' });
  assert.equal(refused.data.success, false);
});

test('watcher entries expire and the countdown goes down', async () => {
  const { call, clock } = makeDaemon();
  const first = (await call('/watcher/state')).data.state;
  const paused = Object.keys(first).find(tag => first[tag].status === 'paused');

  clock.time += 60 * 1000;
  const second = (await call('/watcher/state')).data.state;
  assert.ok(second[paused].time_left < first[paused].time_left);

  clock.time += 3 * 3600 * 1000;
  const third = (await call('/watcher/state')).data.state;
  assert.equal(paused in third, false);
});

test('env: read, update, refuse read-only keys, reset', async () => {
  const { call } = makeDaemon();

  const env = (await call('/env')).data;
  assert.equal(env.config.WATCHER_MAX_STRIKE, 3);
  assert.ok(env.readonly.includes('PORT'));

  const saved = await call('/env', {
    method: 'POST',
    body: { WATCHER_MAX_STRIKE: '5', DRY_RUN: 'true', RUST_DAEMON_LOG: 'debug' },
  });
  assert.equal(saved.data.status, 'success');
  const updated = (await call('/env')).data.config;
  assert.equal(updated.WATCHER_MAX_STRIKE, 5);
  assert.equal(updated.DRY_RUN, true);
  assert.equal(updated.RUST_DAEMON_LOG, 'DEBUG');

  const readonly = await call('/env', { method: 'POST', body: { PORT: 1234 } });
  assert.equal(readonly.status, 403);
  assert.equal(readonly.ok, false);
  assert.equal(readonly.data.status, 'forbidden');

  const badLevel = await call('/env', { method: 'POST', body: { RUST_DAEMON_LOG: 'LOUD' } });
  assert.equal(badLevel.status, 403);

  await call('/env/reset', { method: 'POST' });
  assert.equal((await call('/env')).data.config.WATCHER_MAX_STRIKE, 3);
});

test('unknown routes answer 404 and restart is acknowledged', async () => {
  const { call } = makeDaemon();
  assert.equal((await call('/nope')).status, 404);
  assert.equal((await call('/admin/restart', { method: 'POST' })).data.success, true);
});

test('log timestamps use the daemon format', () => {
  assert.equal(formatLogTime(new Date(2026, 0, 5, 9, 3, 7)), '05-01-2026 09:03:07');
});
