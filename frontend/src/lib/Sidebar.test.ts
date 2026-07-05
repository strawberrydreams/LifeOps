import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/svelte";
import Sidebar from "./Sidebar.svelte";

const schemas = {
  노트: { name: "노트", category: "메모", fields: {} },
  물건: { name: "물건", category: "컬렉션", fields: {} },
  시계: { name: "시계", category: "컬렉션", extends: "물건", fields: {} },
  북마크: { name: "북마크", fields: {} },
};
const categories = [
  { name: "메모", icon: "📝" },
  { name: "컬렉션", icon: "📦" },
];

describe("Sidebar", () => {
  it("카테고리 헤더 아래에 소속 타입이 나오고 미지정은 기타로 간다", () => {
    render(Sidebar, { schemas, categories, onreloaded: () => {} });
    expect(screen.getByText(/메모/)).toBeInTheDocument();
    expect(screen.getByText(/컬렉션/)).toBeInTheDocument();
    expect(screen.getByText("노트")).toBeInTheDocument();
    expect(screen.getByText("시계")).toBeInTheDocument();
    expect(screen.getByText("북마크")).toBeInTheDocument();
    expect(screen.getByText(/기타/)).toBeInTheDocument();
    expect(screen.getByText(/홈/)).toBeInTheDocument();
  });

  it("카테고리마다 새 타입 버튼이 있다", () => {
    render(Sidebar, { schemas, categories, onreloaded: () => {} });
    expect(screen.getAllByTitle("새 타입").length).toBeGreaterThan(0);
  });
});
