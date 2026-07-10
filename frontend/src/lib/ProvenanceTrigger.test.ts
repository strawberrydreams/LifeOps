import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import ProvenanceTrigger from "./ProvenanceTrigger.svelte";
import type { Entity } from "./types";
import * as api from "./api";

function entity(data: Record<string, unknown>): Entity {
  return { id: "e1", type: "측정", data, created_at: "", updated_at: "2026-07-08T00:00:00Z" };
}

describe("ProvenanceTrigger", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("닫힌 상태에선 팝오버가 없다", () => {
    render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값" });
    expect(screen.queryByLabelText("출처")).toBeNull();
  });

  it("열면 기본값(source=manual)을 보여준다", async () => {
    render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값" });
    await fireEvent.click(screen.getByLabelText("출처 정보"));
    const src = screen.getByLabelText("출처") as HTMLSelectElement;
    expect(src.value).toBe("manual");
  });

  it("저장하면 $meta 델타로 PATCH한다", async () => {
    const spy = vi.spyOn(api, "updateEntity").mockResolvedValue(entity({ 값: 72 }) as never);
    render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값" });
    await fireEvent.click(screen.getByLabelText("출처 정보"));
    await fireEvent.change(screen.getByLabelText("민감도"), { target: { value: "sensitive" } });
    await fireEvent.click(screen.getByText("저장"));
    expect(spy).toHaveBeenCalledWith("e1", { $meta: { 값: { source: "manual", sensitivity: "sensitive", confidence: null } } });
  });

  it("열린 팝오버 안의 상호작용은 부모 클릭 핸들러로 버블링하지 않는다", async () => {
    vi.spyOn(api, "updateEntity").mockResolvedValue(entity({ 값: 72 }) as never);
    const parentClick = vi.fn();
    const { container } = render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값" });
    container.addEventListener("click", parentClick);
    container.addEventListener("pointerdown", parentClick);

    await fireEvent.click(screen.getByLabelText("출처 정보"));
    expect(parentClick).not.toHaveBeenCalled();

    await fireEvent.pointerDown(screen.getByRole("dialog"));
    await fireEvent.click(screen.getByLabelText("민감도"));
    await fireEvent.click(screen.getByText("저장"));

    expect(parentClick).not.toHaveBeenCalled();
  });

  it("기존 신뢰도를 빈 값으로 저장하면 confidence null을 PATCH한다", async () => {
    const spy = vi.spyOn(api, "updateEntity").mockResolvedValue(entity({ 값: 72 }) as never);
    render(ProvenanceTrigger, {
      entity: entity({ 값: 72, $meta: { 값: { source: "manual", confidence: 0.7, sensitivity: "normal" } } }),
      fieldName: "값",
    });

    await fireEvent.click(screen.getByLabelText("출처 정보"));
    await fireEvent.input(screen.getByLabelText("신뢰도"), { target: { value: "" } });
    await fireEvent.click(screen.getByText("저장"));

    expect(spy).toHaveBeenCalledWith("e1", { $meta: { 값: { source: "manual", sensitivity: "normal", confidence: null } } });
  });

  it("범위를 벗어난 신뢰도는 저장하지 않고 검증 메시지를 보여준다", async () => {
    const spy = vi.spyOn(api, "updateEntity").mockResolvedValue(entity({ 값: 72 }) as never);
    render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값" });

    await fireEvent.click(screen.getByLabelText("출처 정보"));
    await fireEvent.input(screen.getByLabelText("신뢰도"), { target: { value: "1.5" } });
    await fireEvent.click(screen.getByText("저장"));

    expect(screen.getByText("신뢰도는 0과 1 사이여야 합니다")).toBeTruthy();
    expect(spy).not.toHaveBeenCalled();
  });

  it("저장 후 onupdate를 호출하고 다시 열면 반환된 메타데이터를 반영한다", async () => {
    const updated = entity({
      값: 72,
      $meta: { 값: { source: "imported", confidence: 0.4, sensitivity: "sensitive" } },
    });
    vi.spyOn(api, "updateEntity").mockResolvedValue(updated as never);
    const onupdate = vi.fn();
    render(ProvenanceTrigger, { entity: entity({ 값: 72 }), fieldName: "값", onupdate });

    await fireEvent.click(screen.getByLabelText("출처 정보"));
    await fireEvent.change(screen.getByLabelText("출처"), { target: { value: "imported" } });
    await fireEvent.input(screen.getByLabelText("신뢰도"), { target: { value: "0.4" } });
    await fireEvent.change(screen.getByLabelText("민감도"), { target: { value: "sensitive" } });
    await fireEvent.click(screen.getByText("저장"));
    await fireEvent.click(screen.getByLabelText("출처 정보"));

    expect(onupdate).toHaveBeenCalledWith(updated);
    expect((screen.getByLabelText("출처") as HTMLSelectElement).value).toBe("imported");
    expect((screen.getByLabelText("신뢰도") as HTMLInputElement).value).toBe("0.4");
    expect((screen.getByLabelText("민감도") as HTMLSelectElement).value).toBe("sensitive");
  });
});
