# ADR 0005 — Streaming e progresso em chamadas longas (jobs + SSE + cancel)

- Status: Proposed — protótipo implementado atrás da feature `streaming`;
  revisão ARCH/REV/SEC pendente
- Data: 2026-10-27
- Relacionado: RL-06, CT-08, PX-07, ADR 0001, ADR 0004
- Implementação: `crates/host/src/jobs.rs`, feature `streaming` em
  `crates/host/Cargo.toml`, `plugins/example` (`slow-sum`)

## Contexto

Hoje uma chamada de biblioteca é síncrona: `POST /plugin/{name}/call` espera o
export terminar e devolve um único JSON. O timeout vem de
`permissions.resources.timeout_ms` (default 30 s) e, ao estourar, o host devolve
`504`. Logs já têm SSE (`/plugin/{name}/logs/stream`), mas não existe forma de:

- acompanhar o progresso de um export longo;
- cancelar uma chamada em andamento;
- recuperar o resultado depois de a conexão HTTP cair;
- distinguir “chamada ainda rodando” de “host morreu”.

Serviços (`type = "service"`) têm ciclo de vida próprio (`start`/`stop`/
`status`) e **não** são modelados como chamadas por invocação (AGENTS.md). O
bridge de UI plugins (ADR-0004) é explicitamente request/response e não cobre
streaming. Precisamos de um contrato para chamadas longas de plugins
`library`/`hybrid` sem quebrar `/call` nem exigir mudança nos manifestos atuais.

Restrições:

1. `POST /plugin/{name}/call` deve continuar idêntico e disponível.
2. O novo caminho é opt-in; desligado por padrão (feature/flag).
3. O host não pode embutir runtime JS nem aceitar trabalho ilimitado.
4. O guest é não confiável: limites de payload, concorrência, tempo e memória
   continuam valendo.
5. Progresso do guest só é possível com contrato WIT explícito; o protótipo não
   deve fingir que já o tem.

## Opções consideradas

| Opção | Vantagens | Problemas | Decisão |
|---|---|---|---|
| A. Manter só `/call` síncrono | Simples, zero regressão | Sem progresso/cancelamento; conexão longa frágil | Mantido como caminho estável |
| B. Jobs assíncronos + polling (`202` + `GET /jobs/{id}`) | Desacopla duração da conexão; resultado consultável; idempotente | Latência de polling; sem eventos em tempo real | **Base da decisão** |
| C. Jobs + SSE de eventos (`GET /jobs/{id}/events`) | Progresso em tempo real; mesmo padrão dos logs; simples no axum | Precisa de ring buffer e encerramento determinístico | **Adotado junto com B** |
| D. SSE direto no `POST /call` (streaming da resposta) | Menos endpoints | Quebra clientes/proxies; difícil cancelar e recuperar; sem job id | Rejeitado |
| E. WebSocket | Bidirecional | Complexidade de handshake, proxy e autorização; não necessário | Rejeitado |
| F. WIT `progress` obrigatório em todo plugin | Progresso real do guest | Quebra ABI/manifestos; penaliza plugins que não precisam | **Fase 2, opt-in** |
| G. Cancelar via `stop-service`/drop do host | Já existe para serviços | Não é por chamada; serviço não é job | Rejeitado |

## Decisão

Adotar **jobs assíncronos com eventos SSE e cancelamento**, atrás da feature
Cargo `streaming` (desligada por padrão), mantendo `/call` intacto.

1. **Feature flag.** `cargo build -p owasidea-host --features streaming`
   registra as rotas de job. Sem a feature, nenhuma rota, nenhum job manager e
   nenhum código de execução novo é compilado.
2. **Job id opaco.** Cada job recebe 128 bits aleatórios (`ring::SystemRandom`)
   em hexadecimal. O id não codifica plugin, função nem timestamp.
3. **Criação.** `POST /plugin/{name}/call/jobs` valida plugin/função, tamanho de
   `args`, concorrência e timeout **antes** de aceitar. Resposta `202` com
   `job_id` e URLs de status/eventos/cancelamento.
4. **Execução.** O host cria uma instância Wasmtime por job (mesmo caminho de
   `invoke_export` usado por `/call`) e roda a chamada em `tokio::spawn`. O job
   passa por `queued → running → succeeded|failed|cancelled`.
5. **Progresso.** O protótipo emite eventos de ciclo de vida (`queued`,
   `started`, `finished`) e heartbeats `progress` a cada 500 ms enquanto a
   chamada roda. Um executor também pode emitir `progress` próprio.
6. **SSE.** `GET /jobs/{id}/events` envia `event: job` para cada evento e
   `event: end` quando o job chega a estado terminal. Cliente que conecta depois
   do fim recebe o replay do ring buffer e o `end` imediatamente.
7. **Cancelamento.** `POST /jobs/{id}/cancel` marca o job `cancelled`, emite
   `finished` e acorda a task de execução, que descarta o future. É idempotente.
8. **Timeout.** O timeout efetivo é
   `clamp(manifest.timeout_ms, 1, 300_000)`; ao estourar, o job vira `failed`
   com `error` de timeout. Não há `504` porque a criação já respondeu `202`.
9. **WIT `progress` (fase 2).** A evolução prevista é um import opcional:
   ```wit
   interface progress {
       report: func(percent: option<f32>, message: string);
   }
   ```
   Declarado em `imports.host` e habilitado por permissão no manifesto. O host
   liga o contexto do job à instância e converte `report` em evento `progress`.
   Sem esse import, o plugin continua funcionando com heartbeats do host.
   **Não implementado no protótipo.**

## Contrato HTTP (protótipo)

### Criar job

```http
POST /plugin/{name}/call/jobs
Content-Type: application/json

{ "function": "slow-sum", "args": [50000000] }
```

`202 Accepted`:

```json
{
  "job_id": "9f0c…32 hex chars…",
  "state": "queued",
  "status_url": "/jobs/9f0c…",
  "events_url": "/jobs/9f0c…/events",
  "cancel_url": "/jobs/9f0c…/cancel"
}
```

Erros antes da criação:

| Situação | HTTP | Corpo |
|---|---|---|
| Plugin desconhecido | `404` | `unknown plugin {name}` |
| Plugin não é `library`/`hybrid` | `400` | `plugin {name} is a … plugin; jobs only run library exports` |
| `function` vazio | `400` | `function must not be empty` |
| `function` > 256 bytes | `400` | `function exceeds 256 bytes` |
| `args` > 64 KiB serializados | `413` | `args exceed 65536 bytes` |
| > 8 jobs ativos | `429` | `too many active jobs (limit 8)` |
| RNG falhou | `500` | `failed to generate job id` |
| Export inexistente/aridade/tipo | `202`, depois job `failed` | `error` no status |

O export inexistente só é descoberto ao instanciar o componente; por isso vira
falha de job, não erro síncrono. Isso é intencional para não pagar compilação/
instanciação no caminho de criação.

### Consultar job

```http
GET /jobs/{id}
```

```json
{
  "id": "9f0c…",
  "plugin": "example",
  "function": "slow-sum",
  "state": "running",
  "created_unix_ms": 1793000000000,
  "started_unix_ms": 1793000000012,
  "finished_unix_ms": null,
  "events": [
    { "seq": 1, "ts": 1793000000000, "event": "queued", "data": { "plugin": "example", "function": "slow-sum" } },
    { "seq": 2, "ts": 1793000000012, "event": "started", "data": { "plugin": "example", "function": "slow-sum" } }
  ]
}
```

Quando terminal, inclui `result` (sucesso) ou `error` (falha/cancelamento) e
`finished_unix_ms`. `events` é o snapshot do ring buffer, não a história
completa.

Estados: `queued`, `running`, `succeeded`, `failed`, `cancelled`.

### Eventos (SSE)

```http
GET /jobs/{id}/events
Accept: text/event-stream
```

```
event: job
id: 3
data: {"seq":3,"ts":1793000000512,"event":"progress","data":{"elapsed_ms":500}}

event: job
id: 4
data: {"seq":4,"ts":1793000001012,"event":"finished","data":{"state":"succeeded"}}

event: end
data: {"state":"succeeded"}
```

- `id` é o `seq` monotônico do job (não reinicia quando o ring descarta eventos).
- `event: job` cobre `queued`, `started`, `progress` e `finished`.
- `event: end` é sempre o último e encerra o stream.
- O header `Last-Event-ID` é honrado: o stream começa depois do `seq` informado.
  Um valor inválido é ignorado (equivale a `0`).
- Se o ring descartou eventos entre o último visto e o primeiro disponível, o
  stream emite `event: dropped` com `{"requested_after":N,"first_available":M}`
  antes do replay.
- Se o resultado foi truncado por `MAX_RESULT_BYTES`, o evento `finished`
  carrega `"truncated": true`.
- O stream faz polling interno de 50 ms; não há canal unbounded nem task por
  cliente além do próprio handler.
- Cliente lento não bloqueia o produtor: eventos ficam no ring do job.
- Assinantes SSE são limitados: 4 por job e 64 no host; excedente recebe `429`.
- Todas as respostas `/jobs/*` levam `Cache-Control: no-store`.

### Cancelar job

```http
POST /jobs/{id}/cancel
```

`200 OK` com o `JobView` atual. Se o job já terminou, devolve o estado atual sem
alterar nada (idempotente). `404` para id desconhecido.

## Limites e backpressure

Constantes do protótipo (`crates/host/src/jobs.rs`):

| Constante | Valor | Efeito |
|---|---|---|
| `MAX_ACTIVE_JOBS` | 8 | `429` ao criar além disso |
| `MAX_RETAINED_JOBS` | 64 | jobs terminais antigos são removidos |
| `MAX_EVENTS_PER_JOB` | 256 | ring buffer; descarta o evento mais antigo |
| `MAX_ARGS_BYTES` | 64 KiB | `413` antes de criar |
| `MAX_EVENT_BYTES` | 8 KiB | `data` maior vira `{"truncated":true}` |
| `MAX_RESULT_BYTES` | 256 KiB | resultado maior vira `{"truncated":true,"original_bytes":N}` |
| `MAX_FUNCTION_BYTES` | 256 | nome de função maior recebe `400` |
| `MAX_STREAMS_PER_JOB` | 4 | `429` ao abrir o quinto SSE do mesmo job |
| `MAX_STREAMS_GLOBAL` | 64 | `429` ao abrir o 65º SSE no host |
| `PROGRESS_HEARTBEAT_MS` | 500 ms | heartbeat `progress` |
| timeout default | 30 s | igual a `/call` |
| timeout máximo | 300 s | `clamp` do valor do manifesto |

O job manager reserva slot com `compare_exchange` antes de criar a task, então
duas requisições concorrentes não ultrapassam `MAX_ACTIVE_JOBS`. O slot é
liberado por um guard com `Drop`, inclusive quando o executor entra em panic.
Um panic é capturado com `catch_unwind`, o job vira `failed` (`job panicked`) e
o slot é liberado; o panic não derruba a task de serviço do host. Um job
terminal permanece consultável até ser podado por retenção.

A corrida `cancel` × `queued` é resolvida sob o mutex do registro: a transição
para `running` e a emissão de `started` só acontecem se o job ainda não for
terminal. Se o cancelamento vencer, a task não executa o guest e o job termina
`cancelled` sem evento `started`.

O id do job é gerado com 128 bits do `ring::SystemRandom`. Se o RNG falhar, a
criação falha fechada com `500` e nenhum id previsível é emitido.

Jobs usam o mesmo `instance_slots` do plugin que `/call`
(`permissions.resources.max_instances`, default 1). Com o default, um job longo
serializa outras chamadas ao mesmo plugin; aumentar `max_instances` é a forma
de permitir concorrência real. O limite global de 8 jobs não substitui esse
limite por plugin.

## Segurança

- **Superfície:** as rotas só existem com a feature ligada. O CORS existente
  continua restrito às origens Tauri/Vite; não há `Origin: null`. Respostas
  `/jobs/*` usam `Cache-Control: no-store`.
- **Bind local:** o host recusa bind em endereço não-loopback sem
  `--allow-non-loopback` ou `OWASIDEA_ALLOW_NON_LOOPBACK=1`; a checagem é feita
  antes e depois do bind. O default continua `127.0.0.1`.
- **IDs:** 128 bits aleatórios; não são sequenciais nem adivinháveis. Falha do
  RNG é fail-closed (`500`), nunca fallback previsível. Ids de job não são
  escritos em logs/traces; spans HTTP redigem `/jobs/<id>` para `/jobs/{id}`.
- **Isolamento:** cada job usa o mesmo `PluginRuntime`/linker e as mesmas
  permissões do plugin; jobs não ampliam capabilities.
- **Limites:** payload, resultado, nome de função, concorrência, eventos,
  assinantes SSE, timeout e retenção são aplicados no host. O guest continua
  limitado por fuel/memória do manifesto.
- **Vazamento:** eventos carregam apenas estado/progresso e a mensagem de erro
  do host. Não incluem stack trace, paths, SQL ou segredos. Erros do guest são
  reduzidos a `error: <mensagem>`. Resultado acima de 256 KiB é substituído por
  metadados de truncamento, não devolvido inteiro.
- **DoS:** o número de jobs ativos é fixo; o ring buffer é fixo; SSE é limitado
  a 4 leitores por job e 64 no host, sem fila ilimitada. Um cliente lento não
  bloqueia o produtor.
- **Panic:** panic do executor é capturado, marca o job `failed` e libera o slot
  via guard; não derruba o processo nem vaza concorrência.
- **Cancelamento:** não há preempção cooperativa do guest no protótipo. O host
  descarta o future e a instância; um guest que não devolve controle continua
  consumindo CPU até o drop/timeout do Wasmtime. Isso é uma limitação
  documentada, não uma garantia de parada imediata.
- **Persistência:** jobs vivem só na memória do host; reinício perde jobs e
  eventos.

## Migração e compatibilidade

- Feature `streaming` é **off por padrão**. Sem ela, `/plugin/{name}/call/jobs`,
  `/jobs/{id}`, `/jobs/{id}/events` e `/jobs/{id}/cancel` não existem (`404`), e
  `/call` mantém rota, validação, timeout e métricas atuais.
- Nenhum campo novo em manifestos. Nenhuma mudança em WIT de plugins headless
  no protótipo (o export `slow-sum` é só do exemplo).
- Clientes antigos ignoram as rotas novas; clientes novos devem tratar `202` e
  fazer polling/SSE.
- O WIT `progress` da fase 2 será opt-in via `imports.host` + permissão; plugins
  sem ele não mudam.
- O bridge de UI (ADR-0004) continua request/response. Expor jobs a UI plugins
  exige uma extensão própria do bridge e não faz parte deste ADR.
- Serviços continuam com `start`/`stop`/`status`; jobs não substituem serviços.

## Critérios de teste

Implementados em `crates/host/src/jobs.rs` (`#[cfg(test)]`) e
`crates/host/src/lib.rs`:

- criar job → `202`, id opaco, URLs de status/eventos/cancelamento;
- consultar job em andamento e terminal (resultado e erro);
- cancelar job → `cancelled`, evento `finished`, idempotência;
- cancelamento imediato após criar (incluindo job ainda `queued`) → permanece
  `cancelled`, sem `started` sobrescrevendo o estado;
- cancelar/consultar id desconhecido → `404`;
- SSE emite `queued`/`started`/`progress`/`finished`, termina com `event: end`
  e faz replay quando o cliente conecta após o fim;
- `Last-Event-ID` é honrado e eventos descartados pelo ring geram
  `event: dropped`;
- resultado acima de `MAX_RESULT_BYTES` vira `{"truncated":true}` no status e
  no evento `finished`;
- `args` acima do limite → `413`; nome de função acima de 256 bytes → `400`;
- nono job concorrente → `429`;
- quinto SSE do mesmo job e 65º SSE global → `429`;
- timeout do job → `failed` com erro de timeout, sem travar o host;
- panic do executor → job `failed` e slot liberado (novo job aceito);
- falha do RNG → `500`, sem id previsível;
- respostas `/jobs/*` têm `Cache-Control: no-store`;
- bind não-loopback sem opt-in → erro; loopback continua permitido;
- ring buffer de eventos limitado a `MAX_EVENTS_PER_JOB`;
- feature desligada: rotas de job ausentes (`404`) e `/call` continua roteado
  (teste `job_routes_are_absent_without_streaming_feature`).

Teste manual do exemplo (com a feature ligada e o plugin `example` construído):

```bash
cargo build -p owasidea-host --features streaming
curl -s -X POST localhost:8787/plugin/example/call/jobs \
  -H 'content-type: application/json' \
  -d '{"function":"slow-sum","args":[500000000]}'
curl -N localhost:8787/jobs/<job_id>/events
```

## Consequências

Positivas:

- Chamadas longas deixam de depender de uma conexão HTTP aberta;
- progresso observável e cancelamento explícito;
- contrato pequeno, aditivo e desligado por padrão;
- mesmo caminho de validação/fuel de `/call`, reduzindo divergência.

Negativas / riscos:

- protótipo sem progresso real do guest (só heartbeat do host) até o WIT
  `progress`;
- cancelamento não é cooperativo nem preemptivo no guest;
- jobs em memória, sem persistência/recovery;
- mais uma superfície HTTP para revisar (SEC) e testar sob carga;
- métricas de jobs entram nos contadores do plugin, mas não há métrica própria
  de fila/duração de job ainda.

## Referências

- `docs/adr/0004-ui-plugin-bridge.md` — bridge request/response, sem streaming.
- `docs/API.md` — contrato HTTP, seção “Streaming jobs”.
- `crates/host/src/lib.rs` — `call_export`, `invoke_export`, logs SSE,
  `AppState`, `build_router`.
- `crates/host/src/jobs.rs` — job manager, SSE, limites e executor real.
- `plugins/example` — export `slow-sum` usado como exemplo/teste.
- `docs/plans/ROADMAP_IMPLEMENTATION_PLAN.md` — Fase 6, RL-06.
- Wasmtime 50: `Store`, fuel e `call_async`; não há preempção cooperativa de
  componente neste host.
