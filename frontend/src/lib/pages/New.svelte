<script lang="ts">
  import type { SchemaMap } from "../types";
  import { createEntity } from "../api";
  import { navigate } from "../router.svelte";
  import SchemaForm from "../SchemaForm.svelte";

  let { schemas, type }: { schemas: SchemaMap; type: string } = $props();

  async function submit(data: Record<string, unknown>) {
    const e = await createEntity(type, data);
    navigate(`/entity/${encodeURIComponent(e.id)}`);
  }
</script>

{#if schemas[type]}
  <h1>새 {type}</h1>
  <SchemaForm schema={schemas[type]} onsubmit={submit} />
{:else}
  <p>알 수 없는 타입: {type}</p>
{/if}
