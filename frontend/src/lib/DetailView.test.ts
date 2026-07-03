import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, waitFor } from "@testing-library/svelte";
import DetailView from "./DetailView.svelte";
import type { ResolvedSchema, Entity } from "./types";
import * as api from "./api";
import { ApiError } from "./api";

afterEach(() => vi.restoreAllMocks());

const schema: ResolvedSchema = { name: "물건", extends: null, fields: { 이름: { kind: "text", required: true } } };
const entity: Entity = { id: "e1", type: "물건", data: { 이름: "세이코" }, created_at: "", updated_at: "" };

describe("DetailView", () => {
  it("역링크를 표시한다", () => {
    const { getByText } = render(DetailView, {
      schema, entity, backlinks: [{ from_id: "t1", from_type: "할일", field_name: "관련물건" }],
    });
    expect(getByText(/할일/)).toBeInTheDocument();
  });

  it("삭제 409면 참조 목록을 보여준다", async () => {
    vi.spyOn(api, "deleteEntity").mockRejectedValue(
      new ApiError(409, "delete_blocked", "1곳에서 참조 중", { referrers: [{ from_id: "t1", from_type: "할일", field_name: "관련물건" }] })
    );
    const { getByText, findByText } = render(DetailView, { schema, entity, backlinks: [] });
    await fireEvent.click(getByText("삭제"));
    expect(await findByText(/참조 중/)).toBeInTheDocument();
  });

  it("저장_실패시_에러메시지_표시", async () => {
    vi.spyOn(api, "updateEntity").mockRejectedValue(
      new ApiError(400, "validation", "검증 실패", { fields: [{ field: "이름", message: "필수 필드" }] })
    );
    const { getByText, findByText } = render(DetailView, { schema, entity, backlinks: [] });
    await fireEvent.click(getByText("저장"));
    expect(await findByText(/검증 실패|필수/)).toBeInTheDocument();
  });
});
