<script lang="ts">
  import type { SchemaMap } from "./types";

  export interface EditorRow {
    localId: string;
    originalName: string | null;
    name: string;
    kind: string;
    required: boolean;
    options: string[];
    target: string | null;
    unit: string | null;
  }

  let {
    row,
    schemas,
    onchange,
    onremove,
    onmove,
  }: {
    row: EditorRow;
    schemas: SchemaMap;
    onchange: (row: EditorRow) => void;
    onremove: () => void;
    onmove: (dir: -1 | 1) => void;
  } = $props();

  const KINDS = [
    "text",
    "richtext",
    "number",
    "money",
    "date",
    "bool",
    "enum",
    "url",
    "ref",
    "image",
    "list<text>",
    "list<ref>",
    "list<enum>",
  ];

  function baseKind(kind: string) {
    const match = kind.match(/^list<(.+)>$/);
    return match ? match[1] : kind;
  }

  const isEnum = $derived(baseKind(row.kind) === "enum");
  const isRef = $derived(baseKind(row.kind) === "ref");
  const isNumber = $derived(row.kind === "number");
  const targetOptions = $derived(Object.keys(schemas));

  let optionsText = $state("");

  $effect(() => {
    optionsText = row.options.join(", ");
  });

  function emit(patch: Partial<EditorRow>) {
    onchange({ ...row, ...patch });
  }

  function setKind(kind: string) {
    emit({ kind });
  }

  function setOptions(text: string) {
    optionsText = text;
    emit({ options: text.split(",").map((s) => s.trim()).filter(Boolean) });
  }
</script>

<div class="field-row">
  <input
    aria-label="name"
    class="fname"
    value={row.name}
    placeholder="필드명"
    oninput={(e) => emit({ name: (e.currentTarget as HTMLInputElement).value })}
  />
  <select aria-label="kind" value={row.kind} onchange={(e) => setKind((e.currentTarget as HTMLSelectElement).value)}>
    {#each KINDS as kind (kind)}
      <option value={kind}>{kind}</option>
    {/each}
  </select>
  <label class="required">
    <input
      type="checkbox"
      checked={row.required}
      onchange={(e) => emit({ required: (e.currentTarget as HTMLInputElement).checked })}
    />
    필수
  </label>

  {#if isEnum}
    <input
      aria-label="options"
      placeholder="옵션(쉼표로 구분)"
      value={optionsText}
      oninput={(e) => setOptions((e.currentTarget as HTMLInputElement).value)}
    />
  {/if}
  {#if isRef}
    <select aria-label="target" value={row.target ?? ""} onchange={(e) => emit({ target: (e.currentTarget as HTMLSelectElement).value || null })}>
      <option value="">(대상 없음)</option>
      {#each targetOptions as target (target)}
        <option value={target}>{target}</option>
      {/each}
    </select>
  {/if}
  {#if isNumber}
    <input
      aria-label="unit"
      placeholder="단위"
      value={row.unit ?? ""}
      oninput={(e) => emit({ unit: (e.currentTarget as HTMLInputElement).value || null })}
    />
  {/if}

  <button type="button" title="위로" aria-label="위로" onclick={() => onmove(-1)}>↑</button>
  <button type="button" title="아래로" aria-label="아래로" onclick={() => onmove(1)}>↓</button>
  <button type="button" onclick={onremove}>삭제</button>
</div>
