import { resolvePresetClient } from '../clients/clientCatalog.js';

// Also resolves the client of the preset against the saved client definitions (latest version,
// default client when it no longer exists), see resolvePresetClient.
export function normalizePresetSettings(settings = {}) {
  return resolvePresetClient({
    ...settings,
    ...(settings.idleWhenNoLeechers == null && settings.stopWhenNoLeechers != null
      ? { idleWhenNoLeechers: settings.stopWhenNoLeechers }
      : {}),
    ...(settings.idleWhenNoSeeders == null && settings.stopWhenNoSeeders != null
      ? { idleWhenNoSeeders: settings.stopWhenNoSeeders }
      : {}),
  });
}

// Instance settings a preset may change. Anything else found in a preset (an imported file can
// hold any key: id, torrent, isRunning, stats...) must never reach the instance.
export const PRESET_INSTANCE_KEYS = Object.freeze([
  'selectedClient',
  'selectedClientVersion',
  'uploadRate',
  'downloadRate',
  'port',
  'vpnPortSync',
  'completionPercent',
  'randomizeRates',
  'randomRangePercent',
  'updateIntervalSeconds',
  'scrapeInterval',
  'progressiveRatesEnabled',
  'targetUploadRate',
  'targetDownloadRate',
  'progressiveDurationHours',
  'stopAtRatioEnabled',
  'stopAtRatio',
  'randomizeRatio',
  'randomRatioRangePercent',
  'stopAtUploadedEnabled',
  'stopAtUploadedGB',
  'stopAtDownloadedEnabled',
  'stopAtDownloadedGB',
  'stopAtSeedTimeEnabled',
  'stopAtSeedTimeHours',
  'idleWhenNoLeechers',
  'idleWhenNoSeeders',
  'postStopAction',
  'trackerErrorAction',
]);

// Keys kept in a saved preset: the instance settings, the preset-only options and the legacy
// names still read by normalizePresetSettings.
const PRESET_SETTING_KEYS = new Set([
  ...PRESET_INSTANCE_KEYS,
  'useLatestClientVersion',
  'stopWhenNoLeechers',
  'stopWhenNoSeeders',
]);

function pickKeys(settings, keys) {
  const picked = {};
  for (const [key, value] of Object.entries(settings || {})) {
    if (keys.has(key) && value !== undefined) picked[key] = value;
  }
  return picked;
}

// Settings of a preset coming from outside (imported file): unknown keys are dropped.
export function sanitizePresetSettings(settings = {}) {
  return pickKeys(settings, PRESET_SETTING_KEYS);
}

const INSTANCE_KEY_SET = new Set(PRESET_INSTANCE_KEYS);

// Settings to put on an instance: resolved like above, restricted to the instance settings (no
// preset-only options, nothing that could overwrite the state of the instance).
export function presetInstanceSettings(settings = {}) {
  return pickKeys(normalizePresetSettings(settings), INSTANCE_KEY_SET);
}

export function normalizePreset(preset) {
  if (!preset?.settings) {
    return preset;
  }

  return {
    ...preset,
    settings: normalizePresetSettings(preset.settings),
  };
}

export function normalizePresets(presets = []) {
  return presets.map(normalizePreset);
}

export function buildCustomPreset(
  instance,
  { name, description = '', id, now = new Date(), useLatestClientVersion = true } = {}
) {
  if (!instance) {
    throw new Error('Instance not found.');
  }

  const trimmedName = name?.trim();
  if (!trimmedName) {
    throw new Error('Please enter a preset name.');
  }

  const createdAt =
    typeof now?.toISOString === 'function' ? now.toISOString() : new Date().toISOString();

  return {
    id: id || `custom-${Date.now()}`,
    name: trimmedName,
    description:
      description.trim() || `Custom preset created on ${new Date(createdAt).toLocaleDateString()}`,
    icon: 'star',
    custom: true,
    created_at: createdAt,
    settings: {
      selectedClient: instance.selectedClient,
      selectedClientVersion: instance.selectedClientVersion,
      // Follow the client updates instead of keeping this exact version
      useLatestClientVersion,
      uploadRate: instance.uploadRate,
      downloadRate: instance.downloadRate,
      port: instance.port,
      vpnPortSync: instance.vpnPortSync ?? false,
      completionPercent: instance.completionPercent,
      randomizeRates: instance.randomizeRates,
      randomRangePercent: instance.randomRangePercent,
      updateIntervalSeconds: instance.updateIntervalSeconds,
      scrapeInterval: instance.scrapeInterval,
      progressiveRatesEnabled: instance.progressiveRatesEnabled,
      targetUploadRate: instance.targetUploadRate,
      targetDownloadRate: instance.targetDownloadRate,
      progressiveDurationHours: instance.progressiveDurationHours,
      stopAtRatioEnabled: instance.stopAtRatioEnabled,
      stopAtRatio: instance.stopAtRatio,
      randomizeRatio: instance.randomizeRatio,
      randomRatioRangePercent: instance.randomRatioRangePercent,
      stopAtUploadedEnabled: instance.stopAtUploadedEnabled,
      stopAtUploadedGB: instance.stopAtUploadedGB,
      stopAtDownloadedEnabled: instance.stopAtDownloadedEnabled,
      stopAtDownloadedGB: instance.stopAtDownloadedGB,
      stopAtSeedTimeEnabled: instance.stopAtSeedTimeEnabled,
      stopAtSeedTimeHours: instance.stopAtSeedTimeHours,
      idleWhenNoLeechers: instance.idleWhenNoLeechers,
      idleWhenNoSeeders: instance.idleWhenNoSeeders,
      postStopAction: instance.postStopAction,
      trackerErrorAction: instance.trackerErrorAction,
    },
  };
}

export function buildPresetExportData(instance, options) {
  const preset = buildCustomPreset(instance, options);

  return {
    version: 1,
    type: 'rustatio-preset',
    name: preset.name,
    description: preset.description,
    icon: preset.icon,
    createdAt: preset.created_at,
    settings: preset.settings,
  };
}
