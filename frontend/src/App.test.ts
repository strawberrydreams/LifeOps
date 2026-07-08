import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
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
});
