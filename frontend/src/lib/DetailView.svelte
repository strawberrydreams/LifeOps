<script lang="ts">
  import type { Entity, RefEdge, ResolvedSchema } from "./types";
  import { updateEntity, deleteEntity, ApiError } from "./api";
  import Widget from "./widgets/Widget.svelte";

  let { schema, entity, backlinks, onsaved, ondeleted }: {
    schema: ResolvedSchema;
    entity: Entity;
    backlinks: RefEdge[];
    onsaved?: (e: Entity) => void;
    ondeleted?: () => void;
  } = $props();

  let data = $state<Record<string, unknown>>({});
  let blockers = $state<RefEdge[]>([]);
  let msg = $state<string | null>(null);

  $effect(() => {
    data = { ...entity.data };
  });

  function set(name: string, v: unknown) {
    if (v === null || v === undefined || v === "") delete data[name];
    else data[name] = v;
  }
  async function save() {
    try {
      const updated = await updateEntity(entity.id, { ...data });
      msg = "저장됨";
      onsaved?.(updated);
    } catch (err) {
      if (err instanceof ApiError) {
        const fieldMsgs = err.fields?.map((f) => `${f.field}: ${f.message}`).join(", ");
        msg = fieldMsgs ? `${err.message} (${fieldMsgs})` : err.message;
      } else {
        msg = "저장 실패";
      }
    }
  }
  async function remove() {
    blockers = [];
    try {
      await deleteEntity(entity.id);
      ondeleted?.();
    } catch (err) {
      if (err instanceof ApiError && err.code === "delete_blocked") {
        blockers = err.referrers ?? [];
      } else {
        msg = err instanceof Error ? err.message : "삭제 실패";
      }
    }
  }
</script>

<div class="detail">
  <h2>{entity.type}</h2>
  {#each Object.entries(schema.fields) as [name, field]}
    <div class="field">
      <label>{name}<Widget field={field} value={data[name]} onchange={(v) => set(name, v)} /></label>
    </div>
  {/each}
  <button type="button" onclick={save}>저장</button>
  <button type="button" onclick={remove}>삭제</button>
  {#if msg}<div class="msg">{msg}</div>{/if}

  {#if blockers.length > 0}
    <div class="blockers">
      <p>삭제할 수 없습니다 — 참조 중:</p>
      <ul>{#each blockers as b}<li>{b.from_type} ({b.from_id}) · {b.field_name}</li>{/each}</ul>
    </div>
  {/if}

  <section class="backlinks">
    <h3>역링크</h3>
    {#if backlinks.length === 0}
      <p>참조하는 곳 없음</p>
    {:else}
      <ul>{#each backlinks as b}<li>{b.from_type} ({b.from_id}) · {b.field_name}</li>{/each}</ul>
    {/if}
  </section>
</div>
