import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import Widget from "./Widget.svelte";
import type { ResolvedField } from "../types";

const f = (kind: string, extra: Partial<ResolvedField> = {}): ResolvedField => ({
  kind,
  required: false,
  ...extra,
});

describe("money/list 위젯", () => {
  it("money → 금액과 통화 입력, 부분 변경 시 객체 병합 onchange", async () => {
    const onchange = vi.fn();
    const { getByPlaceholderText } = render(Widget, {
      field: f("money"),
      value: { amount: 1000, currency: "KRW" },
      onchange,
    });
    expect(getByPlaceholderText("통화")).toBeInTheDocument();
    await fireEvent.input(getByPlaceholderText("금액"), { target: { value: "2000" } });
    expect(onchange).toHaveBeenCalledWith({ amount: 2000, currency: "KRW" });
  });

  it("list<text> → 항목 추가 버튼이 배열에 빈 문자열을 더한다", async () => {
    const onchange = vi.fn();
    const { getByText } = render(Widget, { field: f("list<text>"), value: ["a"], onchange });
    await fireEvent.click(getByText("+ 추가"));
    expect(onchange).toHaveBeenCalledWith(["a", ""]);
  });
});
