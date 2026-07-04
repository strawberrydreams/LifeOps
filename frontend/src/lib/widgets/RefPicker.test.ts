import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/svelte";
import RefPicker from "./RefPicker.svelte";
import type { Entity, ResolvedField } from "../types";
import * as api from "../api";

afterEach(() => vi.restoreAllMocks());

const field: ResolvedField = { kind: "ref", required: false, target: "물건" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const entity = (id: string, name: string): Entity => ({
  id,
  type: "시계",
  data: { 이름: name },
  created_at: "",
  updated_at: "",
});

describe("RefPicker", () => {
  it("검색 입력 시 target 타입으로 listEntities 호출, 선택 시 id onchange", async () => {
    const spy = vi.spyOn(api, "listEntities").mockResolvedValue([
      { id: "w1", type: "시계", data: { 이름: "세이코 미쿠" }, created_at: "", updated_at: "" },
    ]);
    const onchange = vi.fn();
    const { getByRole, findByText } = render(RefPicker, { field, value: null, onchange });
    await fireEvent.input(getByRole("textbox"), { target: { value: "세이코" } });
    await waitFor(() => expect(spy).toHaveBeenCalledWith("물건", {}));
    const opt = await findByText("세이코 미쿠");
    await fireEvent.click(opt);
    expect(onchange).toHaveBeenCalledWith("w1");
  });

  it("늦게 도착한 이전 검색 응답은 최신 결과를 덮어쓰지 않는다", async () => {
    const first = deferred<Entity[]>();
    const second = deferred<Entity[]>();
    vi.spyOn(api, "listEntities")
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const { getByRole, findByText, queryByText } = render(RefPicker, {
      field,
      value: null,
      onchange: vi.fn(),
    });

    await fireEvent.input(getByRole("textbox"), { target: { value: "세" } });
    await fireEvent.input(getByRole("textbox"), { target: { value: "오" } });

    second.resolve([entity("w2", "오리스")]);
    await findByText("오리스");

    first.resolve([entity("w1", "세이코")]);
    await waitFor(() => expect(queryByText("세이코")).not.toBeInTheDocument());
    expect(queryByText("오리스")).toBeInTheDocument();
  });

  it("target 없으면 type 파라미터 없이 전 타입을 검색한다", async () => {
    const spy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => [],
    } as Response);
    render(RefPicker, { field: { kind: "ref", required: false }, value: null, onchange: () => {} });
    await fireEvent.input(screen.getByPlaceholderText("검색..."), { target: { value: "미" } });
    await waitFor(() => expect(spy).toHaveBeenCalled());
    expect(String(spy.mock.calls.at(-1)?.[0])).not.toContain("type=%EB");
  });

  it("최신 검색 실패 시 결과를 비우고 드롭다운을 닫는다", async () => {
    const spy = vi
      .spyOn(api, "listEntities")
      .mockResolvedValueOnce([entity("w1", "세이코")])
      .mockRejectedValueOnce(new Error("network"));
    const { getByRole, findByText, queryByText, container } = render(RefPicker, {
      field,
      value: null,
      onchange: vi.fn(),
    });

    await fireEvent.input(getByRole("textbox"), { target: { value: "세" } });
    await findByText("세이코");

    await fireEvent.input(getByRole("textbox"), { target: { value: "오" } });
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));

    await waitFor(() => expect(queryByText("세이코")).not.toBeInTheDocument());
    expect(container.querySelector(".results")).toBeNull();
  });
});
