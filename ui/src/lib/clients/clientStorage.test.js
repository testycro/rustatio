import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  CLIENT_DEFINITIONS_KEY,
  loadStoredClientDefinitions,
  saveStoredClientDefinitions,
} from './clientStorage.js';

function memoryStorage(initial = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: key => (data.has(key) ? data.get(key) : null),
    setItem: (key, value) => data.set(key, String(value)),
    removeItem: key => data.delete(key),
    has: key => data.has(key),
  };
}

const definition = { id: 'qbittorrent', name: 'qBittorrent', versions: ['5.3.0'] };

test('nothing stored: load returns null', () => {
  assert.equal(loadStoredClientDefinitions(memoryStorage()), null);
});

test('save then load round-trips the definitions', () => {
  const storage = memoryStorage();
  assert.equal(saveStoredClientDefinitions(storage, [definition]), true);
  assert.deepEqual(loadStoredClientDefinitions(storage), [definition]);
});

test('corrupted or non-array content is ignored', () => {
  assert.equal(
    loadStoredClientDefinitions(memoryStorage({ [CLIENT_DEFINITIONS_KEY]: '{oops' })),
    null
  );
  assert.equal(
    loadStoredClientDefinitions(memoryStorage({ [CLIENT_DEFINITIONS_KEY]: '{"id":"x"}' })),
    null
  );
  assert.equal(
    loadStoredClientDefinitions(memoryStorage({ [CLIENT_DEFINITIONS_KEY]: '[]' })),
    null
  );
});

test('saving an empty list removes the stored entry', () => {
  const storage = memoryStorage();
  saveStoredClientDefinitions(storage, [definition]);
  assert.equal(storage.has(CLIENT_DEFINITIONS_KEY), true);
  assert.equal(saveStoredClientDefinitions(storage, []), true);
  assert.equal(storage.has(CLIENT_DEFINITIONS_KEY), false);
});

test('missing or failing storage never throws', () => {
  assert.equal(loadStoredClientDefinitions(null), null);
  assert.equal(saveStoredClientDefinitions(null, [definition]), false);

  const failing = {
    getItem: () => {
      throw new Error('blocked');
    },
    setItem: () => {
      throw new Error('quota');
    },
    removeItem: () => {
      throw new Error('blocked');
    },
  };
  assert.equal(loadStoredClientDefinitions(failing), null);
  assert.equal(saveStoredClientDefinitions(failing, [definition]), false);
});
