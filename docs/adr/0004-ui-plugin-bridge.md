# ADR 0004 — Bridge de UI plugin (MessageChannel, nonce, allowlist)

- Status: Accepted (REV/SEC reviewed)
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

4. **Handshake com nonce (fluxo explícito).**
   - O `port2` é transferido por uma mensagem de **controle na `window`** (único
     canal possível para entregar um `MessagePort` a um iframe sandboxed). O
     `hello` subsequente chega pelo **port transferido**, não pela `window`; por
     isso não existe `event.source` nessa mensagem.
   - Fluxo:
     1. Parent cria `MessageChannel` (`port1`, `port2`) e gera `nonce` (128 bits)
        para a sessão do iframe. Nenhum dado é enviado ainda.
     2. Após `load` do iframe (hashes já verificados antes do mount), o parent
        envia a mensagem de controle:
        ```js
        iframe.contentWindow.postMessage(
          { type: 'owasidea:bridge:init', version: 1, nonce, port: port2 },
          '*', [port2]
        );
        ```
        Neste ponto o parent valida `event.source === iframe.contentWindow` na
        **mensagem de controle** (window) e descarta qualquer mensagem de outras
        fontes.
     3. O iframe (SDK) recebe o `port2` e responde **pelo port**:
        ```js
        port.postMessage({ type: 'owasidea:bridge:hello', nonce });
        ```
     4. O parent valida no `port1`: (a) o port é o pendente daquele iframe;
        (b) o `nonce` bate com o gerado; (c) é a primeira `hello` (single-use).
        Só então ativa `port1.onmessage = handleRequest`.
     5. Qualquer `hello` tardia/duplicada/nonce errado fecha o port e não ativa
        o bridge.
   - Autorização efetiva: posse do `port` + nonce single-use. `event.source` só
     se aplica à mensagem de controle (passo 2), nunca ao `hello`.

5. **Origem e identidade.** `event.source === iframe.contentWindow` é validado
   apenas na mensagem de controle da `window` (passo 2 do handshake). As
   mensagens de dados trafegam pelo `MessagePort` e não carregam `origin` nem
   `source`; a autorização é a posse do port + nonce single-use + mapeamento
   `port -> iframe`. O parent nunca aceita mensagem de controle de janelas não
   registradas.

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
   - Timeout: bridge default 10 s, teto 30 s, `timeout = min(configurado, teto)`.
     O default do bridge (10 s) prevalece quando o manifesto não declara
     timeout; o manifesto pode reduzir, nunca ampliar além de 30 s.
     Implementado no parent com `AbortController` e no host com
     `tokio::time::timeout`.
   - O parent (webview) chama o host via HTTP local
     `POST /plugin/{plugin}/call` (único canal disponível ao browser). O host
     **não** deve fazer chamada HTTP a si mesmo; componentes nativos do host
     (se existirem) invocam in-process. Cancelamento é propagado por
     `AbortController` no parent e, no host, pelo drop do future + epoch
     deadline do Wasmtime. Se a implementação não conseguir cancelar o guest
     cooperativamente, isso deve ser documentado como limitação e coberto pelo
     teste de cancelamento (CT-06).
   - O parent devolve:
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

- Módulo ES pequeno, **empacotado same-origin com o bundle do UI plugin**:
  `owasidea build` copia `owasidea-bridge.js` para o output do plugin
  (`dist/owasidea-bridge.js`) e o inclui no manifesto/hashes. O plugin importa
  por caminho relativo (`./owasidea-bridge.js`); não há fetch cross-origin e
  **não** se usa CORS para origem `null`. O framework também pode fornecer o
  arquivo em templates para desenvolvimento local.
- (Dev opcional) o dashboard pode servir o mesmo arquivo em
  `/owasidea-bridge.js`, mas a fonte de verdade em produção é a cópia no bundle
  verificada por hash.
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

### Matriz de validação (CT-06 gate)

| Engine | Status | Observação |
|---|---|---|
| Chromium (Linux) | Validado (2026-10-26) | `postMessage(..., 'null')` inválido; `'*'` entrega; `location` inacessível |
| WebKitGTK (Tauri Linux) | **Não validado** — gate obrigatório do CT-06 antes do merge | Comportamento esperado idêntico; se divergir, usar fallback |
| WebView2 (Tauri Windows) | **Não validado** — gate obrigatório do CT-06 antes do merge | Comportamento esperado idêntico; se divergir, usar fallback |

**Fallback documentado (se um engine rejeitar o transfer ou bloquear a entrega):**
controle via `postMessage` com envelope `{type, nonce, payload}` (sem transferência
de port) + `MessageChannel` criado no parent e compartilhado por nome de sessão
apenas se o engine permitir; caso contrário, desabilitar o bridge naquele
engine (`BRIDGE_DISABLED`) e manter o iframe isolado. A implementação não deve
afirmar suporte validado onde não foi testado.

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
- Cancelamento => `CANCELLED` propagado ao host/guest; teste de cancelamento
  obrigatório (CT-06).
- Concorrência > limite => `CONCURRENCY_LIMIT`.
- Matriz: testes de handshake/transfer/call executados em Chromium, WebKitGTK
  e WebView2; CT-06 não fecha sem os três.
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
  handshake/reload; SDK precisa ser empacotado e versionado com o bundle
  (CT-07); sem CORS para origem `null`.
- Riscos: `targetOrigin '*'` na transferência do port é inevitável no sandbox
  (validado apenas em Chromium; WebKitGTK/WebView2 são gates do CT-06); depende
  de verificação de hashes antes do mount; SEC deve revisar constantes e
  sanitização de erros.

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