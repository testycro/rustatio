import { getAuthToken, getRunMode } from '$lib/api.js';

// The Python admin daemon is reached through the authenticated proxy of the Rustatio
// server: /api/daemon-admin/<daemon path>. The daemon routes all live under /api/.
const BASE = '/api/daemon-admin/api';

export class DaemonApiError extends Error {
  constructor(message, status, data) {
    super(message);
    this.name = 'DaemonApiError';
    this.status = status;
    this.data = data;
  }
}

/** Raw request: resolves with { ok, status, data } (data = parsed JSON or null). Throws on network failure. */
export async function daemonRequest(path, { method = 'GET', body } = {}) {
  // Browser-only (WASM) build: no backend, the Daemon view is a demo served by a stand-in
  if (getRunMode() === 'wasm') {
    const { demoRequest } = await import('./demoBackend.js');
    return demoRequest(path, { method, body });
  }

  const headers = {};
  const token = getAuthToken();
  if (token) headers['Authorization'] = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';

  const response = await daemonFetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  let data = null;
  try {
    data = await response.json();
  } catch {
    // empty or non-JSON body
  }

  return { ok: response.ok, status: response.status, data };
}

/** Like daemonRequest, but resolves with the JSON body and throws DaemonApiError when the call failed. */
export async function daemonJson(path, options) {
  const { ok, status, data } = await daemonRequest(path, options);
  if (!ok || data?.success === false) {
    throw new DaemonApiError(data?.error || data?.message || `HTTP ${status}`, status, data);
  }
  return data;
}

// Desktop app: there is no HTTP server, the app forwards the request to the embedded daemon
// (Tauri command daemon_request). Anywhere else: the server's authenticated proxy.
async function daemonFetch(path, init) {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return fetch(`${BASE}${path}`, init);
  }

  const { invoke } = await import('@tauri-apps/api/core');
  let reply;
  try {
    reply = await invoke('daemon_request', {
      method: init.method,
      path,
      body: init.body ?? null,
    });
  } catch (error) {
    // Tauri rejects with the plain error string of the command
    throw new Error(String(error), { cause: error });
  }
  const { status, body } = reply;

  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => JSON.parse(body),
  };
}
