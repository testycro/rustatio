import assert from 'node:assert/strict';
import test from 'node:test';
import { parseLogLines, toneOf } from './logLines.js';

test('tone follows the emoji prefix, including nested "└─" lines', () => {
  assert.equal(toneOf('❌ boom'), 'error');
  assert.equal(toneOf('⚠️ careful'), 'warn');
  assert.equal(toneOf('🚫 denied'), 'warn');
  assert.equal(toneOf('✅️ done'), 'success');
  assert.equal(toneOf('🔍 Handling log event payload: {}'), 'trace');
  assert.equal(toneOf('🚀 started'), 'accent');
  assert.equal(toneOf('   └─ ❌ nested'), 'error');
  assert.equal(toneOf('💾 saving'), 'info');
});

test('parses timestamp and message, keeps tracebacks in the error tone', () => {
  const lines = parseLogLines(
    [
      '02-11-2026 10:47:22 :: ❌ failed',
      'Traceback (most recent call last):',
      '02-11-2026 10:47:23 :: 🔍 trace',
      'plain continuation',
      '',
    ].join('\n')
  );
  assert.equal(lines.length, 4);
  assert.deepEqual(
    lines.map(l => l.tone),
    ['error', 'error', 'trace', 'info']
  );
  assert.equal(lines[0].time, '02-11-2026 10:47:22');
  assert.equal(lines[0].body, '❌ failed');
  assert.equal(lines[1].time, '');
});

test('empty content gives no lines', () => {
  assert.deepEqual(parseLogLines(''), []);
  assert.deepEqual(parseLogLines(undefined), []);
});
