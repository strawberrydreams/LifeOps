<script lang="ts">
  import type { ResolvedField } from "../types";
  import { parseKind } from "../kind";
  import MoneyWidget from "./MoneyWidget.svelte";
  import ListWidget from "./ListWidget.svelte";

  let {
    field,
    value,
    onchange,
  }: {
    field: ResolvedField;
    value: unknown;
    onchange: (v: unknown) => void;
  } = $props();

  const parsed = $derived(parseKind(field.kind));
  const kind = $derived(parsed.base);
</script>

{#if parsed.list}
  <ListWidget field={{ ...field, kind: parsed.base }} value={value as unknown[] | null} {onchange} />
{:else if kind === "money"}
  <MoneyWidget value={value as { amount: number; currency: string } | null} {onchange} />
{:else if kind === "text" || kind === "image" || kind === "url"}
  <input
    type={kind === "url" ? "url" : "text"}
    value={(value as string) ?? ""}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
  />
{:else if kind === "number"}
  <span>
    <input
      type="number"
      value={value === null || value === undefined ? "" : (value as number)}
      oninput={(e) => {
        const v = (e.currentTarget as HTMLInputElement).value;
        onchange(v === "" ? null : Number(v));
      }}
    />
    {#if field.unit}<span class="unit">{field.unit}</span>{/if}
  </span>
{:else if kind === "date"}
  <input
    type="date"
    value={(value as string) ?? ""}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value || null)}
  />
{:else if kind === "bool"}
  <input
    type="checkbox"
    checked={value === true}
    onchange={(e) => onchange((e.currentTarget as HTMLInputElement).checked)}
  />
{:else if kind === "enum"}
  <select
    value={(value as string) ?? ""}
    onchange={(e) => onchange((e.currentTarget as HTMLSelectElement).value || null)}
  >
    <option value=""></option>
    {#each field.options ?? [] as opt}
      <option value={opt}>{opt}</option>
    {/each}
  </select>
{:else}
  <!-- ref/richtext 는 Task 4·5에서 분기 추가; 그전까지 폴백 input -->
  <input
    value={value === null || value === undefined ? "" : String(value)}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
  />
{/if}
