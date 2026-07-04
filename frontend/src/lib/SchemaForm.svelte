<script lang="ts">
  import type { ResolvedSchema } from "./types";
  import { ApiError } from "./api";
  import Widget from "./widgets/Widget.svelte";

  let {
    schema,
    initial = {},
    onsubmit,
  }: {
    schema: ResolvedSchema;
    initial?: Record<string, unknown>;
    onsubmit: (data: Record<string, unknown>) => Promise<void>;
  } = $props();

  let data = $state<Record<string, unknown> | null>(null);
  let errors = $state<Record<string, string>>({});
  let formError = $state<string | null>(null);

  $effect(() => {
    if (data === null) data = { ...initial };
  });

  function setField(name: string, v: unknown) {
    if (data === null) return;
    if (v === null || v === undefined || v === "") {
      delete data[name];
    } else {
      data[name] = v;
    }
  }

  async function submit(e: Event) {
    e.preventDefault();
    errors = {};
    formError = null;
    try {
      await onsubmit({ ...(data ?? {}) });
    } catch (err) {
      if (err instanceof ApiError && err.fields) {
        const next: Record<string, string> = {};
        for (const fe of err.fields) next[fe.field] = fe.message;
        errors = next;
      } else {
        formError = err instanceof Error ? err.message : "저장 실패";
      }
    }
  }

  function fieldBaseId(i: number) {
    return `schema-form-field-${i}`;
  }
</script>

<form onsubmit={submit}>
  {#each Object.entries(schema.fields) as [name, field], i}
    <div class="field">
      <div class="label" id={`${fieldBaseId(i)}-label`}>{name}{#if field.required}<span class="req">*</span>{/if}</div>
      <Widget
        id={`${fieldBaseId(i)}-control`}
        {field}
        value={data?.[name]}
        onchange={(v) => setField(name, v)}
        labelledby={`${fieldBaseId(i)}-label`}
        describedby={errors[name] ? `${fieldBaseId(i)}-error` : undefined}
      />
      {#if errors[name]}<div class="error" id={`${fieldBaseId(i)}-error`}>{errors[name]}</div>{/if}
    </div>
  {/each}
  {#if formError}<div class="error">{formError}</div>{/if}
  <button type="submit">저장</button>
</form>
