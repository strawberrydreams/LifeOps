import { render, fireEvent, waitFor } from "@testing-library/svelte";
import { describe, it, expect, vi, afterEach } from "vitest";
import QuickRecordWidget from "./QuickRecordWidget.svelte";
import * as api from "../api";

const schemas = {
  측정: {
    name: "측정",
    fields: {
      지표: { kind: "enum", required: true, options: ["체중", "수면시간"] },
      값: { kind: "number", required: true },
      시각: { kind: "date", required: true },
    },
  },
};

const switchedSchemas = {
  ...schemas,
  기분: {
    name: "기분",
    fields: {
      지표: { kind: "enum", required: true, options: ["에너지"] },
      값: { kind: "number", required: true },
      시각: { kind: "date", required: true },
    },
  },
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("QuickRecordWidget", () => {
  it("입력 후 저장하면 createEntity를 호출한다", async () => {
    const created = { id: "m1", type: "측정", data: {}, created_at: "", updated_at: "" };
    const spy = vi.spyOn(api, "createEntity").mockResolvedValue(created as never);
    const block = { view: "빠른 기록", source: "측정", layout: "record", entities: [], aggregates: {} };
    const { getByLabelText, getByRole } = render(QuickRecordWidget, {
      props: { block: block as never, schemas: schemas as never },
    });

    await fireEvent.change(getByLabelText("지표"), { target: { value: "체중" } });
    await fireEvent.input(getByLabelText("값"), { target: { value: "81.5" } });
    await fireEvent.click(getByRole("button", { name: "기록" }));

    expect(spy).toHaveBeenCalledOnce();
    const [type, data] = spy.mock.calls[0];
    expect(type).toBe("측정");
    expect(data["지표"]).toBe("체중");
    expect(data["값"]).toBe(81.5);
    expect(typeof data["시각"]).toBe("string");
  });

  it("source schema가 바뀌면 enum 값을 새 option으로 재설정한다", async () => {
    const created = { id: "m1", type: "기분", data: {}, created_at: "", updated_at: "" };
    const spy = vi.spyOn(api, "createEntity").mockResolvedValue(created as never);
    const block = { view: "빠른 기록", source: "측정", layout: "record", entities: [], aggregates: {} };
    const { getByLabelText, getByRole, rerender } = render(QuickRecordWidget, {
      props: { block: block as never, schemas: switchedSchemas as never },
    });

    await fireEvent.change(getByLabelText("지표"), { target: { value: "수면시간" } });
    await rerender({
      block: { ...block, source: "기분" } as never,
      schemas: switchedSchemas as never,
    });
    await fireEvent.input(getByLabelText("값"), { target: { value: "7" } });
    await fireEvent.click(getByRole("button", { name: "기록" }));

    expect(spy).toHaveBeenCalledOnce();
    const [type, data] = spy.mock.calls[0];
    expect(type).toBe("기분");
    expect(data["지표"]).toBe("에너지");
  });

  it("저장 성공 후 날짜 기본값을 오늘로 다시 설정한다", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-07T12:00:00"));
    const created = { id: "m1", type: "측정", data: {}, created_at: "", updated_at: "" };
    const spy = vi.spyOn(api, "createEntity").mockResolvedValue(created as never);
    const block = { view: "빠른 기록", source: "측정", layout: "record", entities: [], aggregates: {} };
    const { getByLabelText, getByRole } = render(QuickRecordWidget, {
      props: { block: block as never, schemas: schemas as never },
    });

    vi.setSystemTime(new Date("2026-07-08T12:00:00"));
    await fireEvent.input(getByLabelText("값"), { target: { value: "81.5" } });
    await fireEvent.click(getByRole("button", { name: "기록" }));
    await waitFor(() => expect(spy).toHaveBeenCalledOnce());
    await fireEvent.input(getByLabelText("값"), { target: { value: "81.7" } });
    await fireEvent.click(getByRole("button", { name: "기록" }));

    expect(spy).toHaveBeenCalledTimes(2);
    expect(spy.mock.calls[0][1]["시각"]).toBe("2026-07-07");
    expect(spy.mock.calls[1][1]["시각"]).toBe("2026-07-08");
  });
});
