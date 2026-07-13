<script lang="ts">
  import { getSystemInfo, type SystemInfo } from "../api";
  import { getAutostart, importFromDir, isDesktop, openDataDir, setAutostart } from "../tauri";

  const desktop = isDesktop();
  let info = $state<SystemInfo | null>(null);
  let infoError = $state("");
  let autostart = $state(false);
  let autostartReady = $state(false);
  let autostartBusy = $state(false);
  let autostartError = $state("");
  let dataDirError = $state("");
  let importPath = $state("");
  let importBusy = $state(false);
  let importMessage = $state("");
  let importError = $state("");
  let mounted = true;

  $effect(() => () => { mounted = false; });

  $effect(() => {
    let active = true;
    void getSystemInfo()
      .then((value) => {
        if (active) info = value;
      })
      .catch(() => {
        if (active) infoError = "시스템 정보를 불러오지 못했습니다.";
      });
    return () => { active = false; };
  });

  $effect(() => {
    if (!desktop) return;
    let active = true;
    void getAutostart()
      .then((value) => {
        if (active) {
          autostart = value;
          autostartReady = true;
        }
      })
      .catch(() => {
        if (active) {
          autostartReady = false;
          autostartError = "자동 시작 상태를 불러오지 못했습니다.";
        }
      });
    return () => { active = false; };
  });

  async function toggleAutostart(event: Event) {
    if (autostartBusy) return;
    const previous = autostart;
    const next = (event.currentTarget as HTMLInputElement).checked;
    autostart = next;
    autostartBusy = true;
    autostartError = "";
    try {
      await setAutostart(next);
    } catch {
      if (mounted) {
        autostart = previous;
        autostartError = "자동 시작 설정을 변경하지 못했습니다.";
      }
    } finally {
      if (mounted) autostartBusy = false;
    }
  }

  async function revealDataDir() {
    dataDirError = "";
    try {
      await openDataDir();
    } catch {
      if (mounted) dataDirError = "데이터 폴더를 열지 못했습니다.";
    }
  }

  async function doImport() {
    const path = importPath.trim();
    if (!path || importBusy) return;
    importBusy = true;
    importMessage = "";
    importError = "";
    try {
      await importFromDir(path);
      if (mounted) {
        importMessage = "가져오기 준비 완료 — 앱을 재시작하면 적용됩니다.";
        importPath = "";
      }
    } catch (error) {
      if (mounted) importError = `데이터 가져오기를 준비하지 못했습니다: ${String(error)}`;
    } finally {
      if (mounted) importBusy = false;
    }
  }
</script>

<section class="settings">
  <h2>설정</h2>
  {#if info}
    <div class="row">
      <span class="label">데이터 위치</span>
      <code>{info.data_dir}</code>
      {#if desktop}
        <button type="button" onclick={revealDataDir}>폴더 열기</button>
      {/if}
    </div>
    {#if dataDirError}<p class="error" role="alert">{dataDirError}</p>{/if}
    <div class="row"><span class="label">포트</span><code>{info.port}</code></div>
    <div class="row">
      <span class="label">LAN 접속 주소</span>
      {#if info.lan_addrs.length > 0}
        <ul>{#each info.lan_addrs as address (address)}<li><code>{address}</code></li>{/each}</ul>
      {:else}
        <span>없음</span>
      {/if}
    </div>
    {#if desktop}
      <label class="row">
        <span class="label">로그인 시 자동 시작</span>
        <span><input type="checkbox" checked={autostart} disabled={!autostartReady || autostartBusy} onchange={toggleAutostart} /> 사용</span>
      </label>
      <div class="row">
        <label class="label" for="import-path">데이터 가져오기</label>
        <input id="import-path" type="text" placeholder="기존 디렉터리 경로" bind:value={importPath} disabled={importBusy} />
        <button type="button" onclick={doImport} disabled={!importPath.trim() || importBusy}>가져오기</button>
      </div>
      <p class="hint">현재 데이터는 유지되며, 가져온 데이터는 다음 앱 시작 전에 안전하게 적용됩니다.</p>
    {/if}
  {:else if infoError}
    <p class="error" role="alert">{infoError}</p>
  {:else}
    <p>불러오는 중…</p>
  {/if}
  {#if desktop && autostartError}<p class="error" role="alert">{autostartError}</p>{/if}
  {#if desktop && importMessage}<p class="success" role="status">{importMessage}</p>{/if}
  {#if desktop && importError}<p class="error" role="alert">{importError}</p>{/if}
</section>
