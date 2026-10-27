const BRIDGE_MAX_REQUEST_BYTES = 64 * 1024;
const BRIDGE_MAX_RESPONSE_BYTES = 256 * 1024;
const BRIDGE_MAX_IN_FLIGHT = 4;
const BRIDGE_MAX_TIMEOUT_MS = 30000;
const BRIDGE_DEFAULT_TIMEOUT_MS = 10000;
const BRIDGE_HANDSHAKE_TIMEOUT_MS = 5000;
const BRIDGE_SEEN_IDS_LIMIT = 1024;
const BRIDGE_PROBE_INTERVAL_MS = 250;
const BRIDGE_PROTOCOL_VERSION = 1;

/**
 * Query parameter that carries the per-mount handshake nonce to the iframe.
 * It is the only document-bound value available: the sandboxed frame has an
 * opaque origin, so the parent cannot read or verify its URL. A document that
 * was navigated away (e.g. to https://example.com) does not have this parameter
 * and therefore cannot obtain a MessagePort.
 */
export const BRIDGE_NONCE_PARAM = '__owasidea_bridge';

export function createBridgeNonce(): string {
  return makeNonce();
}

export function bridgeSrcWithNonce(src: string, nonce: string): string {
  const separator = src.includes('?') ? '&' : '?';
  return `${src}${separator}${BRIDGE_NONCE_PARAM}=${encodeURIComponent(nonce)}`;
}

type BridgePlugin = {
  name: string;
  bridge?: string[];
  /** Manifest timeout in ms (`permissions.resources.timeout_ms` or `metadata.ui.timeout_ms`). */
  timeoutMs?: number;
  timeout_ms?: number;
};

type PendingRequest = {
  controller: AbortController;
  timer: number;
  settled: boolean;
};

function byteLength(value: unknown): number {
  return new TextEncoder().encode(JSON.stringify(value)).length;
}

function makeNonce(): string {
  if (typeof crypto === 'undefined' || typeof crypto.getRandomValues !== 'function') {
    throw new Error('crypto unavailable');
  }
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

function codeForStatus(status: number, body: string): string {
  const lower = body.toLowerCase();
  if (status === 404) {
    if (lower.includes('export')) return 'UNKNOWN_EXPORT';
    return 'UNKNOWN_PLUGIN';
  }
  if (status === 408 || status === 504) return 'TIMEOUT';
  return 'HOST_ERROR';
}

/**
 * ADR-0004: bridge timeout defaults to 10 s when the manifest does not declare
 * one, and a declared value may only shorten it, never exceed 30 s.
 */
export function resolveBridgeTimeoutMs(plugin: BridgePlugin): number {
  const declared = typeof plugin.timeoutMs === 'number'
    ? plugin.timeoutMs
    : typeof plugin.timeout_ms === 'number' ? plugin.timeout_ms : undefined;
  if (declared === undefined || !Number.isFinite(declared) || declared <= 0) {
    return BRIDGE_DEFAULT_TIMEOUT_MS;
  }
  return Math.min(Math.floor(declared), BRIDGE_MAX_TIMEOUT_MS);
}

export function attachBridge(
  frame: HTMLIFrameElement,
  plugin: BridgePlugin,
  host: string,
  nonceParam?: string
): () => void {
  const bridgeList = Array.isArray(plugin.bridge) ? plugin.bridge.slice() : [];
  if (bridgeList.length === 0) return () => {};

  const timeoutMs = resolveBridgeTimeoutMs(plugin);
  // The nonce travels in the iframe URL (document-bound) and must come back in
  // the SDK's `ready` message before any port is transferred. Callers that set
  // `src` themselves pass it; otherwise it is appended here, still before the
  // load listener is armed.
  let controlNonce = typeof nonceParam === 'string' ? nonceParam : '';
  if (!controlNonce) {
    try {
      controlNonce = createBridgeNonce();
    } catch {
      return () => {};
    }
    const src = frame.getAttribute('src') ?? '';
    if (src && src !== 'about:blank' && !src.includes(`${BRIDGE_NONCE_PARAM}=`)) {
      frame.src = bridgeSrcWithNonce(src, controlNonce);
    }
  }

  let disposed = false;
  // The handshake is armed exactly once, for the first document load that the
  // dashboard verified by hash before mounting the iframe. Any later `load`
  // event means the iframe navigated (the plugin called location.assign/reload
  // or was redirected), so the port is torn down and init is never re-sent:
  // a remote document must not receive the MessagePort.
  let armed = false;
  let loads = 0;
  let activePort: MessagePort | null = null;
  let handshakeDone = false;
  let attempts = 0;
  let lastInitAt = 0;
  let inFlight = 0;

  // Bounded: every attempt goes to the first (hash-verified) document only.
  const MAX_INIT_ATTEMPTS = 3;

  const seenIds = new Set<string>();
  const pending = new Map<string, PendingRequest>();
  const timers: number[] = [];
  let ports: MessagePort[] = [];

  const clearTimers = () => {
    while (timers.length > 0) {
      window.clearTimeout(timers.pop()!);
    }
  };

  const trackTimeout = (fn: () => void, ms: number): number => {
    const id = window.setTimeout(() => {
      const index = timers.indexOf(id);
      if (index >= 0) timers.splice(index, 1);
      fn();
    }, ms);
    timers.push(id);
    return id;
  };

  const settle = (port: MessagePort, id: string, payload: any) => {
    const entry = pending.get(id);
    if (!entry || entry.settled) return;
    entry.settled = true;
    pending.delete(id);
    window.clearTimeout(entry.timer);
    inFlight = Math.max(0, inFlight - 1);
    try { port.postMessage(payload); } catch {}
  };

  const closePorts = (keep?: MessagePort) => {
    for (const port of ports) {
      if (port === keep) continue;
      try { port.close(); } catch {}
    }
    ports = keep ? [keep] : [];
  };

  const abortAll = (port: MessagePort | null) => {
    for (const [id, entry] of Array.from(pending.entries())) {
      if (!entry.settled) {
        try { entry.controller.abort(); } catch {}
        if (port) settle(port, id, { id, ok: false, error: { code: 'CANCELLED', message: 'cancelled' } });
      }
    }
    pending.clear();
    inFlight = 0;
  };

  const teardown = () => {
    clearTimers();
    window.removeEventListener('message', onControlMessage);
    abortAll(activePort);
    closePorts();
    activePort = null;
  };

  const handleRequest = async (port: MessagePort, msg: any) => {
    const id = typeof msg?.id === 'string' ? msg.id : null;
    const reply = (payload: any) => {
      try { port.postMessage(payload); } catch {}
    };

    const fail = (code: string, message: string) => {
      if (id) reply({ id, ok: false, error: { code, message } });
    };

    if (msg?.type === 'cancel') {
      const cid = typeof msg?.id === 'string' ? msg.id : null;
      if (cid && pending.has(cid)) {
        const entry = pending.get(cid)!;
        try { entry.controller.abort(); } catch {}
        settle(port, cid, { id: cid, ok: false, error: { code: 'CANCELLED', message: 'cancelled' } });
      }
      return;
    }

    if (!id) { fail('INVALID_REQUEST', 'missing id'); return; }
    if (seenIds.has(id)) { fail('INVALID_REQUEST', 'duplicate id'); return; }

    const pluginName = typeof msg?.plugin === 'string' ? msg.plugin : '';
    const fnName = typeof msg?.function === 'string' ? msg.function : '';
    const args = msg?.args;
    if (!pluginName || !fnName) { fail('INVALID_REQUEST', 'missing plugin/function'); return; }
    if (!Array.isArray(args)) { fail('INVALID_REQUEST', 'args must be array'); return; }

    const key = `${pluginName}.${fnName}`;
    if (!bridgeList.includes(key)) { fail('NOT_ALLOWED', 'not allowed'); return; }

    let payloadSize = 0;
    try { payloadSize = byteLength({ id, plugin: pluginName, function: fnName, args }); }
    catch { payloadSize = BRIDGE_MAX_REQUEST_BYTES + 1; }
    if (payloadSize > BRIDGE_MAX_REQUEST_BYTES) { fail('PAYLOAD_TOO_LARGE', 'request too large'); return; }

    if (inFlight >= BRIDGE_MAX_IN_FLIGHT) { fail('CONCURRENCY_LIMIT', 'too many in-flight'); return; }

    // Bound the replay window without allowing an evicted id to be replayed:
    // evict the oldest id instead of clearing the whole set.
    if (seenIds.size >= BRIDGE_SEEN_IDS_LIMIT) {
      const oldest = seenIds.values().next().value;
      if (oldest !== undefined) seenIds.delete(oldest);
    }
    seenIds.add(id);

    inFlight++;
    const controller = new AbortController();
    const entry: PendingRequest = { controller, timer: 0, settled: false };
    pending.set(id, entry);
    entry.timer = trackTimeout(() => {
      if (entry.settled) return;
      try { controller.abort(); } catch {}
      settle(port, id, { id, ok: false, error: { code: 'TIMEOUT', message: 'timeout' } });
    }, timeoutMs);

    try {
      const res = await fetch(`${host}/plugin/${encodeURIComponent(pluginName)}/call`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ function: fnName, args }),
        signal: controller.signal,
      });

      const text = await res.text();
      let data: any = null;
      try { data = text ? JSON.parse(text) : null; } catch { data = null; }

      if (!res.ok) {
        let finalCode = codeForStatus(res.status, text);
        const msgText = (data?.error ?? text ?? '').toString().toLowerCase();
        if (msgText.includes('expected') && msgText.includes('args')) finalCode = 'ARITY_MISMATCH';
        else if (msgText.includes('type') || msgText.includes('json')) finalCode = 'TYPE_MISMATCH';
        if (res.status === 404 && msgText.includes('export')) finalCode = 'UNKNOWN_EXPORT';
        if (res.status === 404 && msgText.includes('plugin')) finalCode = 'UNKNOWN_PLUGIN';
        if (entry.settled) return;
        settle(port, id, { id, ok: false, error: { code: finalCode, message: 'host error' } });
        return;
      }

      if (byteLength(data) > BRIDGE_MAX_RESPONSE_BYTES) {
        if (entry.settled) return;
        settle(port, id, { id, ok: false, error: { code: 'PAYLOAD_TOO_LARGE', message: 'response too large' } });
        return;
      }

      if (entry.settled) return;
      if (data && Object.prototype.hasOwnProperty.call(data, 'results')) {
        settle(port, id, { id, ok: true, results: data.results });
      } else if (data && Object.prototype.hasOwnProperty.call(data, 'result')) {
        settle(port, id, { id, ok: true, result: data.result });
      } else {
        settle(port, id, { id, ok: true, result: data });
      }
    } catch (err: any) {
      if (entry.settled) return;
      if (err?.name === 'AbortError') {
        // A cancel request settles before aborting; anything left is a timeout.
        settle(port, id, { id, ok: false, error: { code: 'TIMEOUT', message: 'timeout' } });
      } else {
        settle(port, id, { id, ok: false, error: { code: 'INTERNAL', message: 'internal error' } });
      }
    }
  };

  const sendInit = () => {
    if (disposed || !armed || handshakeDone || loads !== 1) return;
    if (attempts >= MAX_INIT_ATTEMPTS) return;
    const target = frame.contentWindow;
    if (!target) return;
    let channel: MessageChannel;
    try {
      channel = new MessageChannel();
    } catch {
      teardown();
      return;
    }
    // Same nonce the document was mounted with; the SDK refuses anything else.
    const nonce = controlNonce;
    attempts++;
    lastInitAt = Date.now();
    const port1 = channel.port1;
    const port2 = channel.port2;
    ports.push(port1);

    port1.onmessage = (ev: MessageEvent) => {
      const data = ev.data;
      if (handshakeDone) return;
      if (!data || data.type !== 'owasidea:bridge:hello') return;
      if (typeof data.nonce !== 'string' || data.nonce !== nonce) {
        teardown();
        return;
      }
      handshakeDone = true;
      activePort = port1;
      clearTimers();
      closePorts(port1);
      port1.onmessage = (e: MessageEvent) => {
        void handleRequest(port1, e.data);
      };
    };

    try {
      // targetOrigin '*' is unavoidable for an opaque-origin (sandboxed) frame;
      // ADR-0004 documents it as the only permitted wildcard. Attempts are
      // bounded and only ever target the first hash-verified document.
      target.postMessage(
        { type: 'owasidea:bridge:init', version: BRIDGE_PROTOCOL_VERSION, nonce, port: port2 },
        '*',
        [port2]
      );
    } catch {
      teardown();
    }
  };

  function onControlMessage(ev: MessageEvent) {
    if (disposed || !armed || handshakeDone) return;
    // Only the registered iframe window may drive the handshake. A message
    // from the dashboard itself, another iframe or an opened window is dropped.
    if (ev.source !== frame.contentWindow) return;
    const data = ev.data;
    if (!data || data.type !== 'owasidea:bridge:ready') return;
    if (data.version !== BRIDGE_PROTOCOL_VERSION) return;
    // The nonce proves this is the document the dashboard mounted: it came from
    // the URL the parent generated, and a navigated/remote document lacks it.
    if (typeof data.nonce !== 'string' || data.nonce !== controlNonce) return;
    if (Date.now() - lastInitAt < BRIDGE_PROBE_INTERVAL_MS) return;
    sendInit();
  }

  const arm = () => {
    if (disposed || armed) return;
    armed = true;
    window.addEventListener('message', onControlMessage);
    const probe = () => {
      if (disposed || handshakeDone || !armed || loads !== 1) return;
      try {
        (frame.contentWindow as any)?.postMessage(
          { type: 'owasidea:bridge:probe', version: BRIDGE_PROTOCOL_VERSION },
          '*'
        );
      } catch {
        return;
      }
      if (!handshakeDone) trackTimeout(probe, BRIDGE_PROBE_INTERVAL_MS);
    };
    // The probe carries no port and no secret; it only tells an already-loaded
    // SDK to announce readiness. The SDK also announces on connect().
    probe();
    // No init is sent until the SDK proves it holds the nonce from the mounted
    // URL. This is what keeps a navigated (remote) document from ever receiving
    // a MessagePort.
    trackTimeout(() => {
      if (!handshakeDone) teardown();
    }, BRIDGE_HANDSHAKE_TIMEOUT_MS);
  };

  const handleLoad = () => {
    if (disposed) return;
    loads++;
    if (loads > 1) {
      // Navigation/reload after the verified load: kill the old session and
      // never hand a port to the new document.
      teardown();
      return;
    }
    arm();
  };

  frame.addEventListener('load', handleLoad);
  // Accessing contentDocument on a sandboxed (opaque-origin) frame throws in
  // WebKit, so only use it to catch frames that already finished loading.
  try {
    if (frame.contentDocument?.readyState === 'complete') {
      trackTimeout(handleLoad, 0);
    }
  } catch {}

  return () => {
    disposed = true;
    frame.removeEventListener('load', handleLoad);
    teardown();
    seenIds.clear();
  };
}
