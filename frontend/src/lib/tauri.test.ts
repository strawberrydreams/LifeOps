import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

import { getAutostart, importFromDir, isDesktop, openDataDir, setAutostart } from "./tauri";

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  vi.clearAllMocks();
});

describe("tauri", () => {
  it("일반 브라우저에서는 desktop이 아니다", () => {
    expect(isDesktop()).toBe(false);
  });

  it("Tauri internals가 있을 때만 desktop이다", () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    expect(isDesktop()).toBe(true);
  });

  it("desktop 명령 이름과 인자를 그대로 invoke한다", async () => {
    mocks.invoke.mockResolvedValue(undefined);

    await getAutostart();
    await setAutostart(true);
    await openDataDir();
    await importFromDir("/Users/me/Archive");

    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "get_autostart", undefined);
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "set_autostart", { enabled: true });
    expect(mocks.invoke).toHaveBeenNthCalledWith(3, "open_data_dir", undefined);
    expect(mocks.invoke).toHaveBeenNthCalledWith(4, "import_from_dir", { dir: "/Users/me/Archive" });
  });
});
