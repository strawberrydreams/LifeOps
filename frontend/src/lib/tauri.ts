/** Tauri webview 안에서만 true(일반 브라우저·SSR·테스트에서는 false). */
export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export function getAutostart(): Promise<boolean> {
  return invoke<boolean>("get_autostart");
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke<void>("set_autostart", { enabled });
}

export function openDataDir(): Promise<void> {
  return invoke<void>("open_data_dir");
}

export function importFromDir(dir: string): Promise<void> {
  return invoke<void>("import_from_dir", { dir });
}
