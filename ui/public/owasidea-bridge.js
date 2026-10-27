const BRIDGE_HANDSHAKE_TIMEOUT_MS = 5000;
const BRIDGE_DEFAULT_TIMEOUT_MS = 10000;
const BRIDGE_MAX_TIMEOUT_MS = 30000;

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

  // send hello
  try { port.postMessage({ type: 'owasidea:bridge:hello', nonce }); } catch {}

  return { call, cancel, port };
}

let _initResolve = null;
let _initPromise = new Promise((res) => { _initResolve = res; });
let _initReceived = null;

function _handleInit(e) {
  const data = e && e.data;
  if (!data || data.type !== 'owasidea:bridge:init') return;
  const p = e.ports && e.ports[0];
  if (!p || _initReceived) return;
  _initReceived = { port: p, nonce: data.nonce };
  if (_initResolve) _initResolve(_initReceived);
}
if (typeof window !== 'undefined') {
  window.addEventListener('message', _handleInit);
}

export function connect() {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      reject(new BridgeError('BRIDGE_DISABLED', 'handshake timeout'));
    }, BRIDGE_HANDSHAKE_TIMEOUT_MS);

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
