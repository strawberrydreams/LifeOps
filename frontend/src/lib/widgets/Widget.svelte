<script lang="ts">
  import type { ResolvedField } from "../types";
  import { parseKind } from "../kind";
  import MoneyWidget from "./MoneyWidget.svelte";
  import ListWidget from "./ListWidget.svelte";
  import RefPicker from "./RefPicker.svelte";
  import NoteEditor from "./NoteEditor.svelte";

  let {
    field,
    value,
    onchange,
    id,
    labelledby,
    describedby,
  }: {
    field: ResolvedField;
    value: unknown;
    onchange: (v: unknown) => void;
    id?: string;
    labelledby?: string;
    describedby?: string;
  } = $props();

  const parsed = $derived(parseKind(field.kind));
  const kind = $derived(parsed.base);
</script>

{#if parsed.list}
  <ListWidget field={{ ...field, kind: parsed.base }} value={value as unknown[] | null} {onchange} {id} {labelledby} {describedby} />
{:else if kind === "money"}
  <MoneyWidget value={value as { amount: number; currency: string } | null} {onchange} {id} {labelledby} {describedby} />
{:else if kind === "ref"}
  <RefPicker field={field} value={value as string | null} onchange={onchange} {id} {labelledby} {describedby} />
{:else if kind === "richtext"}
  <NoteEditor value={value as string | null} onchange={onchange} {id} {labelledby} {describedby} />
{:else if kind === "text" || kind === "image" || kind === "url"}
  <input
    {id}
    type={kind === "url" ? "url" : "text"}
    value={(value as string) ?? ""}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
  />
{:else if kind === "number"}
  <span>
    <input
      {id}
      type="number"
      value={value === null || value === undefined ? "" : (value as number)}
      aria-labelledby={labelledby}
      aria-describedby={describedby}
      oninput={(e) => {
        const v = (e.currentTarget as HTMLInputElement).value;
        onchange(v === "" ? null : Number(v));
      }}
    />
    {#if field.unit}<span class="unit">{field.unit}</span>{/if}
  </span>
{:else if kind === "date"}
  <input
    {id}
    type="date"
    value={(value as string) ?? ""}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value || null)}
  />
{:else if kind === "bool"}
  <input
    {id}
    type="checkbox"
    checked={value === true}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    onchange={(e) => onchange((e.currentTarget as HTMLInputElement).checked)}
  />
{:else if kind === "enum"}
  <select
    {id}
    value={(value as string) ?? ""}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    onchange={(e) => onchange((e.currentTarget as HTMLSelectElement).value || null)}
  >
    <option value=""></option>
    {#each field.options ?? [] as opt}
      <option value={opt}>{opt}</option>
    {/each}
  </select>
{:else}
  <!-- Unknown kinds use a conservative text fallback. -->
  <input
    {id}
    value={value === null || value === undefined ? "" : String(value)}
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
  />
{/if}
