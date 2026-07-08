<script lang="ts">
  import type { Category, SchemaMap } from "./lib/types";
  import { getSchemas } from "./lib/api";
  import { router } from "./lib/router.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SearchPalette from "./lib/SearchPalette.svelte";
  import Home from "./lib/pages/Home.svelte";
  import Browse from "./lib/pages/Browse.svelte";
  import Detail from "./lib/pages/Detail.svelte";
  import New from "./lib/pages/New.svelte";
  import PageView from "./lib/pages/PageView.svelte";
  import TypeEditor from "./lib/pages/TypeEditor.svelte";

  let schemas = $state<SchemaMap>({});
  let categories = $state<Category[]>([]);
  let loaded = $state(false);
  $effect(() => {
    getSchemas().then((r) => { schemas = r.types; categories = r.categories; loaded = true; });
  });

  let paletteOpen = $state(false);
  function onWindowKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
      e.preventDefault();
      paletteOpen = !paletteOpen;
    }
  }
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="app">
  {#if loaded}
    <Sidebar schemas={schemas} categories={categories} onsearch={() => (paletteOpen = true)} onreloaded={(r) => { schemas = r.types; categories = r.categories; }} />
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
      {:else if router.route.name === "type-new"}
        <TypeEditor schemas={schemas} categories={categories} mode="new" onreloaded={(r) => { schemas = r.types; categories = r.categories; }} />
      {:else if router.route.name === "type-edit"}
        <TypeEditor schemas={schemas} categories={categories} mode="edit" type={router.route.type} onreloaded={(r) => { schemas = r.types; categories = r.categories; }} />
      {/if}
    </main>
    <SearchPalette open={paletteOpen} schemas={schemas} categories={categories} onclose={() => (paletteOpen = false)} />
  {:else}
    <p>불러오는 중…</p>
  {/if}
</div>
