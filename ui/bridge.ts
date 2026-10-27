const BRIDGE_MAX_REQUEST_BYTES = 64 * 1024;
const BRIDGE_MAX_RESPONSE_BYTES = 256 * 1024;
const BRIDGE_MAX_IN_FLIGHT = 4;
const BRIDGE_MAX_TIMEOUT_MS = 30000;
const BRIDGE_DEFAULT_TIMEOUT_MS = 10000;
const BRIDGE_HANDSHAKE_TIMEOUT_MS = 5000;

type BridgePlugin = { name: string; bridge?: string[] };

interface Pending {
  resolve: (v: any) => void;
  reject: (e: any) => void;
  controller: AbortController;
  timer: number;
}

function byteLength(value: unknown): number {
  return new TextEncoder().encode(JSON.stringify(value)).length;
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

  const disposePort = () => {
    if (currentPort) {
      try { currentPort.close(); } catch {}
      currentPort = null;
    }
  };

  const handleRequest = async (port: MessagePort, msg: any) => {
    const id = typeof msg?.id === 'string' ? msg.id : null;
    const reply = (payload: any) => port.postMessage(payload);

    const fail = (code: string, message: string) => {
      if (id) reply({ id, ok: false, error: { code, message } });
    };

    // cancel
    if (msg?.type === 'cancel' && id) {
      // best-effort: cannot abort tracked easily here; handled by id map below
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

    let timeoutMs = BRIDGE_DEFAULT_TIMEOUT_MS;
    if (typeof msg?.timeout === 'number' && Number.isFinite(msg.timeout)) {
      timeoutMs = Math.max(1, Math.min(BRIDGE_MAX_TIMEOUT_MS, Math.floor(msg.timeout)));
    }

    seenIds.add(id);
    inFlight++;

    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), timeoutMs);

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

      if (timer) window.clearTimeout(timer);

      if (!res.ok) {
        const code = codeForStatus(res.status, text);
        // refine arity/type if message indicates
        let finalCode = code;
        const msgText = (data?.error ?? text ?? '').toString().toLowerCase();
        if (msgText.includes('expected') && msgText.includes('args')) finalCode = 'ARITY_MISMATCH';
        else if (msgText.includes('type') || msgText.includes('json')) finalCode = 'TYPE_MISMATCH';
        // map 404 export
        if (res.status === 404 && msgText.includes('export')) finalCode = 'UNKNOWN_EXPORT';
        if (res.status === 404 && msgText.includes('plugin')) finalCode = 'UNKNOWN_PLUGIN';
        fail(finalCode, 'host error');
        return;
      }

      // response size check
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
      if (timer) window.clearTimeout(timer);
      if (err?.name === 'AbortError') {
        fail('TIMEOUT', 'timeout');
      } else {
        fail('INTERNAL', 'internal error');
      }
    } finally {
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

    const nonce = (crypto as any).randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`;

    let activated = false;
    let helloReceived = false;

    const handshakeTimer = window.setTimeout(() => {
      if (!helloReceived) {
        try { port1.close(); } catch {}
        if (currentPort === port1) currentPort = null;
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
        activated = true;
        port1.onmessage = (e: MessageEvent) => {
          const m = e.data;
          if (m?.type === 'cancel') {
            // no-op here; cancellation handled by id not tracked in this simple impl
            return;
          }
          void handleRequest(port1, m);
        };
        return;
      }
      // before activation ignore
    };

    // send init control
    try {
      (frame.contentWindow as any)?.postMessage(
        { type: 'owasidea:bridge:init', version: 1, nonce, port: port2 },
        '*',
        [port2]
      );
    } catch {
      window.clearTimeout(handshakeTimer);
      disposePort();
    }
  };

  frame.addEventListener('load', handleLoad);
  // if already loaded
  // trigger once
  // handleLoad will be called on next load; also call if complete
  if (frame.contentDocument?.readyState === 'complete') {
    // small delay to ensure contentWindow
    window.setTimeout(handleLoad, 0);
  }

  return () => {
    disposed = true;
    frame.removeEventListener('load', handleLoad);
    disposePort();
  };
}


