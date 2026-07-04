export function parseKind(s: string): { base: string; list: boolean } {
  const m = s.match(/^list<(.+)>$/);
  if (m) return { base: m[1], list: true };
  return { base: s, list: false };
}
