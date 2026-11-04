// Client definition helpers shared by the settings screen.
// Pure functions only so they can be unit tested with node:test.

export const VERSION_SCHEMES = [
  { id: 'major_minor_patch', label: 'Major.minor.patch (5.2.0 → 520)' },
  { id: 'major_minor_pad2', label: 'Major + minor on 2 digits (4.1.3 → 410)' },
  { id: 'digits', label: 'All digits (3.5.5 → 355)' },
];

export const HTTP_VERSIONS = [
  { id: 'http11', label: 'HTTP/1.1' },
  { id: 'http10', label: 'HTTP/1.0' },
];

const ID_PATTERN = /^[a-z0-9_-]+$/;
const PEER_ID_CODE_PATTERN = /^[A-Za-z0-9]{2}$/;

export function emptyClientForm() {
  return {
    id: '',
    name: '',
    defaultVersion: '',
    versions: '',
    defaultPort: '6881',
    isDefault: false,
    peerIdCode: '',
    userAgent: '',
    versionScheme: 'major_minor_patch',
    fallbackVersionCode: '',
    numWant: '200',
    supportsCompact: true,
    supportsCrypto: true,
    httpVersion: 'http11',
  };
}

export function slugifyClientId(name) {
  return String(name ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

// One version per line, or separated by commas / spaces. Duplicates are dropped.
export function parseVersionList(text) {
  const seen = new Set();
  const versions = [];
  for (const part of String(text ?? '').split(/[\s,;]+/)) {
    const version = part.trim();
    if (version && !seen.has(version)) {
      seen.add(version);
      versions.push(version);
    }
  }
  return versions;
}

export function formFromDefinition(definition = {}) {
  const empty = emptyClientForm();
  return {
    id: definition.id ?? empty.id,
    name: definition.name ?? empty.name,
    defaultVersion: definition.default_version ?? empty.defaultVersion,
    versions: (definition.versions ?? []).join('\n'),
    defaultPort: String(definition.default_port ?? empty.defaultPort),
    isDefault: Boolean(definition.is_default),
    peerIdCode: definition.peer_id_code ?? empty.peerIdCode,
    userAgent: definition.user_agent ?? empty.userAgent,
    versionScheme: definition.version_scheme ?? empty.versionScheme,
    fallbackVersionCode: definition.fallback_version_code ?? empty.fallbackVersionCode,
    numWant: String(definition.num_want ?? empty.numWant),
    supportsCompact: definition.supports_compact ?? empty.supportsCompact,
    supportsCrypto: definition.supports_crypto ?? empty.supportsCrypto,
    httpVersion: definition.http_version ?? empty.httpVersion,
  };
}

export function definitionFromForm(form) {
  const versions = parseVersionList(form.versions);
  const fallback = String(form.fallbackVersionCode ?? '').trim();

  return {
    id: String(form.id).trim(),
    name: String(form.name).trim(),
    default_version: String(form.defaultVersion).trim() || versions[0] || '',
    versions,
    default_port: Number.parseInt(form.defaultPort, 10),
    is_default: Boolean(form.isDefault),
    peer_id_code: String(form.peerIdCode).trim(),
    user_agent: String(form.userAgent).trim(),
    version_scheme: form.versionScheme,
    fallback_version_code: fallback || null,
    num_want: Number.parseInt(form.numWant, 10),
    supports_compact: Boolean(form.supportsCompact),
    supports_crypto: Boolean(form.supportsCrypto),
    http_version: form.httpVersion,
  };
}

function isIntegerInRange(value, min, max) {
  const text = String(value ?? '').trim();
  if (!/^\d+$/.test(text)) return false;
  const number = Number.parseInt(text, 10);
  return number >= min && number <= max;
}

// Returns an error message, or '' when the form can be saved.
export function validateClientForm(form, { existingIds = [], isNew = true } = {}) {
  const id = String(form.id ?? '').trim();
  if (!id) return 'Client ID is required';
  if (!ID_PATTERN.test(id)) {
    return 'Client ID can only contain lowercase letters, digits, "-" and "_"';
  }
  if (isNew && existingIds.includes(id)) return `A client with ID "${id}" already exists`;

  if (!String(form.name ?? '').trim()) return 'Name is required';

  const versions = parseVersionList(form.versions);
  if (versions.length === 0) return 'Add at least one version';

  const defaultVersion = String(form.defaultVersion ?? '').trim();
  if (defaultVersion && !versions.includes(defaultVersion)) {
    return 'Default version must be one of the listed versions';
  }

  if (!PEER_ID_CODE_PATTERN.test(String(form.peerIdCode ?? '').trim())) {
    return 'Peer ID code must be exactly 2 letters or digits (e.g. "qB")';
  }
  if (!String(form.userAgent ?? '').trim()) return 'User-Agent is required';
  if (!isIntegerInRange(form.defaultPort, 1, 65535)) return 'Port must be between 1 and 65535';
  if (!isIntegerInRange(form.numWant, 0, 1000)) return 'Peers requested must be between 0 and 1000';

  return '';
}

function padVersionCode(code) {
  return code.length >= 4 ? code.slice(0, 4) : code.padEnd(4, '0');
}

// Mirrors the backend so the form can show what the peer ID will start with.
export function peerIdPrefix(form, version) {
  const parts = String(version ?? '').split('.');
  const fallback = String(form.fallbackVersionCode ?? '').trim() || '0000';

  let code;
  switch (form.versionScheme) {
    case 'digits':
      code = String(version ?? '').replaceAll('.', '');
      break;
    case 'major_minor_pad2':
      code = parts.length >= 2 ? `${parts[0]}${parts[1].padEnd(2, '0').slice(0, 2)}` : fallback;
      break;
    default:
      code = parts.length >= 3 ? `${parts[0]}${parts[1]}${parts[2]}` : fallback;
  }

  return `-${String(form.peerIdCode ?? '').trim()}${padVersionCode(code)}-`;
}

export function userAgentPreview(form, version) {
  return String(form.userAgent ?? '').replaceAll('{version}', String(version ?? ''));
}
