<script lang="ts">
  import type { PageBlock } from "../api";
  import type { Entity, SchemaMap } from "../types";
  import { createEntity, updateEntity, ApiError } from "../api";
  import { todayStr } from "../format";
  import { refLabel } from "../reflabel";
  import { navigate } from "../router.svelte";
  import { parseKind } from "../kind";

  let { block, schemas }: { block: PageBlock; schemas: SchemaMap } = $props();

  const schema = $derived(schemas[block.source]);
  const rec = $derived(schema?.behaviors?.recurrence ?? null);
  const flagField = $derived(rec?.flag ?? "완료");
  const dateField = $derived(
    rec?.date ?? Object.entries(schema?.fields ?? {}).find(([, f]) => f.kind === "date")?.[0] ?? null
  );
  const ruleField = $derived(rec?.rule ?? null);
  const priorityField = $derived(schema?.fields?.["우선순위"] ? "우선순위" : null);
  const titleField = $derived(
    Object.entries(schema?.fields ?? {}).find(([, f]) => f.kind === "text" && f.required)?.[0] ?? "내용"
  );
  const refFields = $derived(
    Object.entries(schema?.fields ?? {}).filter(([, f]) => parseKind(f.kind).base === "ref").map(([n]) => n)
  );
  const hidesDone = $derived(block.filter?.[flagField] === false);

  let rows = $state<Entity[]>([]);
  $effect(() => { rows = block.entities; });
  let draft = $state("");
  let error = $state<string | null>(null);
  let warnings = $state<Record<string, string>>({});
  let labels = $state<Record<string, string>>({});

  const today = todayStr();

  function dueClass(e: Entity): string {
    const d = dateField ? (e.data[dateField] as string | undefined) : undefined;
    if (!d) return "";
    if (d < today) return "overdue";
    if (d === today) return "due-today";
    return "";
  }

  function refIds(e: Entity): string[] {
    const out: string[] = [];
    for (const f of refFields) {
      const v = e.data[f];
      if (typeof v === "string") out.push(v);
      if (Array.isArray(v)) out.push(...v.filter((x): x is string => typeof x === "string"));
    }
    return out;
  }

  function loadLabel(id: string) {
    if (!(id in labels)) {
      labels[id] = "…";
      refLabel(id, schemas).then((l) => (labels[id] = l));
    }
    return labels[id];
  }

  async function toggle(e: Entity, checked: boolean) {
    try {
      const updated = await updateEntity(e.id, { [flagField]: checked });
      if (updated.recurrence_warning) warnings[e.id] = updated.recurrence_warning;
      if (checked && hidesDone && !updated.recurrence_warning) {
        rows = rows.filter((r) => r.id !== e.id);
      } else {
        rows = rows.map((r) => (r.id === e.id ? updated : r));
      }
      if (updated.spawned) rows = [...rows, updated.spawned];
      error = null;
    } catch (err) {
      error = err instanceof ApiError ? err.message : "저장 실패";
    }
  }

  async function quickAdd() {
    const value = draft.trim();
    if (!value) return;
    try {
      const created = await createEntity(block.source, { [titleField]: value, [flagField]: false });
      rows = [created, ...rows];
      draft = "";
      error = null;
    } catch (err) {
      error = err instanceof ApiError ? err.message : "추가 실패";
    }
  }
</script>

<div class="todo">
  {#if rows.length === 0}
    <div class="empty-card">
      <p>할 일이 없어요 — 아래 입력창으로 바로 추가해 보세요.</p>
    </div>
  {:else}
    <ul class="checklist">
      {#each rows as e (e.id)}
        <li>
          <input type="checkbox" checked={e.data[flagField] === true} onchange={(ev) => toggle(e, (ev.currentTarget as HTMLInputElement).checked)} />
          <button type="button" class="title" onclick={() => navigate(`/entity/${encodeURIComponent(e.id)}`)}>
            {String(e.data[titleField] ?? e.id)}
          </button>
          {#if dateField && e.data[dateField]}
            <span class="badge {dueClass(e)}">{String(e.data[dateField])}</span>
          {/if}
          {#if priorityField && e.data[priorityField] === "높음"}
            <span class="badge priority">높음</span>
          {/if}
          {#if ruleField && e.data[ruleField]}
            <span class="badge repeat" title={String(e.data[ruleField])}>🔁</span>
          {/if}
          {#each refIds(e) as rid (rid)}
            <button type="button" class="chip" onclick={() => navigate(`/entity/${encodeURIComponent(rid)}`)}>{loadLabel(rid)}</button>
          {/each}
          {#if warnings[e.id]}<span class="badge warn" title={warnings[e.id]}>⚠️</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
  <input
    class="quick-add"
    placeholder="빠른 추가 — 입력 후 Enter"
    bind:value={draft}
    onkeydown={(ev) => { if (ev.key === "Enter") quickAdd(); }}
  />
  {#if error}<p class="error">{error}</p>{/if}
</div>
