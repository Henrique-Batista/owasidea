import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { webkit } from '@playwright/test';

/**
 * Records whether WebKit can actually launch on this host. The bridge suite
 * must not claim WebKit coverage when the engine cannot start (e.g. missing
 * `libicu74`/`libflite1` host libraries), so workers read this marker and skip
 * with a visible reason instead of failing or silently passing.
 */
export default async function globalSetup() {
  const marker = join(process.cwd(), 'test-results', 'webkit-unavailable.txt');
  try {
    const browser = await webkit.launch();
    await browser.close();
    rmSync(marker, { force: true });
  } catch (error) {
    mkdirSync(join(process.cwd(), 'test-results'), { recursive: true });
    const reason = String(error instanceof Error ? error.message : error).split('\n')[0];
    writeFileSync(marker, `WebKit cannot launch on this host: ${reason}\n`);
    console.warn(
      `[playwright] WebKit unavailable (${reason}). Bridge tests will skip WebKit; ` +
        'install host deps with `npx playwright install-deps webkit` to validate it.'
    );
  }
}
