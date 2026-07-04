<script lang="ts">
  import type { Entity, RefEdge, SchemaMap } from "../types";
  import { getEntity } from "../api";
  import { navigate } from "../router.svelte";
  import DetailView from "../DetailView.svelte";

  let { schemas, id }: { schemas: SchemaMap; id: string } = $props();

  let entity = $state<Entity | null>(null);
  let backlinks = $state<RefEdge[]>([]);
  $effect(() => {
    getEntity(id).then((r) => { entity = r.entity; backlinks = r.backlinks; });
  });
</script>

{#if entity && schemas[entity.type]}
  <DetailView schema={schemas[entity.type]} entity={entity} backlinks={backlinks} ondeleted={() => navigate("/")} />
{:else}
  <p>불러오는 중…</p>
{/if}
