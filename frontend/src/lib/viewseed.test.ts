import { describe, it, expect } from "vitest";
import { conditionFromParam, blockFromBrowseParams, setPageSeed, takePageSeed } from "./viewseed.svelte";

describe("viewseed", () => {
  it("스칼라와 연산자 조건을 파싱한다", () => {
    expect(conditionFromParam("주문됨")).toBe("주문됨");
    expect(conditionFromParam("200000")).toBe(200000);
    expect(conditionFromParam("gte:200000")).toEqual({ gte: 200000 });
    expect(conditionFromParam("month:2026-07")).toEqual({ month: "2026-07" });
    expect(conditionFromParam("true")).toBe(true);
    expect(conditionFromParam("false")).toBe(false);
    expect(conditionFromParam("")).toBe("");
  });

  it("Browse URL 파라미터를 블록으로 변환한다", () => {
    const block = blockFromBrowseParams("물건", { 상태: "주문됨", 가격: "gte:200000", sort: "-가격" });
    expect(block.source).toBe("물건");
    expect(block.layout).toBe("table");
    expect(block.sort).toBe("-가격");
    expect(block.filter).toEqual({ 상태: "주문됨", 가격: { gte: 200000 } });
  });

  it("페이지 seed를 한 번만 소비한다", () => {
    const block = blockFromBrowseParams("물건", {});
    setPageSeed(block);
    expect(takePageSeed()).toEqual(block);
    expect(takePageSeed()).toBeNull();
  });
});
