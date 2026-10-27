const BRIDGE_MAX_REQUEST_BYTES = 64 * 1024;
const BRIDGE_MAX_RESPONSE_BYTES = 256 * 1024;
const BRIDGE_MAX_IN_FLIGHT = 4;
const BRIDGE_MAX_TIMEOUT_MS = 30000;
const BRIDGE_DEFAULT_TIMEOUT_MS = 10000;
const BRIDGE_HANDSHAKE_TIMEOUT_MS = 5000;
const BRIDGE_SEEN_IDS_LIMIT = 1024;

type BridgePlugin = { name: string; bridge?: string[] };

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

export function attachBridge(frame: HTMLIFrameElement, plugin: BridgePlugin, host: string): () => void {
  const bridgeList = Array.isArray(plugin.bridge) ? plugin.bridge.slice() : [];
  if (bridgeList.length === 0) return () => {};

  let currentPort: MessagePort | null = null;
  let disposed = false;
  let inFlight = 0;
  const seenIds = new Set<string>();
  const controllers = new Map<string, AbortController>();

  const resetState = () => {
    inFlight = 0;
    seenIds.clear();
    controllers.clear();
  };

  const disposePort = () => {
    if (currentPort) {
      try { currentPort.close(); } catch {}
      currentPort = null;
    }
    resetState();
  };

  const handleRequest = async (port: MessagePort, msg: any) => {
    const id = typeof msg?.id === 'string' ? msg.id : null;
    const reply = (payload: any) => port.postMessage(payload);

    const fail = (code: string, message: string) => {
      if (id) reply({ id, ok: false, error: { code, message } });
    };

    if (msg?.type === 'cancel') {
      const cid = typeof msg?.id === 'string' ? msg.id : null;
      if (cid && controllers.has(cid)) {
        const c = controllers.get(cid)!;
        c.abort();
        controllers.delete(cid);
        reply({ id: cid, ok: false, error: { code: 'CANCELLED', message: 'cancelled' } });
        inFlight = Math.max(0, inFlight - 1);
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

    // bound seenIds
    if (seenIds.size >= BRIDGE_SEEN_IDS_LIMIT) {
      // drop oldest-ish by clearing
      seenIds.clear();
    }
    seenIds.add(id);

    inFlight++;
    const controller = new AbortController();
    controllers.set(id, controller);
    const timer = window.setTimeout(() => controller.abort(), BRIDGE_DEFAULT_TIMEOUT_MS);

    let timedOut = false;
    // ensure abort due to timeout marks timedOut
    const onAbort = () => { timedOut = true; };
    controller.signal.addEventListener('abort', onAbort);

    try {
      const res = await fetch(`${host}/plugin/${encodeURIComponent(pluginName)}/call`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ function: fnName, args }),
        signal: controller.signal,
      });
      window.clearTimeout(timer);

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
        fail(finalCode, 'host error');
        return;
      }

      if (byteLength(data) > BRIDGE_MAX_RESPONSE_BYTES) {
        fail('PAYLOAD_TOO_LARGE', 'response too large');
        return;
      }

      if (data && Object.prototype.hasOwnProperty.call(data, 'results')) {
        reply({ id, ok: true, results: data.results });
      } else if (data && Object.prototype.hasOwnProperty.call(data, 'result')) {
        reply({ id, ok: true, result: data.result });
      } else {
        reply({ id, ok: true, result: data });
      }
    } catch (err: any) {
      window.clearTimeout(timer);
      if (timedOut || err?.name === 'AbortError') {
        // distinguish cancel vs timeout: if controller aborted due to cancel request, controllers no longer has id
        if (!controllers.has(id)) {
          fail('CANCELLED', 'cancelled');
        } else {
          fail('TIMEOUT', 'timeout');
        }
      } else {
        fail('INTERNAL', 'internal error');
      }
    } finally {
      controllers.delete(id);
      inFlight = Math.max(0, inFlight - 1);
    }
  };

  const handleLoad = () => {
    if (disposed) return;
    disposePort();
    const channel = new MessageChannel();
    const port1 = channel.port1;
    const port2 = channel.port2;
    currentPort = port1;

    let nonce = '';
    try {
      nonce = makeNonce();
    } catch {
      try { port1.close(); } catch {}
      currentPort = null;
      return;
    }

    let helloReceived = false;

    const handshakeTimer = window.setTimeout(() => {
      if (!helloReceived) {
        try { port1.close(); } catch {}
        if (currentPort === port1) currentPort = null;
        resetState();
      }
    }, BRIDGE_HANDSHAKE_TIMEOUT_MS);

    port1.onmessage = (ev: MessageEvent) => {
      const data = ev.data;
      if (!helloReceived && data?.type === 'owasidea:bridge:hello') {
        if (data?.nonce !== nonce) {
          try { port1.close(); } catch {}
          return;
        }
        helloReceived = true;
        window.clearTimeout(handshakeTimer);
        port1.onmessage = (e: MessageEvent) => {
          void handleRequest(port1, e.data);
        };
        return;
      }
    };

    let attempts = 0;
    const sendInit = () => {
      if (disposed || helloReceived) return;
      try {
        (frame.contentWindow as any)?.postMessage(
          { type: 'owasidea:bridge:init', version: 1, nonce, port: port2 },
          '*',
          [port2]
        );
      } catch {
        window.clearTimeout(handshakeTimer);
        disposePort();
        return;
      }
      if (!helloReceived && attempts < 25) {
        attempts++;
        window.setTimeout(sendInit, 200);
      }
    };
    window.setTimeout(sendInit, 500);
  };

  frame.addEventListener('load', handleLoad);
  if (frame.contentDocument?.readyState === 'complete') {
    window.setTimeout(handleLoad, 0);
  }

  return () => {
    disposed = true;
    frame.removeEventListener('load', handleLoad);
    disposePort();
  };
}
