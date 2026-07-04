import type { ResolvedField } from "./types";
import { parseKind } from "./kind";

export function formatValue(field: ResolvedField, v: unknown): string {
  if (v === null || v === undefined) return "";
  const { base, list } = parseKind(field.kind);
  if (list && Array.isArray(v)) return v.map((x) => formatScalar(base, x)).join(", ");
  return formatScalar(base, v);
}

function formatScalar(base: string, v: unknown): string {
  if (base === "money" && v && typeof v === "object") {
    const m = v as { amount: number; currency: string };
    return `${m.amount.toLocaleString()} ${m.currency}`;
  }
  if (base === "bool") return v === true ? "✓" : "-";
  return String(v);
}
