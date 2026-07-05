<script lang="ts">
  import type { Entity, SchemaMap } from "../types";
  import { listEntities } from "../api";
  import { navigate } from "../router.svelte";
  import EntityTable from "../EntityTable.svelte";

  let { schemas, type, params }: { schemas: SchemaMap; type: string; params: Record<string, string> } = $props();

  let entities = $state<Entity[]>([]);
  let loaded = $state(false);
  $effect(() => {
    loaded = false;
    listEntities(type, params).then((e) => { entities = e; loaded = true; });
  });
  const category = $derived(schemas[type]?.category ?? "기타");
</script>

{#if schemas[type]}
  <header class="browse-header">
    <div>
      <p class="crumb">{category} › {type}</p>
      <h1>{type} <span class="count">{loaded ? `${entities.length}개` : ""}</span></h1>
    </div>
    <div class="actions">
      <button type="button" class="settings" onclick={() => navigate(`/types/${encodeURIComponent(type)}/edit`)}>타입 설정</button>
      <button type="button" class="new" onclick={() => navigate(`/new/${encodeURIComponent(type)}`)}>+ 새 {type}</button>
    </div>
  </header>
  {#if loaded && entities.length === 0}
    <div class="empty-card">
      <p>아직 {type}이 없어요 — 첫 항목을 추가해 보세요.</p>
      <button type="button" onclick={() => navigate(`/new/${encodeURIComponent(type)}`)}>+ {type} 추가</button>
    </div>
  {:else}
    <EntityTable schema={schemas[type]} entities={entities} schemas={schemas} onrowclick={(e) => navigate(`/entity/${encodeURIComponent(e.id)}`)} />
  {/if}
{:else}
  <p>알 수 없는 타입: {type}</p>
{/if}
