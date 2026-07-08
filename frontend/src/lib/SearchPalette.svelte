<script lang="ts">
  import type { Category, SchemaMap } from "./types";
  import { search as searchApi, type SearchHit } from "./api";
  import { navigate } from "./router.svelte";

  let { open, schemas, categories, onclose }: {
    open: boolean;
    schemas: SchemaMap;
    categories: Category[];
    onclose: () => void;
  } = $props();
  $effect(() => void schemas); // 예약(향후 타입별 아이콘 등). 현재 미사용.

  let query = $state("");
  let limit = $state(20);
  let hits = $state<SearchHit[]>([]);
  let total = $state(0);
  let truncated = $state(false);
  let selected = $state(0);
  let loading = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let reqSeq = 0; // 요청 세대 번호 — 늦게 온 오래된 응답을 폐기한다.

  $effect(() => {
    if (open) {
      query = "";
      hits = [];
      total = 0;
      truncated = false;
      selected = 0;
      limit = 20;
      queueMicrotask(() => inputEl?.focus());
    }
  });

  async function run() {
    const q = query.trim();
    if (!q) { hits = []; total = 0; truncated = false; return; }
    const myReq = ++reqSeq;
    loading = true;
    try {
      const res = await searchApi(q, limit);
      if (myReq !== reqSeq) return; // 더 새로운 요청이 시작됨 → 오래된 응답 폐기
      hits = res.results;
      total = res.total;
      truncated = res.truncated;
      selected = 0;
    } finally {
      if (myReq === reqSeq) loading = false;
    }
  }

  function oninput() {
    limit = 20;
    if (timer) clearTimeout(timer);
    timer = setTimeout(run, 150);
  }

  function more() {
    limit += 30;
    run();
  }

  const groups = $derived.by(() => {
    const order = categories.map((c) => c.name);
    const byCat = new Map<string, SearchHit[]>();
    for (const h of hits) {
      const key = h.category && order.includes(h.category) ? h.category : "기타";
      let arr = byCat.get(key);
      if (!arr) { arr = []; byCat.set(key, arr); }
      arr.push(h);
    }
    const out: { name: string; icon?: string | null; hits: SearchHit[] }[] = [];
    for (const c of categories) {
      if (byCat.has(c.name)) out.push({ name: c.name, icon: c.icon, hits: byCat.get(c.name)! });
    }
    if (byCat.has("기타")) out.push({ name: "기타", hits: byCat.get("기타")! });
    return out;
  });

  const flat = $derived(groups.flatMap((g) => g.hits));

  function choose(h: SearchHit) {
    navigate(h.href);
    onclose();
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); onclose(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); if (flat.length) selected = (selected + 1) % flat.length; }
    else if (e.key === "ArrowUp") { e.preventDefault(); if (flat.length) selected = (selected - 1 + flat.length) % flat.length; }
    else if (e.key === "Enter") { e.preventDefault(); if (flat[selected]) choose(flat[selected]); }
  }

  function parts(h: SearchHit) {
    const s = h.snippet;
    if (h.match.len === 0) return { before: s, hit: "", after: "" };
    const a = h.match.start;
    const b = h.match.start + h.match.len;
    return { before: s.slice(0, a), hit: s.slice(a, b), after: s.slice(b) };
  }
</script>

{#if open}
  <div class="search-palette-overlay" onclick={onclose} role="presentation">
    <div
      class="search-palette"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => { if (e.key === "Escape" || e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "Enter") e.stopPropagation(); }}
      role="dialog"
      aria-label="검색"
      aria-modal="true"
      tabindex="-1"
    >
      <input
        bind:this={inputEl}
        bind:value={query}
        oninput={oninput}
        onkeydown={onkey}
        placeholder="검색…"
        aria-label="검색어"
      />
      {#if query.trim() && !loading && flat.length === 0}
        <p class="empty">일치하는 항목이 없어요</p>
      {/if}
      {#each groups as g (g.name)}
        <div class="group">
          <div class="group-title"><span class="icon">{g.icon ?? "📁"}</span> <span class="name">{g.name}</span></div>
          <ul>
            {#each g.hits as h (h.id)}
              {@const p = parts(h)}
              <li>
                <button type="button" class:selected={flat[selected]?.id === h.id} onclick={() => choose(h)}>
                  <span class="type">{h.type}</span>
                  <span class="label">{h.label}</span>
                  <span class="snippet">{p.before}<mark>{p.hit}</mark>{p.after}</span>
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
      {#if truncated}
        <button type="button" class="more" onclick={more}>더 보기 ({total}건 중 {flat.length}건)</button>
      {/if}
    </div>
  </div>
{/if}
