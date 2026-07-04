<script lang="ts">
  import type { Entity, ResolvedField } from "../types";
  import { listEntities } from "../api";

  let {
    field,
    value,
    onchange,
    id,
    labelledby,
    describedby,
  }: {
    field: ResolvedField;
    value: string | null;
    onchange: (v: string | null) => void;
    id?: string;
    labelledby?: string;
    describedby?: string;
  } = $props();

  let query = $state("");
  let results = $state<Entity[]>([]);
  let open = $state(false);
  let requestSeq = 0;

  function label(e: Entity): string {
    const firstStr = Object.values(e.data).find((v) => typeof v === "string" && v.length > 0);
    const base = (firstStr as string) ?? e.id;
    return field.target ? base : `[${e.type}] ${base}`;
  }

  async function search(q: string) {
    const requestId = ++requestSeq;
    query = q;
    open = true;
    try {
      const all = await listEntities(field.target ?? "", {});
      if (requestId !== requestSeq) return;
      results = q ? all.filter((e) => label(e).includes(q)) : all;
      open = true;
    } catch {
      if (requestId !== requestSeq) return;
      results = [];
      open = false;
    }
  }

  function pick(e: Entity) {
    onchange(e.id);
    query = label(e);
    open = false;
  }
</script>

<div class="refpicker">
  <input
    {id}
    type="text"
    placeholder="검색..."
    value={query || (value ?? "")}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    oninput={(e) => search((e.currentTarget as HTMLInputElement).value)}
  />
  {#if open && results.length > 0}
    <ul class="results">
      {#each results as e (e.id)}
        <li><button type="button" onclick={() => pick(e)}>{label(e)}</button></li>
      {/each}
    </ul>
  {/if}
</div>
