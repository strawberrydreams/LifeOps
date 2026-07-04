import { afterEach, describe, it, expect, vi } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/svelte";
import Widget from "./Widget.svelte";
import { parseKind } from "../kind";
import type { ResolvedField } from "../types";
import * as api from "../api";

afterEach(() => vi.restoreAllMocks());

const f = (kind: string, extra: Partial<ResolvedField> = {}): ResolvedField => ({
  kind,
  required: false,
  ...extra,
});

describe("Widget 매핑", () => {
  it("text → 텍스트 input, 입력 시 onchange", async () => {
    const onchange = vi.fn();
    const { getByRole } = render(Widget, { field: f("text"), value: "", onchange });
    const input = getByRole("textbox");
    await fireEvent.input(input, { target: { value: "세이코" } });
    expect(onchange).toHaveBeenCalledWith("세이코");
  });

  it("enum → select, options 렌더 및 선택 시 onchange", async () => {
    const onchange = vi.fn();
    const { getByRole, getAllByRole } = render(Widget, {
      field: f("enum", { options: ["위시", "보유"] }),
      value: "위시",
      onchange,
    });
    const select = getByRole("combobox");
    expect(select).toBeInTheDocument();
    // 빈 옵션 + 2개
    expect(getAllByRole("option").length).toBe(3);
    await fireEvent.change(select, { target: { value: "보유" } });
    expect(onchange).toHaveBeenCalledWith("보유");
  });

  it("bool → checkbox, 토글 시 boolean onchange", async () => {
    const onchange = vi.fn();
    const { getByRole } = render(Widget, { field: f("bool"), value: false, onchange });
    await fireEvent.click(getByRole("checkbox"));
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it("number → number input, 숫자 onchange", async () => {
    const onchange = vi.fn();
    const { getByRole } = render(Widget, {
      field: f("number", { unit: "개" }),
      value: null,
      onchange,
    });
    const input = getByRole("spinbutton");
    await fireEvent.input(input, { target: { value: "3" } });
    expect(onchange).toHaveBeenCalledWith(3);
  });

  it("date → date input, 입력 시 onchange", async () => {
    const onchange = vi.fn();
    const { container } = render(Widget, {
      field: f("date"),
      value: "2026-07-03",
      onchange,
    });
    const input = container.querySelector('input[type="date"]');
    expect(input).not.toBeNull();
    await fireEvent.input(input!, { target: { value: "2026-07-04" } });
    expect(onchange).toHaveBeenCalledWith("2026-07-04");
  });

  it("list<text> → 반복 위젯으로 base text 항목과 추가 버튼 렌더", () => {
    const { container, getByText } = render(Widget, {
      field: f("list<text>"),
      value: ["세이코"],
      onchange: vi.fn(),
    });
    expect(container.querySelector(".list")).not.toBeNull();
    expect(container.querySelector('input[type="text"]')).not.toBeNull();
    expect(getByText("+ 추가")).toBeInTheDocument();
  });

  it("ref → RefPicker로 라우팅하고 선택 시 id onchange", async () => {
    const spy = vi.spyOn(api, "listEntities").mockResolvedValue([
      { id: "w1", type: "시계", data: { 이름: "세이코 미쿠" }, created_at: "", updated_at: "" },
    ]);
    const onchange = vi.fn();
    const { getByRole, findByText } = render(Widget, {
      field: f("ref", { target: "물건" }),
      value: null,
      onchange,
    });

    await fireEvent.input(getByRole("textbox"), { target: { value: "세이코" } });
    await waitFor(() => expect(spy).toHaveBeenCalledWith("물건", {}));
    await fireEvent.click(await findByText("세이코 미쿠"));

    expect(onchange).toHaveBeenCalledWith("w1");
  });

  it("parseKind는 list kind를 base와 list flag로 분리", () => {
    expect(parseKind("list<ref>")).toEqual({ base: "ref", list: true });
  });
});
