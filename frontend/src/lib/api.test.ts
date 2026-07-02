import { describe, it, expect, vi, afterEach } from "vitest";
import { ApiError, createEntity, listEntities } from "./api";

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

  it("204 응답(삭제)은 값 없이 통과한다", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 204, json: async () => ({}) } as Response));
    const { deleteEntity } = await import("./api");
    await expect(deleteEntity("x")).resolves.toBeUndefined();
  });
});
