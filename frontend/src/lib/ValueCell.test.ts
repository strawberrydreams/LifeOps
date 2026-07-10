import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/svelte";
import ValueCell from "./ValueCell.svelte";

describe("ValueCell", () => {
  it("지난 date는 overdue 배지", () => {
    render(ValueCell, { field: { kind: "date", required: false }, value: "2000-01-01", schemas: {} });
    expect(screen.getByText("2000-01-01").className).toContain("overdue");
  });

  it("url은 도메인만 보이는 링크", () => {
    render(ValueCell, { field: { kind: "url", required: false }, value: "https://example.com/x/y", schemas: {} });
    const a = screen.getByRole("link");
    expect(a).toHaveTextContent("example.com");
    expect(a).toHaveAttribute("href", "https://example.com/x/y");
  });

  it("bool은 체크 표시", () => {
    render(ValueCell, { field: { kind: "bool", required: false }, value: true, schemas: {} });
    expect(screen.getByText("✓")).toBeInTheDocument();
  });

  it("entity+fieldName이 있으면 프로비넌스 트리거를 렌더한다", () => {
    render(ValueCell, {
      field: { kind: "number", required: false },
      value: 72,
      schemas: {},
      entity: { id: "e1", type: "측정", data: { 값: 72 }, created_at: "", updated_at: "2026-07-08T00:00:00Z" },
      fieldName: "값",
    });
    expect(screen.getByLabelText("출처 정보")).toBeInTheDocument();
  });

  it("entity가 없으면 트리거를 렌더하지 않는다", () => {
    render(ValueCell, { field: { kind: "number", required: false }, value: 72, schemas: {} });
    expect(screen.queryByLabelText("출처 정보")).toBeNull();
  });
});
