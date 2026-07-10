import { describe, it, expect } from "vitest";
import { formatTimestamp, resolveProvenance } from "./provenance";
import type { Entity } from "./types";

function entity(data: Record<string, unknown>): Entity {
  return { id: "e1", type: "측정", data, created_at: "", updated_at: "2026-07-08T00:00:00Z" };
}

describe("resolveProvenance", () => {
  it("$meta 없으면 기본값 파생(source=manual, updatedAt=엔티티 updated_at)", () => {
    const p = resolveProvenance(entity({ 값: 72 }), "값");
    expect(p.source).toBe("manual");
    expect(p.sensitivity).toBe("normal");
    expect(p.confidence).toBeNull();
    expect(p.updatedAt).toBe("2026-07-08T00:00:00Z");
  });

  it("$meta 항목이 있으면 그 값을 쓴다", () => {
    const p = resolveProvenance(
      entity({ 값: 72, $meta: { 값: { source: "imported", confidence: 0.7, sensitivity: "sensitive", updatedAt: "2026-01-01T00:00:00Z" } } }),
      "값"
    );
    expect(p.source).toBe("imported");
    expect(p.confidence).toBe(0.7);
    expect(p.sensitivity).toBe("sensitive");
    expect(p.updatedAt).toBe("2026-01-01T00:00:00Z");
  });

  it("malformed $meta 값은 기본값으로 대체한다", () => {
    const p = resolveProvenance(
      entity({
        값: 72,
        $meta: {
          값: {
            source: 123,
            confidence: "0.7",
            sensitivity: "secret",
            updatedAt: false,
          },
        },
      }),
      "값"
    );

    expect(p).toEqual({
      source: "manual",
      confidence: null,
      sensitivity: "normal",
      updatedAt: "2026-07-08T00:00:00Z",
    });
  });

  it("$meta 또는 $meta 항목이 객체가 아니면 없는 것처럼 처리한다", () => {
    expect(resolveProvenance(entity({ 값: 72, $meta: "bad" }), "값")).toEqual({
      source: "manual",
      confidence: null,
      sensitivity: "normal",
      updatedAt: "2026-07-08T00:00:00Z",
    });
    expect(resolveProvenance(entity({ 값: 72, $meta: { 값: null } }), "값")).toEqual({
      source: "manual",
      confidence: null,
      sensitivity: "normal",
      updatedAt: "2026-07-08T00:00:00Z",
    });
  });
});

describe("formatTimestamp", () => {
  it("파싱 가능한 timestamp는 로컬 날짜 표시를 반환한다", () => {
    const ts = "2026-07-08T00:00:00Z";
    expect(formatTimestamp(ts)).toBe(new Date(ts).toLocaleDateString());
  });

  it("파싱 실패 시 원문을 그대로 반환한다", () => {
    expect(formatTimestamp("not-a-date")).toBe("not-a-date");
  });
});
