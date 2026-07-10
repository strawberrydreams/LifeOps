<script lang="ts">
  import type { Entity, ResolvedField, SchemaMap } from "./types";
  import { formatValue, todayStr } from "./format";
  import { parseKind } from "./kind";
  import ProvenanceTrigger from "./ProvenanceTrigger.svelte";
  import { refLabel } from "./reflabel";
  import { navigate } from "./router.svelte";

  let {
    field,
    value,
    schemas,
    entity = null,
    fieldName,
  }: {
    field: ResolvedField;
    value: unknown;
    schemas: SchemaMap;
    entity?: Entity | null;
    fieldName?: string;
  } = $props();

  const parsed = $derived(parseKind(field.kind));
  const today = todayStr();
  let labels = $state<Record<string, string>>({});

  function dateClass(v: string): string {
    if (v < today) return "overdue";
    if (v === today) return "due-today";
    return "";
  }
  function domain(u: string): string {
    try { return new URL(u).hostname; } catch { return u; }
  }
  function ids(v: unknown): string[] {
    if (typeof v === "string") return [v];
    if (Array.isArray(v)) return v.filter((x): x is string => typeof x === "string");
    return [];
  }
  function loadLabel(id: string) {
    if (!(id in labels)) {
      labels[id] = "…";
      refLabel(id, schemas).then((l) => (labels[id] = l));
    }
    return labels[id];
  }
</script>

{#if value === null || value === undefined || value === ""}
  <span class="muted"></span>
{:else if parsed.base === "date" && typeof value === "string"}
  <span class="badge {dateClass(value)}">{value}</span>
{:else if parsed.base === "bool"}
  <span>{value === true ? "✓" : "–"}</span>
{:else if parsed.base === "url" && typeof value === "string"}
  <a href={value} target="_blank" rel="noreferrer" onclick={(e) => e.stopPropagation()}>{domain(value)}</a>
{:else if parsed.base === "ref"}
  {#each ids(value) as id (id)}
    <button type="button" class="chip" onclick={(e) => { e.stopPropagation(); navigate(`/entity/${encodeURIComponent(id)}`); }}>{loadLabel(id)}</button>
  {/each}
{:else}
  <span>{formatValue(field, value)}</span>
{/if}
{#if entity && fieldName}
  <ProvenanceTrigger {entity} {fieldName} />
{/if}
