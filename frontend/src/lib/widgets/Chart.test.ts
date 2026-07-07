import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import Chart from "./Chart.svelte";
import type { ChartSeries } from "../api";

const series: ChartSeries[] = [
  {
    name: "체중",
    points: [
      { x: "2026-07-01", y: 52.1 },
      { x: "2026-07-02", y: 52.4 },
    ],
  },
  {
    name: "수면시간",
    points: [
      { x: "2026-07-01", y: 7 },
      { x: "2026-07-02", y: 8 },
    ],
  },
];

describe("Chart", () => {
  it("line mode renders one svg path.series per series and legend text", () => {
    const { container, getByText } = render(Chart, { series, chartType: "line" });

    expect(container.querySelectorAll("svg path.series")).toHaveLength(2);
    expect(getByText("체중")).toBeInTheDocument();
    expect(getByText("수면시간")).toBeInTheDocument();
  });

  it("bar mode renders one svg rect.bar per point", () => {
    const { container } = render(Chart, { series, chartType: "bar" });

    expect(container.querySelectorAll("svg rect.bar")).toHaveLength(4);
  });

  it("line mode renders a visible marker for a one-point series", () => {
    const { container } = render(Chart, {
      series: [{ name: "체중", points: [{ x: "2026-07-01", y: 52.1 }] }],
      chartType: "line",
    });

    const point = container.querySelector("svg circle.point");
    expect(point).not.toBeNull();
    expect(Number(point?.getAttribute("r"))).toBeGreaterThan(0);
  });

  it("bar mode positions uneven series by globally ordered shared x buckets", () => {
    const unevenSeries: ChartSeries[] = [
      {
        name: "A",
        points: [
          { x: "a", y: 1 },
          { x: "c", y: 3 },
        ],
      },
      {
        name: "B",
        points: [{ x: "b", y: 2 }],
      },
    ];
    const { container } = render(Chart, { series: unevenSeries, chartType: "bar" });

    const barXs = Array.from(container.querySelectorAll("svg rect.bar"))
      .map((bar) => Number(bar.getAttribute("x")));

    expect(new Set(barXs).size).toBe(3);
    expect(barXs[0]).toBeLessThan(barXs[2]);
    expect(barXs[2]).toBeLessThan(barXs[1]);
    expect(barXs[1]).toBeGreaterThan(200);
  });
});
