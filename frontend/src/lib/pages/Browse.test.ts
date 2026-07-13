import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import Browse from "./Browse.svelte";
import { navigate } from "../router.svelte";
import { setPageSeed } from "../viewseed.svelte";
import type { Entity, SchemaMap } from "../types";

vi.mock("../router.svelte", () => ({ navigate: vi.fn() }));
vi.mock("../viewseed.svelte", () => ({
  setPageSeed: vi.fn(),
  blockFromBrowseParams: (source: string) => ({ view: source, source, layout: "table", filter: null, sort: null }),
}));

const schemas: SchemaMap = {
  물건: { name: "물건", category: "컬렉션", fields: { 이름: { kind: "text", required: true } } },
};

function mockList(entities: Entity[]) {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200, json: () => Promise.resolve(entities) }));
}

afterEach(() => vi.unstubAllGlobals());

describe("Browse", () => {
  it("뷰로 저장이 씨앗을 설정하고 새 페이지로 이동한다", async () => {
    mockList([]);
    render(Browse, { schemas, type: "물건", params: { 상태: "위시" } });
    await fireEvent.click(await screen.findByRole("button", { name: "뷰로 저장" }));
    expect(setPageSeed).toHaveBeenCalled();
    expect(navigate).toHaveBeenCalledWith("/pages/new");
  });

  it("브레드크럼·항목수·새 항목 버튼", async () => {
    mockList([{ id: "1", type: "물건", data: { 이름: "A" }, created_at: "", updated_at: "" }]);
    render(Browse, { schemas, type: "물건", params: {} });
    expect(await screen.findByText(/컬렉션 › 물건/)).toBeInTheDocument();
    expect(await screen.findByText(/1개/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /새 물건/ })).toBeInTheDocument();
  });

  it("0건이면 빈 상태 카드", async () => {
    mockList([]);
    render(Browse, { schemas, type: "물건", params: {} });
    expect(await screen.findByText(/아직 물건이 없어요/)).toBeInTheDocument();
  });
});
