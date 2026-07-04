<script lang="ts">
  import type { SchemaMap } from "./lib/types";
  import { getSchemas } from "./lib/api";
  import { router } from "./lib/router.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Home from "./lib/pages/Home.svelte";
  import Browse from "./lib/pages/Browse.svelte";
  import Detail from "./lib/pages/Detail.svelte";
  import New from "./lib/pages/New.svelte";
  import PageView from "./lib/pages/PageView.svelte";

  let schemas = $state<SchemaMap>({});
  let loaded = $state(false);
  $effect(() => {
    getSchemas().then((s) => { schemas = s; loaded = true; });
  });
</script>

<div class="app">
  {#if loaded}
    <Sidebar schemas={schemas} onreloaded={(s) => (schemas = s)} />
    <main>
      {#if router.route.name === "home"}
        <Home schemas={schemas} />
      {:else if router.route.name === "browse"}
        <Browse schemas={schemas} type={router.route.type} params={router.route.params} />
      {:else if router.route.name === "entity"}
        <Detail schemas={schemas} id={router.route.id} />
      {:else if router.route.name === "new"}
        <New schemas={schemas} type={router.route.type} />
      {:else if router.route.name === "page"}
        <PageView pageName={router.route.pageName} schemas={schemas} />
      {/if}
    </main>
  {:else}
    <p>불러오는 중…</p>
  {/if}
</div>

<style>
  .app { display: flex; gap: 1rem; }
  .sidebar { width: 200px; }
  main { flex: 1; }
</style>
