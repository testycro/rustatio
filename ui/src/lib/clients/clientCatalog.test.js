import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  compareVersions,
  defaultClientDefinition,
  getClientCatalog,
  latestVersion,
  resolvePresetClient,
  setClientCatalog,
} from './clientCatalog.js';

const definitions = [
  { id: 'qbittorrent', default_version: '5.1.0', versions: ['5.1.0', '5.0.4'], is_default: true },
  { id: 'transmission', default_version: '4.0.5', versions: ['3.00', '4.0.5', '4.1.4', '4.0.6'] },
];

test('versions are compared number by number', () => {
  assert.ok(compareVersions('4.1.4', '4.0.6') > 0);
  assert.ok(compareVersions('3.00', '4.0.5') < 0);
  assert.ok(compareVersions('2.94', '2.93') > 0);
  assert.ok(compareVersions('5.1', '5.1.0') === 0);
  assert.ok(compareVersions('1.2.10', '1.2.9') > 0);
});

test('the latest version does not depend on the order of the list', () => {
  assert.equal(latestVersion(definitions[1]), '4.1.4');
  assert.equal(latestVersion({ default_version: '1.0', versions: [] }), '1.0');
  assert.equal(latestVersion(null), null);
});

test('the default client is the flagged one, else the first', () => {
  assert.equal(defaultClientDefinition(definitions).id, 'qbittorrent');
  assert.equal(defaultClientDefinition([definitions[1]]).id, 'transmission');
  assert.equal(defaultClientDefinition([]), null);
});

test('presets use the latest version of their client by default', () => {
  const resolved = resolvePresetClient(
    { selectedClient: 'transmission', selectedClientVersion: '4.0.5', uploadRate: 700 },
    definitions
  );
  assert.equal(resolved.selectedClient, 'transmission');
  assert.equal(resolved.selectedClientVersion, '4.1.4');
  assert.equal(resolved.uploadRate, 700);
});

test('a custom preset can keep its own version', () => {
  const settings = {
    selectedClient: 'transmission',
    selectedClientVersion: '4.0.5',
    useLatestClientVersion: false,
  };
  assert.equal(resolvePresetClient(settings, definitions).selectedClientVersion, '4.0.5');

  // a version that is no longer listed is replaced by the latest one
  const removed = { ...settings, selectedClientVersion: '1.0.0' };
  assert.equal(resolvePresetClient(removed, definitions).selectedClientVersion, '4.1.4');
});

test('a client that no longer exists is replaced by the default client', () => {
  for (const useLatestClientVersion of [true, false, undefined]) {
    const resolved = resolvePresetClient(
      { selectedClient: 'deluge', selectedClientVersion: '2.1.1', useLatestClientVersion },
      definitions
    );
    assert.equal(resolved.selectedClient, 'qbittorrent');
    assert.equal(resolved.selectedClientVersion, '5.1.0');
  }
});

test('nothing changes without client or without catalog', () => {
  const noClient = { uploadRate: 10 };
  assert.deepEqual(resolvePresetClient(noClient, definitions), noClient);
  const settings = { selectedClient: 'deluge', selectedClientVersion: '2.1.1' };
  assert.deepEqual(resolvePresetClient(settings, []), settings);
});

test('without catalog, a preset without version does not keep the previous version', () => {
  assert.deepEqual(resolvePresetClient({ selectedClient: 'transmission' }, []), {
    selectedClient: 'transmission',
    selectedClientVersion: null,
  });
});

test('the catalog keeps valid definitions only', () => {
  setClientCatalog([definitions[0], null, { name: 'no id' }]);
  assert.deepEqual(getClientCatalog(), [definitions[0]]);
  setClientCatalog(undefined);
  assert.deepEqual(getClientCatalog(), []);
});
