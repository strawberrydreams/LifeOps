import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import PageRenderer from "./PageRenderer.svelte";
import type { PageBlock } from "./api";

describe("PageRenderer", () => {
  it("layout이 card면 카드 레이아웃을 렌더링한다 (table 아님)", () => {
    const blocks: PageBlock[] = [
      {
        view: "카드뷰",
        layout: "card",
        columns: ["이름"],
        entities: [{ id: "e1", type: "물건", data: { 이름: "A" }, created_at: "", updated_at: "" }],
        aggregates: {},
      },
    ];
    const { container, getByText } = render(PageRenderer, { page: "테스트", blocks });

    expect(container.querySelector(".card")).toBeInTheDocument();
    expect(container.querySelector("table")).not.toBeInTheDocument();
    expect(getByText(/A/)).toBeInTheDocument();
  });
});
