import type { Entity } from "./types";

export type Sensitivity = "normal" | "sensitive";

export interface ProvenanceMeta {
  source?: string;
  confidence?: number;
  sensitivity?: Sensitivity;
  updatedAt?: string;
}

export interface ResolvedProvenance {
  source: string;
  confidence: number | null;
  sensitivity: Sensitivity;
  updatedAt: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isSensitivity(value: unknown): value is Sensitivity {
  return value === "normal" || value === "sensitive";
}

/** entity.data.$meta[field]를 읽어 기본값으로 채운 프로비넌스를 만든다. */
export function resolveProvenance(entity: Entity, field: string): ResolvedProvenance {
  const meta = entity.data["$meta"];
  const fieldMeta = isRecord(meta) ? meta[field] : undefined;
  const m = isRecord(fieldMeta) ? fieldMeta : {};

  return {
    source: typeof m.source === "string" ? m.source : "manual",
    confidence: typeof m.confidence === "number" ? m.confidence : null,
    sensitivity: isSensitivity(m.sensitivity) ? m.sensitivity : "normal",
    updatedAt: typeof m.updatedAt === "string" ? m.updatedAt : entity.updated_at,
  };
}

/** RFC3339 -> 로컬 날짜 표시. 파싱 실패 시 원문 그대로. */
export function formatTimestamp(ts: string): string {
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleDateString();
}
