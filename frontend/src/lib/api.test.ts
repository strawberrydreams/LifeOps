import { describe, it, expect, vi, afterEach } from "vitest";
import { ApiError, createEntity, getPage, getSchemas, listEntities, updateEntity } from "./api";

function mockFetch(status: number, body: unknown) {
  return vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  } as Response);
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("api", () => {
  it("400 검증 에러를 ApiError(fields 포함)로 변환한다", async () => {
    vi.stubGlobal("fetch", mockFetch(400, {
      error: { code: "validation", message: "검증 실패", fields: [{ field: "이름", message: "필수 필드" }] },
    }));
    const error = await createEntity("시계", {}).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      code: "validation",
      status: 400,
      fields: [{ field: "이름", message: "필수 필드" }],
    });
  });

  it("listEntities는 type과 필터를 쿼리스트링으로 보낸다", async () => {
    const f = mockFetch(200, []);
    vi.stubGlobal("fetch", f);
    await listEntities("물건", { 상태: "위시", sort: "-가격" });
    const url = (f.mock.calls[0][0] as string);
    expect(url).toContain("/api/entities?");
    expect(decodeURIComponent(url)).toContain("type=물건");
    expect(decodeURIComponent(url)).toContain("상태=위시");
    expect(decodeURIComponent(url)).toContain("sort=-가격");
  });

  it("getSchemas는 types와 categories를 반환한다", async () => {
    vi.stubGlobal("fetch", mockFetch(200, { types: { 노트: { name: "노트", fields: {} } }, categories: [{ name: "메모" }] }));
    const res = await getSchemas();
    expect(res.types["노트"].name).toBe("노트");
    expect(res.categories[0].name).toBe("메모");
  });

  it("getPage가 chart 계열을 파싱한다", async () => {
    vi.stubGlobal("fetch", mockFetch(200, {
      page: "건강",
      blocks: [{
        view: "체중 추세",
        source: "측정",
        layout: "chart",
        x: "시각",
        y: "값",
        chart_type: "bar",
        entities: [],
        aggregates: {},
        chart: [{ name: "체중", points: [{ x: "2026-07-01", y: 82 }] }],
      }],
    }));

    const res = await getPage("건강");
    expect(res.blocks[0].layout).toBe("chart");
    expect(res.blocks[0].chart_type).toBe("bar");
    expect(res.blocks[0].chart?.[0].name).toBe("체중");
    expect(res.blocks[0].chart?.[0].points[0].y).toBe(82);
  });

  it("updateEntity는 spawned를 그대로 전달한다", async () => {
    vi.stubGlobal("fetch", mockFetch(200, { id: "1", type: "할일", data: { 완료: true }, created_at: "", updated_at: "", spawned: { id: "2", type: "할일", data: { 완료: false }, created_at: "", updated_at: "" } }));
    const res = await updateEntity("1", { 완료: true });
    expect(res.spawned?.id).toBe("2");
  });

  it("204 응답(삭제)은 값 없이 통과한다", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 204, json: async () => ({}) } as Response));
    const { deleteEntity } = await import("./api");
    await expect(deleteEntity("x")).resolves.toBeUndefined();
  });

  it("updateSchema는 dry_run 쿼리를 붙인다", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ affected_entities: 2, warnings: ["x"] }), { status: 200 })
    );
    vi.stubGlobal("fetch", fetchMock);
    const { updateSchema } = await import("./api");
    const res = await updateSchema("물건", { fields: {} }, { dryRun: true });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/schemas/%EB%AC%BC%EA%B1%B4?dry_run=true",
      expect.objectContaining({ method: "PUT" })
    );
    expect(res).toEqual({ affected_entities: 2, warnings: ["x"] });
  });

  it("createSchema는 POST /api/schemas로 보낸다", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 201 })
    );
    vi.stubGlobal("fetch", fetchMock);
    const { createSchema } = await import("./api");
    await createSchema({ type: "북마크", fields: { 제목: { kind: "text" } } });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/schemas",
      expect.objectContaining({ method: "POST" })
    );
  });
});
