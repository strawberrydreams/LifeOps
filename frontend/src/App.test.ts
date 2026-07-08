import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import App from "./App.svelte";
import * as api from "./lib/api";

vi.mock("./lib/router.svelte", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/router.svelte")>();
  return { ...actual, navigate: vi.fn() };
});

afterEach(() => vi.restoreAllMocks());

describe("App", () => {
  it("Cmd/Ctrl+K로 검색 팔레트를 연다", async () => {
    vi.spyOn(api, "getSchemas").mockResolvedValue({ types: {}, categories: [] });
    vi.spyOn(api, "search").mockResolvedValue({ query: "", results: [], total: 0, truncated: false });
    render(App);
    await screen.findByText("LifeOps"); // 사이드바 로드 대기
    await fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(await screen.findByLabelText("검색어")).toBeInTheDocument();
  });

  it("Cmd/Ctrl+K로 열린 팔레트를 다시 눌러 닫는다(입력 포커스에서 전파)", async () => {
    vi.spyOn(api, "getSchemas").mockResolvedValue({ types: {}, categories: [] });
    vi.spyOn(api, "search").mockResolvedValue({ query: "", results: [], total: 0, truncated: false });
    render(App);
    await screen.findByText("LifeOps");
    await fireEvent.keyDown(window, { key: "k", metaKey: true });
    const input = await screen.findByLabelText("검색어");
    // 포커스된 입력에서 발생 → 다이얼로그 keydown 핸들러를 거쳐 window로 전파되어야 토글 닫힘
    await fireEvent.keyDown(input, { key: "k", metaKey: true });
    await waitFor(() => expect(screen.queryByLabelText("검색어")).toBeNull());
  });
});
