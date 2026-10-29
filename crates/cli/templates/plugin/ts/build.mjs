import { execFileSync } from 'node:child_process';

const jco = process.platform === 'win32' ? 'jco.cmd' : 'jco';
const wit = process.env.OWASIDEA_WIT_DIR ?? '.owasidea/wit';
const out = process.env.OWASIDEA_OUT ?? '{{name}}.component.wasm';

execFileSync(
  jco,
  ['componentize', 'src/index.ts', '--wit', wit, '--world-name', '{{name}}', '-o', out],
  { stdio: 'inherit' },
);
