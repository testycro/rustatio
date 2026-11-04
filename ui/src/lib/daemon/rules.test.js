import assert from 'node:assert/strict';
import test from 'node:test';
import {
  parseRules,
  serializeRules,
  createRule,
  withCurrent,
  DEFAULT_CONFIG_SUGGESTIONS,
} from './rules.js';
import { formatTimeLeft } from './format.js';

test('parses conditions joined by AND / OR with their operators', () => {
  const [rule] = parseRules('tags = end AND stats.state != "Stopped" | stop |');
  assert.equal(rule.action, 'stop');
  assert.deepEqual(
    rule.conditions.map(c => [c.logOp, c.field, c.op, c.val]),
    [
      ['AND', 'tags', '=', 'end'],
      ['AND', 'stats.state', '!=', '"Stopped"'],
    ]
  );

  const [or] = parseRules('stats.ratio >= 2 or tags: a | pause |');
  assert.equal(or.conditions[1].logOp, 'OR');
});

test('round-trips a rules file without comments', () => {
  const text = [
    'tags = end AND stats.state != "Stopped" | stop |',
    'stats.ratio >= 1.5 | update | config.upload_rate = default_config.upload_rate',
    'tags = old | addtags | done, archived',
  ].join('\n');
  assert.equal(serializeRules(parseRules(text)), text);
});

test('the colon operator is written as "field: value"', () => {
  assert.equal(
    serializeRules(parseRules('stats.ratio: 1.8 - 2.4 | start |')),
    'stats.ratio: 1.8 - 2.4 | start |'
  );
});

test('update keeps "=" inside the value and unknown settings', () => {
  const [rule] = parseRules('id: x | update | config.my_custom = a=b');
  assert.equal(rule.conf, 'config.my_custom');
  assert.equal(rule.value, 'a=b');
  assert.equal(serializeRules([rule]), 'id: x | update | config.my_custom = a=b');
});

test('comments and blank lines are skipped, empty input gives one blank rule', () => {
  assert.equal(parseRules('# comment\n\n').length, 1);
  assert.equal(parseRules('# comment\n\n')[0].action, 'start');
  assert.equal(parseRules('# c\nid: 1 | start |\n').length, 1);
});

test('simple actions drop their assignment', () => {
  assert.equal(serializeRules(parseRules('id: 1 | start | ignored')), 'id: 1 | start |');
});

test('serialize skips nothing for a fresh rule and unknown fields stay selectable', () => {
  assert.equal(serializeRules([createRule()]), 'id:  | start |');
  assert.deepEqual(withCurrent(['a', 'b'], 'c'), ['a', 'b', 'c']);
  assert.deepEqual(withCurrent(['a', 'b'], 'a'), ['a', 'b']);
  assert.ok(DEFAULT_CONFIG_SUGGESTIONS.includes('default_config.upload_rate'));
});

test('formatTimeLeft', () => {
  assert.equal(formatTimeLeft(0, 'exp'), 'exp');
  assert.equal(formatTimeLeft(125), '2m 5s');
  assert.equal(formatTimeLeft(3725), '1h 2m 5s');
});
