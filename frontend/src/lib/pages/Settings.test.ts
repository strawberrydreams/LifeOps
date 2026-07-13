import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  desktop: false,
  getSystemInfo: vi.fn(),
  getAutostart: vi.fn(),
  setAutostart: vi.fn(),
  openDataDir: vi.fn(),
  importFromDir: vi.fn(),
}));

vi.mock("../api", () => ({ getSystemInfo: mocks.getSystemInfo }));
vi.mock("../tauri", () => ({
  isDesktop: () => mocks.desktop,
  getAutostart: mocks.getAutostart,
  setAutostart: mocks.setAutostart,
  openDataDir: mocks.openDataDir,
  importFromDir: mocks.importFromDir,
}));

import Settings from "./Settings.svelte";

const info = {
  data_dir: "/Users/me/Library/Application Support/LifeOps",
  port: 3000,
  lan_addrs: ["http://192.168.0.2:3000"],
};

afterEach(() => {
  cleanup();
  mocks.desktop = false;
  vi.clearAllMocks();
});

describe("Settings", () => {
  it("데이터 경로·LAN 주소를 렌더하고 브라우저에서는 desktop 기능을 완전히 숨긴다", async () => {
    mocks.getSystemInfo.mockResolvedValue(info);

    render(Settings);

    expect(await screen.findByText(info.lan_addrs[0])).toBeInTheDocument();
    expect(screen.getByText(info.data_dir)).toBeInTheDocument();
    expect(screen.queryByText("로그인 시 자동 시작")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "폴더 열기" })).not.toBeInTheDocument();
    expect(screen.queryByText("데이터 가져오기")).not.toBeInTheDocument();
    expect(mocks.getAutostart).not.toHaveBeenCalled();
    expect(mocks.openDataDir).not.toHaveBeenCalled();
  });

  it("데스크탑 가져오기는 경로를 전달하고 재시작 적용을 안내한다", async () => {
    mocks.desktop = true;
    mocks.getSystemInfo.mockResolvedValue(info);
    mocks.getAutostart.mockResolvedValue(false);
    mocks.importFromDir.mockResolvedValue(undefined);

    render(Settings);
    const input = await screen.findByLabelText("데이터 가져오기");
    await fireEvent.input(input, { target: { value: " /old/lifeops " } });
    await fireEvent.click(screen.getByRole("button", { name: "가져오기" }));

    expect(mocks.importFromDir).toHaveBeenCalledWith("/old/lifeops");
    expect(await screen.findByRole("status")).toHaveTextContent("앱을 재시작하면 적용됩니다");
  });

  it("데스크탑 가져오기 실패를 표시하고 재시도 경로를 보존한다", async () => {
    mocks.desktop = true;
    mocks.getSystemInfo.mockResolvedValue(info);
    mocks.getAutostart.mockResolvedValue(false);
    mocks.importFromDir.mockRejectedValue(new Error("unsafe"));

    render(Settings);
    const input = await screen.findByLabelText("데이터 가져오기");
    await fireEvent.input(input, { target: { value: "/old/lifeops" } });
    await fireEvent.click(screen.getByRole("button", { name: "가져오기" }));

    expect(await screen.findByText(/데이터 가져오기를 준비하지 못했습니다: Error: unsafe/)).toBeInTheDocument();
    expect(input).toHaveValue("/old/lifeops");
  });

  it("데스크탑에서는 데이터 폴더를 열 수 있다", async () => {
    mocks.desktop = true;
    mocks.getSystemInfo.mockResolvedValue(info);
    mocks.getAutostart.mockResolvedValue(false);
    mocks.openDataDir.mockResolvedValue(undefined);

    render(Settings);
    await fireEvent.click(await screen.findByRole("button", { name: "폴더 열기" }));

    expect(mocks.openDataDir).toHaveBeenCalledOnce();
  });

  it("자동시작 토글 실패 시 이전 상태로 롤백하고 오류를 표시한다", async () => {
    mocks.desktop = true;
    mocks.getSystemInfo.mockResolvedValue(info);
    mocks.getAutostart.mockResolvedValue(true);
    mocks.setAutostart.mockRejectedValue(new Error("권한 거부"));

    render(Settings);
    const checkbox = await screen.findByRole("checkbox", { name: "로그인 시 자동 시작 사용" });
    await waitFor(() => expect(checkbox).toBeChecked());
    await fireEvent.click(checkbox);

    await waitFor(() => expect(checkbox).toBeChecked());
    expect(await screen.findByRole("alert")).toHaveTextContent("자동 시작 설정을 변경하지 못했습니다.");
    expect(mocks.setAutostart).toHaveBeenCalledWith(false);
  });

  it("시스템 정보와 자동시작 조회 실패를 안전하게 표시한다", async () => {
    mocks.desktop = true;
    mocks.getSystemInfo.mockRejectedValue(new Error("offline"));
    mocks.getAutostart.mockRejectedValue(new Error("unsupported"));

    render(Settings);

    expect(await screen.findByText("시스템 정보를 불러오지 못했습니다.")).toBeInTheDocument();
    expect(await screen.findByText("자동 시작 상태를 불러오지 못했습니다.")).toBeInTheDocument();
  });

  it("비동기 조회 중 unmount되어도 늦은 결과를 안전하게 무시한다", async () => {
    let resolveInfo!: (value: typeof info) => void;
    let resolveAutostart!: (value: boolean) => void;
    mocks.desktop = true;
    mocks.getSystemInfo.mockReturnValue(new Promise((resolve) => { resolveInfo = resolve; }));
    mocks.getAutostart.mockReturnValue(new Promise((resolve) => { resolveAutostart = resolve; }));
    const unhandled = vi.fn();
    window.addEventListener("unhandledrejection", unhandled);

    const view = render(Settings);
    view.unmount();
    resolveInfo(info);
    resolveAutostart(true);
    await Promise.resolve();
    await Promise.resolve();

    expect(unhandled).not.toHaveBeenCalled();
    window.removeEventListener("unhandledrejection", unhandled);
  });
});
