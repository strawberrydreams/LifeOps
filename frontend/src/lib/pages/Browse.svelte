<script lang="ts">
  import type { Entity, SchemaMap } from "../types";
  import { listEntities } from "../api";
  import { navigate } from "../router.svelte";
  import EntityTable from "../EntityTable.svelte";

  let { schemas, type, params }: { schemas: SchemaMap; type: string; params: Record<string, string> } = $props();

  let entities = $state<Entity[]>([]);
  $effect(() => {
    listEntities(type, params).then((e) => (entities = e));
  });
</script>

{#if schemas[type]}
  <h1>{type}</h1>
  <EntityTable schema={schemas[type]} entities={entities} onrowclick={(e) => navigate(`/entity/${encodeURIComponent(e.id)}`)} />
{:else}
  <p>알 수 없는 타입: {type}</p>
{/if}
