import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import PageRenderer from "./PageRenderer.svelte";
import type { ChartSeries, PageBlock } from "./api";
import type { SchemaMap } from "./types";

const schemas: SchemaMap = {
  물건: {
    name: "물건",
    extends: null,
    fields: {
      이름: { kind: "text", required: true },
      가격: { kind: "money", required: false },
    },
  },
  측정: {
    name: "측정",
    extends: null,
    fields: {
      지표: { kind: "enum", required: true, options: ["체중", "수면시간"] },
      값: { kind: "number", required: true },
      시각: { kind: "date", required: true },
    },
  },
};

const chart: ChartSeries[] = [
  {
    name: "체중",
    points: [
      { x: "2026-07-01", y: 52.1 },
      { x: "2026-07-02", y: 52.4 },
    ],
  },
];

describe("PageRenderer", () => {
  it("onedit가 있으면 편집 버튼을 보이고 클릭 시 호출한다", async () => {
    const onedit = vi.fn();
    render(PageRenderer, { page: "홈", blocks: [], schemas: {}, onedit });
    await fireEvent.click(screen.getByRole("button", { name: "편집" }));
    expect(onedit).toHaveBeenCalled();
  });

  it("layout이 card면 카드 레이아웃을 렌더링한다 (table 아님)", () => {
    const blocks: PageBlock[] = [
      {
        view: "카드뷰",
        source: "물건",
        layout: "card",
        columns: ["이름"],
        entities: [{ id: "e1", type: "물건", data: { 이름: "A" }, created_at: "", updated_at: "" }],
        aggregates: {},
      },
    ];
    const { container, getByText } = render(PageRenderer, { page: "테스트", blocks, schemas });

    expect(container.querySelector(".card")).toBeInTheDocument();
    expect(container.querySelector("table")).not.toBeInTheDocument();
    expect(getByText(/A/)).toBeInTheDocument();
  });

  it("money 필드는 formatValue로 포맷되어 렌더링된다 ([object Object] 아님)", () => {
    const blocks: PageBlock[] = [
      {
        view: "카드뷰",
        source: "물건",
        layout: "card",
        columns: ["이름", "가격"],
        entities: [
          { id: "e1", type: "물건", data: { 이름: "A", 가격: { amount: 1000, currency: "KRW" } }, created_at: "", updated_at: "" },
        ],
        aggregates: {},
      },
    ];
    const { container, getByText } = render(PageRenderer, { page: "테스트", blocks, schemas });

    expect(getByText(/KRW/)).toBeInTheDocument();
    expect(container.textContent).not.toContain("[object Object]");
  });

  it("card columns 생략 시 예약 메타 필드는 표시하지 않는다", () => {
    const blocks: PageBlock[] = [
      {
        view: "카드뷰",
        source: "물건",
        layout: "card",
        entities: [
          {
            id: "e1",
            type: "물건",
            data: { 이름: "A", $meta: { 이름: { source: "manual" } } },
            created_at: "",
            updated_at: "2026-07-08T00:00:00Z",
          },
        ],
        aggregates: {},
      },
    ];
    const { container } = render(PageRenderer, { page: "테스트", blocks, schemas });

    expect(container.textContent).not.toContain("$meta");
    expect(screen.getByText(/A/)).toBeInTheDocument();
    expect(screen.getAllByLabelText("출처 정보")).toHaveLength(1);
  });

  it("card columns에 명시된 예약 필드는 그대로 표시한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "카드뷰",
        source: "물건",
        layout: "card",
        columns: ["$meta"],
        entities: [
          {
            id: "e1",
            type: "물건",
            data: { 이름: "A", $meta: { 이름: { source: "manual" } } },
            created_at: "",
            updated_at: "2026-07-08T00:00:00Z",
          },
        ],
        aggregates: {},
      },
    ];
    const { container } = render(PageRenderer, { page: "테스트", blocks, schemas });

    expect(container.textContent).toContain("$meta");
  });

  it("table 블록 제목이 browse 링크가 된다", () => {
    const blocks: PageBlock[] = [{
      view: "다가오는", source: "물건",
      filter: { 상태: "주문됨", 배송예정일: { lte: "$today+7d" } }, sort: "배송예정일",
      layout: "table" as const, columns: ["이름"], entities: [], aggregates: {},
    }];
    render(PageRenderer, { page: "홈", blocks, schemas: {} });
    const link = screen.getByRole("link", { name: /다가오는/ });
    const href = link.getAttribute("href")!;
    expect(href).toContain("/browse/%EB%AC%BC%EA%B1%B4");
    expect(decodeURIComponent(href)).toContain("상태=주문됨");
    expect(decodeURIComponent(href)).toContain("배송예정일=lte:$today+7d");
    expect(decodeURIComponent(href)).toContain("sort=배송예정일");
  });

  it("chart layout은 Chart 위젯에 chart 데이터를 line 기본값으로 렌더링한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "추세",
        source: "측정",
        layout: "chart",
        entities: [],
        aggregates: {},
        chart,
        chart_type: null,
      },
    ];
    const { container, getByText } = render(PageRenderer, { page: "홈", blocks, schemas });

    expect(container.querySelector("svg[aria-label='chart']")).toBeInTheDocument();
    expect(container.querySelectorAll("svg path.series")).toHaveLength(1);
    expect(container.querySelectorAll("svg rect.bar")).toHaveLength(0);
    expect(getByText("체중")).toBeInTheDocument();
  });

  it("chart layout은 chart_type이 bar일 때만 bar 차트로 렌더링한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "막대",
        source: "측정",
        layout: "chart",
        entities: [],
        aggregates: {},
        chart,
        chart_type: "bar",
      },
    ];
    const { container } = render(PageRenderer, { page: "홈", blocks, schemas });

    expect(container.querySelectorAll("svg rect.bar")).toHaveLength(2);
    expect(container.querySelectorAll("svg path.series")).toHaveLength(0);
  });

  it("chart layout은 chart 데이터가 없어도 빈 차트로 렌더링한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "빈 차트",
        source: "측정",
        layout: "chart",
        entities: [],
        aggregates: {},
      },
    ];
    const { container } = render(PageRenderer, { page: "홈", blocks, schemas });

    expect(container.querySelector("svg[aria-label='chart']")).toBeInTheDocument();
    expect(container.querySelectorAll(".legend-item")).toHaveLength(0);
  });

  it("record layout은 QuickRecordWidget에 block과 schemas를 전달해 렌더링한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "빠른 기록",
        source: "측정",
        layout: "record",
        entities: [],
        aggregates: {},
      },
    ];
    render(PageRenderer, { page: "홈", blocks, schemas });

    expect(screen.getByLabelText("지표")).toBeInTheDocument();
    expect(screen.getByLabelText("값")).toBeInTheDocument();
    expect(screen.getByLabelText("시각")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "기록" })).toBeInTheDocument();
  });

  it("profile layout은 browse 링크 없이 ProfileView로 렌더링한다", () => {
    const blocks: PageBlock[] = [
      {
        view: "내 프로필",
        source: "물건",
        layout: "profile",
        entities: [],
        aggregates: {},
        sections: [{ title: "기본", fields: ["이름"] }],
      },
    ];
    render(PageRenderer, { page: "나", blocks, schemas });

    expect(screen.queryByRole("link", { name: /내 프로필/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "프로필 시작하기" })).toBeInTheDocument();
  });
});
