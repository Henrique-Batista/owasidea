type Bucket = 'breaking' | 'features' | 'fixes' | 'maintenance';

function bucketFor(change: string): Bucket {
  const lower = change.toLowerCase();
  if (lower.includes('breaking') || lower.startsWith('!')) return 'breaking';
  if (lower.startsWith('feat') || lower.includes('add') || lower.includes('new')) return 'features';
  if (lower.startsWith('fix') || lower.includes('repair') || lower.includes('bug')) return 'fixes';
  return 'maintenance';
}

export function render(version: string, changes: string[]): string {
  const buckets: Record<Bucket, string[]> = {
    breaking: [],
    features: [],
    fixes: [],
    maintenance: [],
  };
  for (const change of changes) {
    const text = change.trim();
    if (!text) continue;
    buckets[bucketFor(text)].push(text.replace(/^!\s*/, ''));
  }

  const sections: Array<[Bucket, string]> = [
    ['breaking', 'Breaking changes'],
    ['features', 'Features'],
    ['fixes', 'Fixes'],
    ['maintenance', 'Maintenance'],
  ];

  const lines = [`# ${version} — Shipyard release notes`, ''];
  for (const [key, title] of sections) {
    if (buckets[key].length === 0) continue;
    lines.push(`## ${title}`, '');
    for (const item of buckets[key]) lines.push(`- ${item}`);
    lines.push('');
  }
  lines.push(`_Prepared by the TypeScript changelog component._`);
  return lines.join('\n');
}

export const api = { render };
