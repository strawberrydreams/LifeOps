import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import Sidebar from "./Sidebar.svelte";
import { navigate } from "./router.svelte";

vi.mock("./router.svelte", () => ({ navigate: vi.fn() }));

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

afterEach(() => vi.clearAllMocks());

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

  it("싱글턴 타입은 /pages로 이동하고 추가 버튼이 없다", async () => {
    render(Sidebar, {
      schemas: { 프로필: { name: "프로필", category: "나", singleton: true, fields: {} } },
      categories: [{ name: "나", icon: "🧑" }],
      onreloaded: () => {},
    });

    expect(screen.queryByTitle("추가")).not.toBeInTheDocument();
    await fireEvent.click(screen.getByText("프로필"));

    expect(navigate).toHaveBeenCalledWith("/pages/%ED%94%84%EB%A1%9C%ED%95%84");
  });

  it("검색 버튼이 onsearch를 호출한다", async () => {
    const onsearch = vi.fn();
    render(Sidebar, {
      schemas: { 시계: { name: "시계", fields: {} } } as any,
      categories: [{ name: "컬렉션" }],
      onreloaded: vi.fn(),
      onsearch,
    });
    await fireEvent.click(screen.getByRole("button", { name: "🔍 검색" }));
    expect(onsearch).toHaveBeenCalled();
  });

  it("페이지 섹션이 페이지 목록과 새 페이지 버튼을 보인다", async () => {
    render(Sidebar, { schemas, categories, pages: ["건강", "대시보드"], onreloaded: () => {} });
    expect(screen.getByText("건강")).toBeInTheDocument();
    expect(screen.getByText("대시보드")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "+ 새 페이지" }));
    expect(navigate).toHaveBeenCalledWith("/pages/new");
  });
});
