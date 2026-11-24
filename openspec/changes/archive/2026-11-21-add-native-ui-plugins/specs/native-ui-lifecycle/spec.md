# Spec Delta

## Purpose

Defines how native UI processes are started, observed, and stopped alongside the Owasidea host so that a native desktop window and the host lifecycle stay consistent during local development and `run`.

## ADDED Requirements

### Requirement: Start native UIs with the host

`owasidea run` SHALL start the host first, wait until its HTTP address accepts connections, and then start every declared native UI plugin.

Each native UI process MUST receive at least:

- `OWASIDEA_HOST` with the host base URL;
- `OWASIDEA_PLUGIN` with its plugin name;
- `OWASIDEA_MANIFESTS` with the manifest directory in use.

The command MUST fail with a non-zero exit if the host does not become reachable within 30 seconds, and MUST NOT start native UIs in that case.

#### Scenario: Host and native UI start together

- **WHEN** `owasidea run` is executed for an app with one native UI plugin
- **THEN** the native UI process starts only after the host HTTP endpoint accepts connections and receives the three environment variables

#### Scenario: Host fails to start

- **WHEN** the host process exits before becoming reachable
- **THEN** `owasidea run` exits non-zero and starts no native UI process

### Requirement: Process supervision

While `owasidea run` is active, the CLI MUST keep native UI child processes attached to its lifetime:

- child stdout and stderr MUST be forwarded with a per-plugin prefix;
- a child exit MUST be reported with the plugin name and exit status, and MUST NOT terminate the host or the other native UIs;
- terminating the CLI (including SIGINT/SIGTERM) MUST terminate all native UI children and reap them before exit;
- a native UI that exits MUST NOT be restarted automatically in `run`.

#### Scenario: One native UI crashes

- **WHEN** a native UI exits with a non-zero status while `owasidea run` is active
- **THEN** the CLI prints the plugin name and status, the host keeps serving, and no restart occurs

#### Scenario: Operator stops the app

- **WHEN** the user presses Ctrl+C during `owasidea run`
- **THEN** every native UI child is terminated and reaped before the CLI exits

### Requirement: Development controls

`owasidea dev` SHALL expose native UI actions in its interactive dashboard: start, stop, and restart an individual native UI, plus start/stop all. The dashboard MUST show each native UI as running, stopped, or failed.

When a native UI build output changes, `owasidea dev` MUST NOT restart the process automatically; restart MUST remain an explicit action.

#### Scenario: Restart from the dashboard

- **WHEN** the operator selects restart for a running native UI
- **THEN** the old process is terminated and a new one starts with the same environment contract

#### Scenario: Status after crash

- **WHEN** a native UI started from `owasidea dev` exits unexpectedly
- **THEN** the dashboard shows it as failed and offers start and restart actions

### Requirement: Headless and opt-out behaviour

`owasidea run` SHALL accept `--no-native-ui` to start the host without native UI processes. When no graphical display is available, the CLI MUST report the missing display and MUST continue running the host instead of failing the whole command.

#### Scenario: Opt-out

- **WHEN** `owasidea run --no-native-ui` is executed
- **THEN** the host starts and no native UI process is spawned

#### Scenario: No display

- **WHEN** a native UI plugin is declared and the process environment has no display available
- **THEN** the CLI reports that the native UI cannot start and the host remains available
