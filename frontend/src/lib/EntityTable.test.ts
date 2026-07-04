import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/svelte";
import EntityTable from "./EntityTable.svelte";
import type { ResolvedSchema, Entity } from "./types";
import * as api from "./api";
import { ApiError } from "./api";

afterEach(() => vi.restoreAllMocks());

const schema: ResolvedSchema = {
  name: "물건", extends: null,
  fields: { 이름: { kind: "text", required: true }, 상태: { kind: "enum", required: false, options: ["위시", "보유"] } },
};
const entities: Entity[] = [
  { id: "e1", type: "물건", data: { 이름: "A", 상태: "위시" }, created_at: "", updated_at: "" },
];

describe("EntityTable 인라인 편집", () => {
  it("셀 클릭 → 위젯 전환 → 값 변경·Enter → updateEntity 호출·낙관적 갱신", async () => {
    const spy = vi.spyOn(api, "updateEntity").mockResolvedValue({ ...entities[0], data: { 이름: "A", 상태: "보유" } });
    const { getByText, getByRole } = render(EntityTable, { schema, entities });
    await fireEvent.click(getByText("위시")); // 상태 셀
    const select = getByRole("combobox");
    await fireEvent.change(select, { target: { value: "보유" } });
    await fireEvent.keyDown(select, { key: "Enter" });
    await waitFor(() => expect(spy).toHaveBeenCalledWith("e1", { 상태: "보유" }));
    expect(await (async () => getByText("보유"))()).toBeInTheDocument();
  });

  it("편집중_위젯_클릭은_행이동을_유발하지_않는다", async () => {
    const rowClickSpy = vi.fn();
    const { getByText, getByRole } = render(EntityTable, { schema, entities, onrowclick: rowClickSpy });
    await fireEvent.click(getByText("위시")); // 상태 셀 → 편집 모드 진입
    const select = getByRole("combobox");
    await fireEvent.click(select); // 위젯 내부 클릭
    expect(rowClickSpy).not.toHaveBeenCalled();
  });

  it("표시 모드 셀에서 Enter 키 → 편집 모드 진입(위젯 렌더)", async () => {
    const { getByText, getByRole } = render(EntityTable, { schema, entities });
    const cell = getByText("위시");
    await fireEvent.keyDown(cell, { key: "Enter" });
    expect(getByRole("combobox")).toBeInTheDocument();
  });

  it("인라인_저장_실패시_편집유지_에러표시", async () => {
    vi.spyOn(api, "updateEntity").mockRejectedValue(
      new ApiError(400, "validation", "검증 실패", { fields: [{ field: "상태", message: "필수 필드" }] })
    );
    const { getByText, getByRole, findByText } = render(EntityTable, { schema, entities });
    await fireEvent.click(getByText("위시")); // 상태 셀 → 편집 모드 진입
    const select = getByRole("combobox");
    await fireEvent.change(select, { target: { value: "보유" } });
    await fireEvent.keyDown(select, { key: "Enter" });

    expect(await findByText(/검증 실패/)).toBeInTheDocument();
    // 편집 모드가 유지되어야 함 — 위젯이 여전히 존재해야 한다.
    expect(getByRole("combobox")).toBeInTheDocument();
  });

  it("포커스아웃시_커밋", async () => {
    const spy = vi.spyOn(api, "updateEntity").mockResolvedValue({ ...entities[0], data: { 이름: "A", 상태: "보유" } });
    const { getByText, getByRole } = render(EntityTable, { schema, entities });
    await fireEvent.click(getByText("위시")); // 상태 셀 → 편집 모드 진입
    const select = getByRole("combobox");
    await fireEvent.change(select, { target: { value: "보유" } });
    await fireEvent.focusOut(select, { relatedTarget: document.body });
    await waitFor(() => expect(spy).toHaveBeenCalledWith("e1", { 상태: "보유" }));
  });
});
