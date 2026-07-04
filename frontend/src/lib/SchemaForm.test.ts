import { describe, it, expect, vi } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/svelte";
import { tick } from "svelte";
import SchemaForm from "./SchemaForm.svelte";
import { ApiError } from "./api";
import type { ResolvedSchema } from "./types";

const schema: ResolvedSchema = {
  name: "시계",
  extends: null,
  fields: {
    이름: { kind: "text", required: true },
    상태: { kind: "enum", required: false, options: ["위시", "보유"] },
  },
};

describe("SchemaForm", () => {
  it("필드 순서대로 위젯을 렌더하고 제출 시 데이터를 넘긴다", async () => {
    const onsubmit = vi.fn().mockResolvedValue(undefined);
    const { container, getByRole, getByText } = render(SchemaForm, { schema, onsubmit });
    const labels = [...container.querySelectorAll(".field .label")].map((el) => el.textContent);
    expect(labels).toEqual(["이름*", "상태"]);
    await fireEvent.input(getByRole("textbox"), { target: { value: "세이코 미쿠" } });
    await fireEvent.click(getByText("저장"));
    await waitFor(() => expect(onsubmit).toHaveBeenCalledWith({ 이름: "세이코 미쿠" }));
  });

  it("initial 값을 렌더하고 빈 문자열로 지운 필드는 제출 데이터에서 제외한다", async () => {
    const onsubmit = vi.fn().mockResolvedValue(undefined);
    const { getByRole, getByText } = render(SchemaForm, {
      schema,
      initial: { 이름: "세이코 미쿠", 상태: "보유" },
      onsubmit,
    });

    await fireEvent.input(getByRole("textbox", { name: /이름/ }), { target: { value: "" } });
    await fireEvent.click(getByText("저장"));

    await waitFor(() => expect(onsubmit).toHaveBeenCalledWith({ 상태: "보유" }));
  });

  it("편집 중 새 initial 객체 identity로 rerender되어도 사용자 입력을 덮어쓰지 않는다", async () => {
    const onsubmit = vi.fn().mockResolvedValue(undefined);
    const { getByRole, rerender } = render(SchemaForm, {
      schema,
      initial: { 이름: "세이코" },
      onsubmit,
    });

    const input = getByRole("textbox", { name: /이름/ });
    await fireEvent.input(input, { target: { value: "오리스" } });
    await rerender({ schema, initial: { 이름: "세이코" }, onsubmit });
    await tick();

    expect(getByRole("textbox", { name: /이름/ })).toHaveValue("오리스");
  });

  it("400 필드 에러를 필드 옆에 표시한다", async () => {
    const onsubmit = vi
      .fn()
      .mockRejectedValue(new ApiError(400, "validation", "검증 실패", { fields: [{ field: "이름", message: "필수 필드" }] }));
    const { container, getByRole, getByText, findByText } = render(SchemaForm, { schema, onsubmit });
    await fireEvent.click(getByText("저장"));
    expect(await findByText("필수 필드")).toBeInTheDocument();
    const nameField = [...container.querySelectorAll(".field")].find((el) => el.querySelector(".label")?.textContent?.includes("이름"));
    expect(nameField).toContainElement(getByText("필수 필드"));
    expect(getByRole("textbox", { name: /이름/ })).toHaveAccessibleDescription("필수 필드");
  });
});
