import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import ProfileView from "./ProfileView.svelte";
import type { PageBlock } from "../api";
import type { Entity, SchemaMap } from "../types";
import * as api from "../api";
import { navigate } from "../router.svelte";

vi.mock("../router.svelte", () => ({ navigate: vi.fn() }));

const schemas: SchemaMap = {
  프로필: {
    name: "프로필",
    singleton: true,
    fields: {
      이름: { kind: "text", required: true },
      상태: { kind: "enum", required: false, options: ["활동", "휴식"] },
      소개: { kind: "richtext", required: false },
    },
  },
};

function entity(data: Record<string, unknown>): Entity {
  return { id: "p1", type: "프로필", data, created_at: "", updated_at: "" };
}

function block(entities: Entity[]): PageBlock {
  return {
    view: "프로필",
    source: "프로필",
    layout: "profile",
    entities,
    aggregates: {},
    sections: [{ title: "기본", fields: ["이름", "상태"] }],
  };
}

afterEach(() => vi.restoreAllMocks());

describe("ProfileView", () => {
  it("프로필 엔티티가 없으면 생성 진입 버튼을 보여준다", async () => {
    render(ProfileView, { block: block([]), schemas });

    await fireEvent.click(screen.getByRole("button", { name: "프로필 시작하기" }));

    expect(navigate).toHaveBeenCalledWith("/new/%ED%94%84%EB%A1%9C%ED%95%84");
  });

  it("섹션 필드를 표시하고 저장 시 수정된 값을 PATCH한다", async () => {
    vi.spyOn(api, "updateEntity").mockResolvedValue(entity({ 이름: "미쿠", 상태: "활동" }));

    render(ProfileView, { block: block([entity({ 이름: "하츠네", 상태: "휴식" })]), schemas });

    expect(screen.getByText("기본")).toBeInTheDocument();
    const nameInput = screen.getByRole("textbox", { name: "이름" });
    expect(nameInput).toHaveValue("하츠네");
    expect(screen.queryByRole("textbox", { name: /출처 정보/ })).not.toBeInTheDocument();

    await fireEvent.input(nameInput, { target: { value: "미쿠" } });
    await fireEvent.click(screen.getByRole("button", { name: "저장" }));

    expect(api.updateEntity).toHaveBeenCalledWith("p1", expect.objectContaining({ 이름: "미쿠" }));
  });

  it("각 프로필 필드에 프로비넌스 트리거를 렌더한다", () => {
    render(ProfileView, { block: block([entity({ 이름: "미쿠", 상태: "활동" })]), schemas });

    expect(screen.getAllByLabelText("출처 정보")).toHaveLength(2);
  });

  it("여러 프로필 뷰를 함께 렌더해도 필드 라벨 id가 중복되지 않는다", () => {
    render(ProfileView, { block: block([entity({ 이름: "미쿠", 상태: "활동" })]), schemas });
    render(ProfileView, { block: block([entity({ 이름: "루카", 상태: "휴식" })]), schemas });

    const ids = Array.from(document.querySelectorAll<HTMLElement>("[id^='profile-field-label-']")).map((el) => el.id);

    expect(ids).toHaveLength(4);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
