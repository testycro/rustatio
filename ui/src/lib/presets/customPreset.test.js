import assert from 'node:assert/strict';
import test from 'node:test';

import {
  buildCustomPreset,
  buildPresetExportData,
  normalizePreset,
  normalizePresetSettings,
  presetInstanceSettings,
  sanitizePresetSettings,
} from './customPreset.js';
import { setClientCatalog } from '../clients/clientCatalog.js';

function makeInstance() {
  return {
    selectedClient: 'qbittorrent',
    selectedClientVersion: '5.2.0',
    uploadRate: 50,
    downloadRate: 100,
    port: 6881,
    vpnPortSync: true,
    completionPercent: 100,
    randomizeRates: true,
    randomRangePercent: 20,
    updateIntervalSeconds: 5,
    scrapeInterval: 60,
    progressiveRatesEnabled: true,
    targetUploadRate: 80,
    targetDownloadRate: 120,
    progressiveDurationHours: 3,
    stopAtRatioEnabled: true,
    stopAtRatio: 2.5,
    randomizeRatio: false,
    randomRatioRangePercent: 10,
    stopAtUploadedEnabled: false,
    stopAtUploadedGB: 10,
    stopAtDownloadedEnabled: false,
    stopAtDownloadedGB: 10,
    stopAtSeedTimeEnabled: true,
    stopAtSeedTimeHours: 24,
    idleWhenNoLeechers: true,
    idleWhenNoSeeders: false,
    postStopAction: 'pause',
    trackerErrorAction: 'stop',
  };
}

test('buildCustomPreset creates a storable custom preset', () => {
  const now = new Date('2026-03-27T12:00:00.000Z');
  const preset = buildCustomPreset(makeInstance(), {
    id: 'custom-fixed',
    name: '  My preset  ',
    description: '  Great preset  ',
    now,
  });

  assert.equal(preset.id, 'custom-fixed');
  assert.equal(preset.name, 'My preset');
  assert.equal(preset.description, 'Great preset');
  assert.equal(preset.custom, true);
  assert.equal(preset.created_at, '2026-03-27T12:00:00.000Z');
  assert.equal(preset.settings.selectedClient, 'qbittorrent');
  assert.equal(preset.settings.postStopAction, 'pause');
  assert.equal(preset.settings.trackerErrorAction, 'stop');
  assert.equal(presetInstanceSettings(preset.settings).trackerErrorAction, 'stop');
});

test('buildPresetExportData wraps preset data for file export', () => {
  const now = new Date('2026-03-27T12:00:00.000Z');
  const data = buildPresetExportData(makeInstance(), { name: 'Exported', now });

  assert.equal(data.type, 'rustatio-preset');
  assert.equal(data.name, 'Exported');
  assert.equal(data.createdAt, '2026-03-27T12:00:00.000Z');
  assert.equal(data.settings.uploadRate, 50);
});

test('buildCustomPreset rejects missing preset names', () => {
  assert.throws(
    () => buildCustomPreset(makeInstance(), { name: '   ' }),
    /Please enter a preset name/
  );
});

test('normalizePresetSettings maps legacy stopWhenNo aliases', () => {
  const settings = normalizePresetSettings({
    stopWhenNoLeechers: true,
    stopWhenNoSeeders: false,
  });

  assert.equal(settings.idleWhenNoLeechers, true);
  assert.equal(settings.idleWhenNoSeeders, false);
});

test('normalizePresetSettings preserves canonical idleWhen keys', () => {
  const settings = normalizePresetSettings({
    stopWhenNoLeechers: false,
    idleWhenNoLeechers: true,
  });

  assert.equal(settings.idleWhenNoLeechers, true);
});

test('normalizePreset normalizes nested preset settings', () => {
  const preset = normalizePreset({
    id: 'legacy',
    settings: {
      stopWhenNoLeechers: true,
    },
  });

  assert.equal(preset.settings.idleWhenNoLeechers, true);
});

test('custom presets follow the latest client version unless asked not to', () => {
  assert.equal(
    buildCustomPreset(makeInstance(), { name: 'Latest' }).settings.useLatestClientVersion,
    true
  );
  assert.equal(
    buildCustomPreset(makeInstance(), { name: 'Pinned', useLatestClientVersion: false }).settings
      .useLatestClientVersion,
    false
  );
});

test('preset settings are resolved against the saved clients', () => {
  setClientCatalog([
    { id: 'qbittorrent', versions: ['5.1.0', '5.2.0', '5.3.0'], is_default: true },
    { id: 'transmission', versions: ['4.1.4', '4.0.5'] },
  ]);
  try {
    const latest = normalizePresetSettings({ selectedClient: 'qbittorrent' });
    assert.equal(latest.selectedClientVersion, '5.3.0');

    const pinned = normalizePresetSettings({
      selectedClient: 'qbittorrent',
      selectedClientVersion: '5.2.0',
      useLatestClientVersion: false,
    });
    assert.equal(pinned.selectedClientVersion, '5.2.0');

    const removed = normalizePresetSettings({ selectedClient: 'deluge' });
    assert.equal(removed.selectedClient, 'qbittorrent');
    assert.equal(removed.selectedClientVersion, '5.3.0');
  } finally {
    setClientCatalog([]);
  }
});

test('preset settings applied to an instance only contain instance settings', () => {
  const settings = presetInstanceSettings({
    uploadRate: 42,
    scrapeInterval: 90,
    useLatestClientVersion: true,
    stopWhenNoLeechers: true,
    id: 'other-instance',
    torrent: { name: 'injected' },
    isRunning: true,
    stats: {},
  });

  assert.deepEqual(settings, {
    uploadRate: 42,
    scrapeInterval: 90,
    idleWhenNoLeechers: true,
  });
});

test('imported preset settings drop unknown keys', () => {
  // Parsed like an imported file: "__proto__" is then a real key of the object
  const settings = sanitizePresetSettings(
    JSON.parse(
      '{"selectedClient":"qbittorrent","useLatestClientVersion":false,"stopWhenNoSeeders":true,' +
        '"torrentPath":"/etc/passwd","__proto__":{"polluted":true}}'
    )
  );

  assert.deepEqual(settings, {
    selectedClient: 'qbittorrent',
    useLatestClientVersion: false,
    stopWhenNoSeeders: true,
  });
  assert.equal(settings.polluted, undefined);
});
