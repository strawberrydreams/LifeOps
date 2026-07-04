import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/svelte";
import { tick } from "svelte";
import NoteEditor from "./NoteEditor.svelte";

describe("NoteEditor", () => {
  it("초기 HTML을 렌더하고 에디터 컨테이너가 존재한다", () => {
    const { container } = render(NoteEditor, { value: "<p>안녕</p>", onchange: vi.fn() });
    expect(container.querySelector(".note-editor")).not.toBeNull();
    expect(container.textContent).toContain("안녕");
    // Tiptap이 마운트되면 contenteditable 요소가 생긴다
    expect(container.querySelector('[contenteditable="true"]')).not.toBeNull();
  });

  it("부모가 value를 바꾸면 마운트된 에디터 내용을 동기화한다", async () => {
    const onchange = vi.fn();
    const { container, rerender } = render(NoteEditor, { value: "<p>처음</p>", onchange });

    await rerender({ value: "<p>변경</p>", onchange });
    await tick();

    expect(container.textContent).toContain("변경");
    expect(container.textContent).not.toContain("처음");
    expect(onchange).not.toHaveBeenCalled();
  });
});
