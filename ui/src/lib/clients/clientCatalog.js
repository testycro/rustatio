// Client definitions currently saved (same list in every mode), kept here so that presets can be
// resolved synchronously when they are applied. Filled by the app at startup and by the clients
// manager after each change; while it is empty, presets are applied as written.

let catalog = [];

export function setClientCatalog(definitions) {
  catalog = Array.isArray(definitions) ? definitions.filter(definition => definition?.id) : [];
}

export function getClientCatalog() {
  return catalog;
}

function versionParts(version) {
  return (String(version ?? '').match(/\d+/g) || []).map(Number);
}

// Negative when `left` is older than `right`, positive when newer, 0 when equal.
export function compareVersions(left, right) {
  const a = versionParts(left);
  const b = versionParts(right);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

// Newest version of a client definition, whatever the order of its list.
export function latestVersion(definition) {
  const versions = definition?.versions || [];
  if (versions.length === 0) return definition?.default_version || null;
  return versions.reduce((latest, version) =>
    compareVersions(version, latest) > 0 ? version : latest
  );
}

// The client flagged as default, else the first one.
export function defaultClientDefinition(definitions = catalog) {
  return definitions.find(definition => definition.is_default) || definitions[0] || null;
}

// Client and version a preset really uses:
// - a client that no longer exists is replaced by the default client, at its latest version;
// - with `useLatestClientVersion` (always for built-in presets, the default for custom ones), the
//   latest version of the client is used instead of the one saved in the preset.
export function resolvePresetClient(settings = {}, definitions = catalog) {
  if (!settings.selectedClient) return settings;
  if (definitions.length === 0) {
    // Catalog not loaded (yet): a preset without version must not keep the version of the
    // client previously selected on the instance (e.g. a qBittorrent version announced as
    // Transmission). null lets the default version of the client be used.
    return settings.selectedClientVersion === undefined
      ? { ...settings, selectedClientVersion: null }
      : settings;
  }

  const saved = definitions.find(definition => definition.id === settings.selectedClient);
  const definition = saved || defaultClientDefinition(definitions);
  if (!definition) return settings;

  const keepVersion =
    saved &&
    settings.useLatestClientVersion === false &&
    settings.selectedClientVersion &&
    (saved.versions || []).includes(settings.selectedClientVersion);

  return {
    ...settings,
    selectedClient: definition.id,
    selectedClientVersion: keepVersion ? settings.selectedClientVersion : latestVersion(definition),
  };
}
