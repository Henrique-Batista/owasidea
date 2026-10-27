# ADR 0004 — Bridge de UI plugin (MessageChannel, nonce, allowlist)

- Status: Proposed (SEC review pendente)
- Data: 2026-10-26
- Relacionado: CT-05, CT-06, CT-07, RF-04, RF-05, ADR 0001, ADR 0003

## Contexto

UI plugins são bundles web estáticos executados em iframe sandbox no webview
(`sandbox="allow-scripts allow-forms"`, origem opaca, sem same-origin, sem
top-navigation, sem popups). Hoje eles **não** têm acesso ao backend: sem
bridge, sem WASI, sem host KV/log/filesystem/socket. O dashboard verifica os
hashes dos assets antes de montar o iframe (`ui/main.ts`, `verifyUiPluginAssets`
+ `renderUiPlugin`), e o host expõe `POST /plugin/{name}/call` com validação de
aridade/tipos e timeout (`crates/host/src/lib.rs`, `call_export`).

A necessidade de CT-07 é permitir que um UI plugin (ex.: `board-svelte`) chame
funções de backend selecionadas (ex.: `changelog-ts.render`) **sem** quebrar o
isolamento do iframe nem expor a API do host inteira. Restrições:

1. O host nunca embute runtime JS (ADR 0001); portanto o bridge não pode ser
   "injetar funções" no iframe.
2. A origem do iframe é `null` (opaca), então `fetch` direto ao host é bloqueado
   por CSP/CORS e daria origem `null` não confiável.
3. O bridge deve ser **opt-in** e **least-privilege**: negado por padrão, só as
   funções explicitamente listadas.
4. Manifestos antigos (sem `bridge`) continuam válidos e idênticos em
   comportamento.
5. UI plugin sem SDK não é afetado.

## Decisão

Adotar um **bridge por iframe baseado em `MessageChannel`**, mediado pelo
parent (dashboard), com handshake por nonce, allowlist por função, validação de
args/aridade/tipos, timeout, limites de payload e concorrência.

### Modelo

1. **Configuração opt-in.** No `wasm-app.toml`, seção do plugin UI:
   ```toml
   [plugins.board-svelte]
   ...
   bridge = ["changelog-ts.render"]
   ```
   O campo é opcional (default `[]`). `owasidea build` copia a lista para o
   manifesto gerado em `metadata.ui.bridge` (ordenada, deduplicada) e **falha o
   build** se algum `plugin.fn` não existir ou o export não for encontrado.
   Manifesto sem `bridge` é tratado como `[]`.

2. **Bridge só se necessário.** O parent só cria `MessageChannel` e inicia
   handshake para iframes cujo manifesto tenha `bridge` não vazio. Caso
   contrário o iframe permanece 100% isolado, exatamente como hoje.

3. **Canal por iframe.** Para cada iframe bridged, o parent cria um
   `MessageChannel` (`port1` no parent, `port2` transferido ao iframe). O
   `port2` é transferido **uma única vez**, no handshake, e o parent mantém o
   mapeamento `port -> {iframe, uiPlugin, nonce, inFlight}`.

4. **Handshake com nonce.**
   - Parent gera `nonce` (128 bits, `crypto.randomUUID()` ou equivalente) por
     sessão de iframe.
   - Após o `load` do iframe (e após verificação de hashes, que ocorre antes da
     criação do iframe), o parent envia:
     ```js
     iframe.contentWindow.postMessage(
       { type: 'owasidea:bridge:init', version: 1, nonce, port: port2 },
       '*', [port2]
     );
     ```
   - O iframe (SDK) responde pelo `port2`:
     ```js
     port.postMessage({ type: 'owasidea:bridge:hello', nonce });
     ```
   - O parent valida `nonce` contra o pendente daquele iframe e só então ativa
     o `port1` (`port1.onmessage = handleRequest`). Qualquer `hello` com nonce
     errado/duplicado é descartado e o port fechado.

5. **Origem e identidade.** O parent valida `event.source === iframe.contentWindow`
   na mensagem de controle inicial. Como o port é a capacidade, as mensagens
   subsequentes não dependem de `origin`; a posse do port + nonce é o que
   autoriza. O parent nunca aceita `postMessage` de janelas não registradas.

6. **Processamento de requests (parent).** Mensagem recebida em `port1`:
   `{ id, plugin, function, args }`.
   - `id` deve ser único na sessão (string/uuid). Duplicado => `INVALID_REQUEST`.
   - `(plugin, function)` deve estar **exatamente** na allowlist do UI plugin.
     Sem wildcards, sem prefixos.
   - O plugin headless deve existir e o export deve existir no manifesto.
   - `args` deve ser array; aridade deve bater com `params` do export (CT-01/CT-02).
   - Cada arg deve validar contra o tipo declarado (primitivos, list, option,
     result, tuple, record, enum, flags).
   - Payload serializado <= `BRIDGE_MAX_REQUEST_BYTES` (default 64 KiB).
   - Concorrência: <= `BRIDGE_MAX_IN_FLIGHT` (default 4) por iframe; excesso
     recebe `CONCURRENCY_LIMIT` imediatamente (fail fast).
   - Timeout: `min(manifest timeout, BRIDGE_MAX_TIMEOUT_MS)` (default 10 s,
     teto 30 s). Implementado com `AbortController`/`tokio::time::timeout`.
   - O parent chama internamente `POST /plugin/{plugin}/call` (ou equivalente
     in-process) e devolve:
     ```js
     { id, ok: true, result }          // um valor de retorno
     { id, ok: true, results: [...] }  // múltiplos
     { id, ok: false, error: { code, message } }
     ```
   - Resposta serializada <= `BRIDGE_MAX_RESPONSE_BYTES` (default 256 KiB);
     excedente => `PAYLOAD_TOO_LARGE`.

7. **Cancelamento.** O iframe pode enviar `{ id, type: 'cancel' }` pelo port;
   o parent aborta a chamada em voo e responde `{ id, ok:false, error:{code:'CANCELLED'} }`.
   Timeout também aborta e responde `TIMEOUT`.

8. **Erros tipados (sem vazar segredos).** Códigos:
   `BRIDGE_DISABLED`, `INVALID_REQUEST`, `NOT_ALLOWED`, `UNKNOWN_PLUGIN`,
   `UNKNOWN_EXPORT`, `ARITY_MISMATCH`, `TYPE_MISMATCH`, `PAYLOAD_TOO_LARGE`,
   `CONCURRENCY_LIMIT`, `TIMEOUT`, `CANCELLED`, `HOST_ERROR`, `INTERNAL`.
   `message` é genérico (sem stack trace, paths, SQL, segredos). `HOST_ERROR`
   não repassa o erro bruto do host.

9. **Ciclo de vida.** Reload do iframe invalida o port antigo (close + remoção
   do mapeamento); chamadas em voo recebem `CANCELLED`. Novo handshake cria
   novo nonce/port. Unmount fecha o port. Nenhum port vaza entre iframes.

### SDK (`owasidea-bridge.js`)

- Módulo ES pequeno, distribuído pelo framework (CT-07) em
  `/owasidea-bridge.js` (servido pelo dashboard/host com CORS para origens
  sandboxed) e/ou copiado nos templates.
- API mínima:
  ```js
  import { connect } from '/owasidea-bridge.js';
  const bridge = await connect(); // handshake
  const result = await bridge.call('changelog-ts', 'render', [input]);
  // ou bridge.call('changelog-ts.render', [input])
  ```
  Retorna `Promise` que resolve com `result`/`results` ou rejeita com `BridgeError { code, message }`.
- `connect()` rejeita com `BRIDGE_DISABLED` se o handshake não ocorrer em
  `BRIDGE_HANDSHAKE_TIMEOUT_MS` (default 5 s).

## Alternativas consideradas

| Alternativa | Motivo de rejeição |
|---|---|
| `postMessage` com `targetOrigin '*'` para tudo | Sem validação de origem/identidade; qualquer janela pode enviar/receber; replay e spoofing triviais. Rejeitado como canal de dados. (Nota: o transfer do `port2` usa `'*'` por obrigação do sandbox — ver Segurança.) |
| `fetch` direto do iframe ao host | Bloqueado por CSP/CORS (origem `null`); exigiria CORS permissivo e exporia o host a qualquer script do iframe; viola least-privilege. |
| Host injetar funções no iframe | Exigiria runtime JS no host ou injeção global; viola ADR 0001 ("host nunca embute JS runtime") e o sandbox. |
| Protocolo custom (`owasidea://`) | Mais complexo (registro de protocolo, handlers async, CSP) sem ganho sobre MessageChannel+port; mantido como opção futura. |
| `SharedArrayBuffer`/Atomics | Exige COOP/COEP e complexidade de sincronização; desnecessário para request/response. |

## Segurança

### Ameaças e mitigações

| Ameaça | Mitigação | Limite residual |
|---|---|---|
| UI plugin malicioso | Allowlist por função; validação de args/aridade/tipos; sandbox; timeouts; limites de payload/concorrência | Pode chamar funções permitidas repetidamente dentro dos limites (rate limit futuro) |
| XSS no plugin | Port já está na página; atacante só acessa funções allowlisted; erros sanitizados; sem escalação | XSS ainda pode abusar das funções permitidas |
| Replay de nonce | Nonce por sessão, single-use na ativação, invalidado no reload | Janela entre geração e ativação (mitigada por `event.source`) |
| Port leak | Um port por iframe; close em unmount/reload; sem broadcast; transferido só após verificação de hashes | Port pode ser usado por código injetado no mesmo iframe (mesmo sandbox) |
| DoS | Limites de payload, concorrência, timeout; fail fast | Atacante pode abrir muitos iframes (limitado pelo dashboard) |
| Cross-iframe | Ports não compartilhados; mapeamento `port -> iframe` no parent | Nenhum |
| Spoof de origem | `event.source` validado na handshake; dados trafegam pelo port (posse = capacidade) | Origem `null` não é verificável como string (ver abaixo) |
| Bypass de allowlist | Allowlist lida do manifesto no parent; match exato `plugin.fn`; sem wildcards/dinâmico | Manifesto adulterado poderia ampliar lista (mitigado por build-time validation e signing) |
| Vazamento de segredos | Erros com código + mensagem genérica; sem stack/path/SQL | Mensagens do host podem conter detalhes do plugin; sanitizar no parent |

### `targetOrigin` e sandbox (validado)

Validação realizada em Chromium (2026-10-26):
- Iframe com `sandbox="allow-scripts"` (sem `allow-same-origin`) tem origem serializada
  `"null"`; ler `location.href`/`location.origin` do parent lança `SecurityError`.
- `iframe.contentWindow.postMessage(msg, 'null')` é **rejeitado** pelo Chromium
  ("Invalid target origin 'null'").
- `postMessage(msg, '*')` entrega normalmente.

Consequência: a transferência do `port2` para o iframe sandboxed **precisa** usar
`targetOrigin '*'` — é o único valor aceito. Esta é a **única** exceção permitida
ao "sem wildcard": o transfer carrega apenas o `MessagePort` (nenhum dado/segredo),
é feito uma única vez por sessão, após verificação de hashes e `load`, e é
gatilhado por `event.source === iframe.contentWindow`. Todo o tráfego posterior
ocorre pelo port transferido, validado por nonce e posse.

### Limites operacionais (constantes)

- `BRIDGE_MAX_REQUEST_BYTES` = 64 KiB
- `BRIDGE_MAX_RESPONSE_BYTES` = 256 KiB
- `BRIDGE_MAX_IN_FLIGHT` = 4 por iframe
- `BRIDGE_MAX_TIMEOUT_MS` = 30 000 (default 10 000)
- `BRIDGE_HANDSHAKE_TIMEOUT_MS` = 5 000

### Fora de escopo (v1)

- Sem streaming/subscriptions (request/response apenas).
- Sem handles de arquivo ou capacidades WASI.
- Sem rate limit global (por-iframe apenas).
- Sem bridge para iframes não-UI (dashboard principal não usa bridge).

## Contrato de dados

**Controle (window postMessage):**
```
parent -> iframe: { type: 'owasidea:bridge:init', version: 1, nonce, port }  // port transferido
iframe -> parent(port): { type: 'owasidea:bridge:hello', nonce }
```

**Dados (MessagePort, iframe -> parent):**
```json
{ "id": "uuid", "plugin": "changelog-ts", "function": "render", "args": [ ... ] }
{ "id": "uuid", "type": "cancel" }
```

**Dados (MessagePort, parent -> iframe):**
```json
{ "id": "uuid", "ok": true, "result": <value> }
{ "id": "uuid", "ok": true, "results": [ <value>, ... ] }
{ "id": "uuid", "ok": false, "error": { "code": "TIMEOUT", "message": "..." } }
```

`id` correlaciona request/response. `args` segue a serialização JSON dos tipos
WIT (CT-02). Erros usam apenas os códigos listados em "Erros tipados".

## Migração / compatibilidade

- `bridge` default `[]` em config e manifesto; ausente => sem bridge. Manifestos
  antigos continuam aceitos (tratados como `[]`).
- UI plugin sem SDK não faz handshake; nenhum port é transferido se `bridge`
  vazio; comportamento idêntico ao atual.
- Sem mudança em manifests de plugins headless; apenas `metadata.ui.bridge`
  opcional nos manifests UI.
- Dashboard antigo ignora campo desconhecido; sem quebra.
- Bridge desligado por padrão; opt-in explícito por plugin.

## Critérios de teste (CT-06 / CT-07)

- Chamada permitida retorna `result` e o SDK resolve a Promise.
- Função não listada => `NOT_ALLOWED`.
- Plugin/export desconhecido => `UNKNOWN_PLUGIN` / `UNKNOWN_EXPORT`.
- Aridade errada => `ARITY_MISMATCH`.
- Tipo errado => `TYPE_MISMATCH`.
- Payload > limite => `PAYLOAD_TOO_LARGE`.
- Origem/fonte errada na handshake => port não ativado; SDK recebe
  `BRIDGE_DISABLED`.
- Iframe não registrado => sem port; sem chamada possível.
- Timeout => `TIMEOUT`; port permanece utilizável depois.
- Concorrência > limite => `CONCURRENCY_LIMIT`.
- Reload => port antigo fechado, chamadas em voo `CANCELLED`, novo handshake
  com novo nonce.
- Playwright: `board-svelte` chama `changelog-ts.render` via bridge e exibe
  resultado; função negada é rejeitada.
- Regressão: UI plugin sem `bridge` continua sem port e sem chamadas.

## Consequências

- Positivas: UI plugins ganham acesso controlado e auditável ao backend sem
  perder isolamento; allowlist e validação centralizadas no parent; erros
  tipados e previsíveis; compatível com manifests antigos.
- Negativas: parent assume responsabilidade de proxy/validação; complexidade de
  handshake/reload; SDK precisa ser distribuído e versionado (CT-07); CORS do
  endpoint do SDK.
- Riscos: `targetOrigin '*'` na transferência do port é inevitável no sandbox
  (validado); depende de verificação de hashes antes do mount; SEC deve
  revisar constantes e sanitização de erros.

## Referências

- Chromium (2026-10-26): `postMessage(..., 'null')` inválido; origem sandboxed é
  `"null"`; `location` inacessível. Teste em
  `/tmp/opencode/test-origin/` (parent/child HTML).
- Node.js: `MessageChannel`/`MessagePort` existem e ports são transferíveis
  (`node -e "new MessageChannel()"`).
- Repo: `ui/main.ts` (`renderUiPlugin`, `verifyUiPluginAssets`,
  `sandbox="allow-scripts allow-forms"`); `crates/host/src/lib.rs`
  (`call_export`, `/ui-plugins`, `list_ui_plugins`); `crates/cli/src/main.rs`
  (`build_ui_plugin`, `ui_index_entry`); `docs/UI_PLUGINS.md`;
  `docs/plans/ROADMAP_IMPLEMENTATION_PLAN.md` (CT-05/06/07).
- MDN: `MessageChannel`, `MessagePort.postMessage`, `HTMLIFrameElement.sandbox`,
  `Window.postMessage` targetOrigin.