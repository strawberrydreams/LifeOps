import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import FieldRowEditor from "./FieldRowEditor.svelte";

const schemas = {
  물건: { name: "물건", fields: {} },
  장소: { name: "장소", fields: {} },
};

function baseRow(over: Record<string, unknown> = {}) {
  return {
    localId: "r1",
    originalName: null,
    name: "가격",
    kind: "money",
    required: false,
    options: [] as string[],
    target: null as string | null,
    unit: null as string | null,
    ...over,
  };
}

describe("FieldRowEditor", () => {
  it("kind를 enum으로 바꾸면 options 입력이 나타난다", async () => {
    const onchange = vi.fn();
    render(FieldRowEditor, {
      row: baseRow(),
      schemas,
      onchange,
      onremove: () => {},
      onmove: () => {},
    });

    expect(screen.queryByPlaceholderText(/옵션/)).toBeNull();
    const kindSelect = screen.getByLabelText("kind") as HTMLSelectElement;
    await fireEvent.change(kindSelect, { target: { value: "enum" } });
    expect(onchange).toHaveBeenCalled();
    const last = onchange.mock.calls.at(-1)![0];
    expect(last.kind).toBe("enum");
  });

  it("ref kind면 target select에 다른 타입이 나온다", () => {
    const onchange = vi.fn();
    render(FieldRowEditor, {
      row: baseRow({ name: "곳", kind: "ref", target: null }),
      schemas,
      onchange,
      onremove: () => {},
      onmove: () => {},
    });
    const target = screen.getByLabelText("target") as HTMLSelectElement;
    expect(target).toBeInTheDocument();
    const opts = Array.from(target.options).map((o) => o.value);
    expect(opts).toContain("장소");
    expect(opts).toContain("물건");
  });

  it("삭제 버튼은 onremove를 부른다", async () => {
    const onremove = vi.fn();
    render(FieldRowEditor, {
      row: baseRow(),
      schemas,
      onchange: () => {},
      onremove,
      onmove: () => {},
    });
    await fireEvent.click(screen.getByRole("button", { name: "삭제" }));
    expect(onremove).toHaveBeenCalled();
  });
});
