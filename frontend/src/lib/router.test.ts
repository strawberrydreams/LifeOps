import { describe, it, expect } from "vitest";
import { parseRoute } from "./router.svelte";

describe("parseRoute", () => {
  it("browse 경로와 쿼리 파라미터", () => {
    const r = parseRoute("/browse/물건?상태=위시&sort=-가격");
    expect(r).toMatchObject({ name: "browse", type: "물건", params: { 상태: "위시", sort: "-가격" } });
  });
  it("entity 상세", () => {
    expect(parseRoute("/entity/abc")).toMatchObject({ name: "entity", id: "abc" });
  });
  it("new 생성", () => {
    expect(parseRoute("/new/시계")).toMatchObject({ name: "new", type: "시계" });
  });
  it("pages 커스텀 페이지", () => {
    expect(parseRoute("/pages/데일리 대시보드")).toMatchObject({ name: "page", pageName: "데일리 대시보드" });
  });
  it("타입 생성/수정 라우트를 파싱한다", () => {
    expect(parseRoute("/types/new")).toEqual({ name: "type-new" });
    expect(parseRoute("/types/%EB%AC%BC%EA%B1%B4/edit")).toEqual({
      name: "type-edit",
      type: "물건",
    });
  });
  it("페이지 생성/수정 라우트를 파싱한다", () => {
    expect(parseRoute("/pages/new")).toEqual({ name: "page-new" });
    expect(parseRoute("/pages/홈/edit")).toEqual({ name: "page-edit", pageName: "홈" });
    expect(parseRoute("/pages/데일리 대시보드")).toMatchObject({ name: "page", pageName: "데일리 대시보드" });
  });
  it("루트는 home", () => {
    expect(parseRoute("/")).toMatchObject({ name: "home" });
  });
  it("잘못된 percent encoding도 예외 없이 원문으로 파싱한다", () => {
    expect(parseRoute("/pages/%E0%A4%A")).toEqual({ name: "page", pageName: "%E0%A4%A" });
  });
});
