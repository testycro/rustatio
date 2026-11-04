import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  definitionFromForm,
  emptyClientForm,
  formFromDefinition,
  parseVersionList,
  peerIdPrefix,
  slugifyClientId,
  userAgentPreview,
  validateClientForm,
} from './clientDefinitions.js';

const sampleDefinition = {
  id: 'qbittorrent',
  name: 'qBittorrent',
  default_version: '5.3.0',
  versions: ['5.3.0', '5.2.4'],
  default_port: 6881,
  is_default: true,
  peer_id_code: 'qB',
  user_agent: 'qBittorrent/{version}',
  version_scheme: 'major_minor_patch',
  fallback_version_code: '514',
  num_want: 200,
  supports_compact: true,
  supports_crypto: true,
  http_version: 'http11',
};

function validForm(overrides = {}) {
  return { ...formFromDefinition(sampleDefinition), ...overrides };
}

test('definition survives a form round trip', () => {
  assert.deepEqual(definitionFromForm(formFromDefinition(sampleDefinition)), sampleDefinition);
});

test('missing optional fields get defaults and an empty fallback becomes null', () => {
  const form = formFromDefinition({ id: 'x', name: 'X' });
  assert.equal(form.defaultPort, '6881');
  assert.equal(form.numWant, '200');
  assert.equal(form.supportsCompact, true);
  assert.equal(definitionFromForm(form).fallback_version_code, null);
});

test('default version falls back to the first listed version', () => {
  const definition = definitionFromForm(validForm({ defaultVersion: '' }));
  assert.equal(definition.default_version, '5.3.0');
});

test('parseVersionList accepts lines, commas and spaces and drops duplicates', () => {
  assert.deepEqual(parseVersionList('5.3.0\n5.2.4, 5.2.4  5.1.0 \n\n'), [
    '5.3.0',
    '5.2.4',
    '5.1.0',
  ]);
  assert.deepEqual(parseVersionList(''), []);
  assert.deepEqual(parseVersionList(null), []);
});

test('slugifyClientId builds a valid id from a display name', () => {
  assert.equal(slugifyClientId('My Client 2.0!'), 'my-client-2-0');
  assert.equal(slugifyClientId('  µTorrent '), 'torrent');
  assert.equal(slugifyClientId(''), '');
});

test('validateClientForm accepts a complete form', () => {
  assert.equal(validateClientForm(validForm(), { existingIds: [], isNew: true }), '');
});

test('validateClientForm checks the id', () => {
  assert.match(validateClientForm(validForm({ id: '' })), /ID is required/);
  assert.match(validateClientForm(validForm({ id: 'Bad Id' })), /lowercase/);
  assert.match(
    validateClientForm(validForm(), { existingIds: ['qbittorrent'], isNew: true }),
    /already exists/
  );
  assert.equal(validateClientForm(validForm(), { existingIds: ['qbittorrent'], isNew: false }), '');
});

test('validateClientForm checks the other required fields', () => {
  assert.match(validateClientForm(validForm({ name: ' ' })), /Name/);
  assert.match(validateClientForm(validForm({ versions: '' })), /at least one version/);
  assert.match(validateClientForm(validForm({ defaultVersion: '9.9.9' })), /Default version/);
  assert.match(validateClientForm(validForm({ peerIdCode: 'qBt' })), /exactly 2/);
  assert.match(validateClientForm(validForm({ userAgent: '' })), /User-Agent/);
  assert.match(validateClientForm(validForm({ defaultPort: '0' })), /Port/);
  assert.match(validateClientForm(validForm({ defaultPort: 'abc' })), /Port/);
  assert.match(validateClientForm(validForm({ numWant: '5000' })), /Peers requested/);
});

test('peerIdPrefix mirrors the backend version schemes', () => {
  assert.equal(peerIdPrefix(validForm(), '5.2.0'), '-qB5200-');
  assert.equal(peerIdPrefix(validForm(), '5'), '-qB5140-');

  const transmission = validForm({
    peerIdCode: 'TR',
    versionScheme: 'major_minor_pad2',
    fallbackVersionCode: '400',
  });
  assert.equal(peerIdPrefix(transmission, '4.1.3'), '-TR4100-');
  assert.equal(peerIdPrefix(transmission, '4'), '-TR4000-');

  const utorrent = validForm({ peerIdCode: 'UT', versionScheme: 'digits' });
  assert.equal(peerIdPrefix(utorrent, '3.5.5'), '-UT3550-');

  const rtorrent = validForm({ peerIdCode: 'RT', fallbackVersionCode: '' });
  assert.equal(peerIdPrefix(rtorrent, '0.16.12'), '-RT0161-');
});

test('userAgentPreview substitutes the version', () => {
  assert.equal(userAgentPreview(validForm(), '5.2.0'), 'qBittorrent/5.2.0');
});

test('empty form is not valid until filled in', () => {
  assert.notEqual(validateClientForm(emptyClientForm()), '');
});
