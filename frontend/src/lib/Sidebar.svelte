<script lang="ts">
  import type { SchemaMap } from "./types";
  import { navigate } from "./router.svelte";
  import { reload, getSchemas } from "./api";

  let { schemas, onreloaded }: { schemas: SchemaMap; onreloaded: (s: SchemaMap) => void } = $props();

  async function doReload() {
    await reload();
    onreloaded(await getSchemas());
  }
</script>

<nav class="sidebar">
  <h1>LifeOps</h1>
  <ul>
    {#each Object.keys(schemas) as type}
      <li>
        <button type="button" onclick={() => navigate(`/browse/${encodeURIComponent(type)}`)}>{type}</button>
        <button type="button" class="add" title="추가" onclick={() => navigate(`/new/${encodeURIComponent(type)}`)}>+</button>
      </li>
    {/each}
  </ul>
  <button type="button" class="reload" onclick={doReload}>스키마 리로드</button>
</nav>
