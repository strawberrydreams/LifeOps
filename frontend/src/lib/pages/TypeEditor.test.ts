import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import TypeEditor from "./TypeEditor.svelte";
import * as api from "../api";

const schemas = {
  물건: { name: "물건", category: "컬렉션", fields: { 이름: { kind: "text", required: true } } },
};
const categories = [{ name: "컬렉션", icon: "📦" }];

afterEach(() => vi.restoreAllMocks());

describe("TypeEditor", () => {
  it("생성 모드 저장은 createSchema를 부른다", async () => {
    const createSpy = vi.spyOn(api, "createSchema").mockResolvedValue({ ok: true });
    vi.spyOn(api, "getSchemas").mockResolvedValue({ types: {}, categories });
    render(TypeEditor, { schemas, categories, mode: "new", onreloaded: () => {} });

    await fireEvent.input(await screen.findByLabelText("타입명"), { target: { value: "북마크" } });
    await fireEvent.click(screen.getByRole("button", { name: "+ 필드" }));
    await fireEvent.click(screen.getByRole("button", { name: "저장" }));

    expect(createSpy).toHaveBeenCalled();
    const body = createSpy.mock.calls.at(-1)![0];
    expect(body.type).toBe("북마크");
  });

  it("수정 모드 저장은 dry-run 후 updateSchema를 부른다", async () => {
    vi.spyOn(api, "getSchemaRaw").mockResolvedValue({
      type: "물건",
      category: "컬렉션",
      extends: null,
      behaviors: null,
      field_order: null,
      fields: { 이름: { kind: "text", required: true } },
      inherited: {},
    });
    const updateSpy = vi.spyOn(api, "updateSchema");
    updateSpy
      .mockResolvedValueOnce({ affected_entities: 0, warnings: [] })
      .mockResolvedValueOnce({ ok: true });
    vi.spyOn(api, "getSchemas").mockResolvedValue({ types: {}, categories });

    render(TypeEditor, { schemas, categories, mode: "edit", type: "물건", onreloaded: () => {} });
    await screen.findByDisplayValue("물건");
    await fireEvent.click(screen.getByRole("button", { name: "저장" }));

    expect(updateSpy).toHaveBeenCalledTimes(2);
    expect(updateSpy.mock.calls[0][2]).toEqual({ dryRun: true });
    expect(updateSpy.mock.calls[1][2]).toBeUndefined();
  });
});
