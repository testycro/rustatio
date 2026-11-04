// Persistence of the emulated client definitions for the browser-only (WASM) build.
// Server and desktop keep them in their state file; without a backend they live in the
// browser's localStorage. Pure functions taking the storage object so they can be unit
// tested with node:test.

export const CLIENT_DEFINITIONS_KEY = 'rustatio-client-definitions';

// localStorage can throw (blocked site data, sandboxed frames): treat it as absent.
export function browserStorage() {
  try {
    return typeof window !== 'undefined' ? window.localStorage : null;
  } catch {
    return null;
  }
}

// Saved definitions, or null when nothing usable is stored (the built-in list applies then).
export function loadStoredClientDefinitions(storage) {
  try {
    const raw = storage?.getItem(CLIENT_DEFINITIONS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.length > 0 ? parsed : null;
  } catch {
    return null;
  }
}

// Stores the full list. An empty or invalid list removes the entry. Returns false when the
// browser refused the write (the in-memory list still works for the current session).
export function saveStoredClientDefinitions(storage, definitions) {
  try {
    if (!storage) return false;
    if (!Array.isArray(definitions) || definitions.length === 0) {
      storage.removeItem(CLIENT_DEFINITIONS_KEY);
    } else {
      storage.setItem(CLIENT_DEFINITIONS_KEY, JSON.stringify(definitions));
    }
    return true;
  } catch {
    return false;
  }
}
