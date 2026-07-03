<script lang="ts">
  import type { ResolvedField } from "../types";
  import Widget from "./Widget.svelte";

  let {
    field,
    value,
    onchange,
  }: {
    field: ResolvedField;
    value: unknown[] | null;
    onchange: (v: unknown[]) => void;
  } = $props();

  const items = $derived((value ?? []) as unknown[]);

  function setAt(i: number, v: unknown) {
    const next = items.slice();
    next[i] = v;
    onchange(next);
  }

  function add() {
    onchange([...items, defaultFor(field.kind)]);
  }

  function removeAt(i: number) {
    onchange(items.filter((_, j) => j !== i));
  }

  function defaultFor(kind: string): unknown {
    if (kind === "bool") return false;
    if (kind === "number") return null;
    if (kind === "money") return null;
    return "";
  }
</script>

<div class="list">
  {#each items as item, i}
    <div class="list-row">
      <Widget field={field} value={item} onchange={(v) => setAt(i, v)} />
      <button type="button" onclick={() => removeAt(i)}>✕</button>
    </div>
  {/each}
  <button type="button" onclick={add}>+ 추가</button>
</div>
