import { execFileSync } from 'node:child_process';

const jco = process.platform === 'win32' ? 'jco.cmd' : 'jco';
const wit = process.env.OWASIDEA_WIT_DIR ?? 'wit';
const out = process.env.OWASIDEA_OUT ?? 'changelog-ts.component.wasm';

execFileSync(
  jco,
  ['componentize', 'src/index.ts', '--wit', wit, '--world-name', 'changelog', '-o', out],
  { stdio: 'inherit' },
);
