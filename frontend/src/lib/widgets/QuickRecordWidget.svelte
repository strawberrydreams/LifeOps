<script lang="ts">
  import type { PageBlock } from "../api";
  import type { SchemaMap } from "../types";
  import { createEntity, ApiError } from "../api";
  import { todayStr } from "../format";

  let { block, schemas }: { block: PageBlock; schemas: SchemaMap } = $props();

  const schema = $derived(schemas[block.source]);
  const enumField = $derived(
    Object.entries(schema?.fields ?? {}).find(([, field]) => field.kind === "enum")?.[0] ?? null
  );
  const numberField = $derived(
    Object.entries(schema?.fields ?? {}).find(([, field]) => field.kind === "number")?.[0] ?? null
  );
  const dateField = $derived(
    Object.entries(schema?.fields ?? {}).find(([, field]) => field.kind === "date")?.[0] ?? null
  );
  const options = $derived(enumField ? (schema?.fields[enumField]?.options ?? []) : []);

  let metric = $state("");
  let amount = $state<string | number>("");
  let when = $state(todayStr());
  let error = $state<string | null>(null);

  $effect(() => {
    if (!enumField) {
      metric = "";
    } else if (!options.includes(metric)) {
      metric = options[0] ?? "";
    }
  });

  async function record() {
    if (!numberField || String(amount).trim() === "") return;
    const data: Record<string, unknown> = { [numberField]: Number(amount) };
    if (enumField) data[enumField] = metric;
    if (dateField) data[dateField] = when;
    try {
      await createEntity(block.source, data);
      amount = "";
      when = todayStr();
      error = null;
    } catch (err) {
      error = err instanceof ApiError ? err.message : "기록 실패";
    }
  }
</script>

<div style="display: flex; gap: 0.6rem; align-items: end; flex-wrap: wrap;">
  {#if enumField}
    <label style="display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.85rem;">
      {enumField}
      <select bind:value={metric}>
        {#each options as option}
          <option value={option}>{option}</option>
        {/each}
      </select>
    </label>
  {/if}
  {#if numberField}
    <label style="display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.85rem;">
      {numberField}
      <input type="number" step="any" bind:value={amount} />
    </label>
  {/if}
  {#if dateField}
    <label style="display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.85rem;">
      {dateField}
      <input type="date" bind:value={when} />
    </label>
  {/if}
  <button type="button" onclick={record}>기록</button>
  {#if error}<span class="error">{error}</span>{/if}
</div>
