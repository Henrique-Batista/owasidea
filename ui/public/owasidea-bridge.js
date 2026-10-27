const BRIDGE_HANDSHAKE_TIMEOUT_MS = 5000;
const BRIDGE_DEFAULT_TIMEOUT_MS = 10000;
const BRIDGE_MAX_TIMEOUT_MS = 30000;
const BRIDGE_PROTOCOL_VERSION = 1;
const BRIDGE_NONCE_PARAM = '__owasidea_bridge';

class BridgeError extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'BridgeError';
    this.code = code;
  }
}

function genId() {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {}
  return 'b-' + Math.random().toString(36).slice(2) + Date.now().toString(36);
}

function parseCallArgs(args) {
  let plugin = null;
  let fn = null;
  let callArgs = [];
  if (typeof args[0] === 'string') {
    if (args[0].indexOf('.') >= 0 && typeof args[1] !== 'string') {
      const parts = args[0].split('.');
      plugin = parts[0];
      fn = parts[1];
      callArgs = Array.isArray(args[1]) ? args[1] : (args[1] == null ? [] : [args[1]]);
    } else {
      plugin = args[0];
      fn = args[1];
      callArgs = Array.isArray(args[2]) ? args[2] : (args[2] == null ? [] : [args[2]]);
    }
  } else {
    plugin = args[0];
    fn = args[1];
    callArgs = Array.isArray(args[2]) ? args[2] : (args[2] == null ? [] : [args[2]]);
  }
  return { plugin, fn, callArgs };
}

function createBridge(port, nonce) {
  const pending = new Map();

  function handleResponse(ev) {
    const d = ev && ev.data;
    if (!d || typeof d.id !== 'string') return;
    const entry = pending.get(d.id);
    if (!entry) return;
    clearTimeout(entry.timer);
    pending.delete(d.id);
    if (d.ok) {
      if (Object.prototype.hasOwnProperty.call(d, 'results')) {
        entry.resolve(d.results);
      } else {
        entry.resolve(d.result);
      }
    } else {
      const err = (d.error && typeof d.error === 'object') ? d.error : {};
      entry.reject(new BridgeError(err.code || 'INTERNAL', err.message || 'error'));
    }
  }

  port.onmessage = handleResponse;

  function call(...args) {
    const { plugin, fn, callArgs } = parseCallArgs(args);
    if (!plugin || !fn) {
      return Promise.reject(new BridgeError('INVALID_REQUEST', 'missing plugin/function'));
    }
    const id = genId();
    const timeoutMs = Math.min(
      Math.max((args.length > 3 && typeof args[3] === 'number') ? args[3] : BRIDGE_DEFAULT_TIMEOUT_MS, 1),
      BRIDGE_MAX_TIMEOUT_MS
    );

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (pending.has(id)) {
          pending.delete(id);
          try { port.postMessage({ id, type: 'cancel' }); } catch {}
          reject(new BridgeError('TIMEOUT', 'timeout'));
        }
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        port.postMessage({ id, plugin, function: fn, args: callArgs });
      } catch (e) {
        clearTimeout(timer);
        pending.delete(id);
        reject(new BridgeError('INTERNAL', String(e && e.message ? e.message : e)));
      }
    });
  }

  function cancel(id) {
    const entry = pending.get(id);
    if (!entry) return false;
    clearTimeout(entry.timer);
    pending.delete(id);
    try { port.postMessage({ id, type: 'cancel' }); } catch {}
    entry.reject(new BridgeError('CANCELLED', 'cancelled'));
    return true;
  }

  // send hello over the transferred port; the parent matches it against the
  // nonce it generated for this iframe session.
  try { port.postMessage({ type: 'owasidea:bridge:hello', nonce }); } catch {}

  return { call, cancel, port };
}

let _initResolve = null;
let _initPromise = new Promise((res) => { _initResolve = res; });
let _initReceived = null;

// The dashboard mounts this document with a one-time nonce in the query string.
// It is the only document-bound value available in a sandboxed (opaque origin)
// iframe: a navigated document does not carry it, so it cannot obtain a port.
let _urlNonce = null;
try {
  _urlNonce = new URLSearchParams(location.search).get(BRIDGE_NONCE_PARAM);
} catch {}
// Keep the plugin's own URL clean for apps that parse query strings.
if (_urlNonce) {
  try {
    const url = new URL(location.href);
    url.searchParams.delete(BRIDGE_NONCE_PARAM);
    history.replaceState(history.state, '', url.toString());
  } catch {}
}

function _parentWindow() {
  if (typeof window === 'undefined') return null;
  try {
    return window.parent && window.parent !== window ? window.parent : null;
  } catch {
    return null;
  }
}

// Announce readiness to the dashboard. The message carries no secret; it only
// tells the parent that this document's SDK is listening, so the parent can
// transfer the MessagePort to the verified document.
function announce() {
  const parent = _parentWindow();
  if (!parent || _initReceived || !_urlNonce) return;
  try {
    parent.postMessage(
      { type: 'owasidea:bridge:ready', version: BRIDGE_PROTOCOL_VERSION, nonce: _urlNonce },
      '*'
    );
  } catch {}
}

function _handleMessage(e) {
  const data = e && e.data;
  if (!data || typeof data !== 'object') return;
  // Only the embedding dashboard may drive the handshake; ignore messages from
  // other windows/frames.
  if (e.source !== window.parent) return;

  if (data.type === 'owasidea:bridge:probe') {
    if (data.version !== BRIDGE_PROTOCOL_VERSION) return;
    if (!_initReceived) announce();
    return;
  }

  if (data.type !== 'owasidea:bridge:init') return;
  if (data.version !== BRIDGE_PROTOCOL_VERSION) return;
  // Refuse init that does not match the nonce this document was mounted with.
  if (typeof data.nonce !== 'string' || data.nonce.length === 0) return;
  if (!_urlNonce || data.nonce !== _urlNonce) return;
  const p = e.ports && e.ports[0];
  if (!p || typeof p.postMessage !== 'function') return;
  if (_initReceived) {
    // Single-use handshake: a second init for this document is refused.
    try { p.close(); } catch {}
    return;
  }
  _initReceived = { port: p, nonce: data.nonce };
  if (_initResolve) _initResolve(_initReceived);
}

if (typeof window !== 'undefined') {
  window.addEventListener('message', _handleMessage);
  // The parent may attach after this script ran, so announce once on load and
  // rely on `probe` for late attach. Announce immediately when possible.
  announce();
  if (typeof document !== 'undefined' && document.readyState !== 'complete') {
    document.addEventListener('DOMContentLoaded', announce, { once: true });
  }
}

export function connect() {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      reject(new BridgeError('BRIDGE_DISABLED', 'handshake timeout'));
    }, BRIDGE_HANDSHAKE_TIMEOUT_MS);

    if (!_initReceived) announce();

    (async () => {
      try {
        const init = _initReceived || await Promise.race([
          _initPromise,
          new Promise((_, rej) => setTimeout(() => rej(new Error('timeout')), BRIDGE_HANDSHAKE_TIMEOUT_MS)),
        ]);
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        const bridge = createBridge(init.port, init.nonce);
        resolve(bridge);
      } catch {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        reject(new BridgeError('BRIDGE_DISABLED', 'handshake timeout'));
      }
    })();
  });
}

export { BridgeError };

// IIFE/global fallback
try {
  if (typeof window !== 'undefined') {
    window.OwasideaBridge = { connect, BridgeError };
  }
} catch {}
