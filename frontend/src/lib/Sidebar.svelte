<script lang="ts">
  import type { Category, SchemaMap, SchemasResponse } from "./types";
  import { navigate } from "./router.svelte";
  import { reload, getSchemas } from "./api";

  let { schemas, categories, onreloaded }: {
    schemas: SchemaMap;
    categories: Category[];
    onreloaded: (r: SchemasResponse) => void;
  } = $props();

  let collapsed = $state<Record<string, boolean>>({});

  const groups = $derived.by(() => {
    const known = new Set(categories.map((c) => c.name));
    const out: { cat: Category; types: string[] }[] = categories.map((cat) => ({
      cat,
      types: Object.keys(schemas).filter((t) => schemas[t].category === cat.name),
    }));
    const rest = Object.keys(schemas).filter(
      (t) => !schemas[t].category || !known.has(schemas[t].category!)
    );
    if (rest.length > 0) out.push({ cat: { name: "기타" }, types: rest });
    return out.filter((g) => g.types.length > 0);
  });

  async function doReload() {
    await reload();
    onreloaded(await getSchemas());
  }
</script>

<nav class="sidebar">
  <h1>LifeOps</h1>
  <button type="button" class="home" onclick={() => navigate("/")}>🏠 홈</button>
  {#each groups as g (g.cat.name)}
    <div class="group">
      <button type="button" class="group-header" onclick={() => (collapsed[g.cat.name] = !collapsed[g.cat.name])}>
        {g.cat.icon ?? "📁"} {g.cat.name} <span class="chev">{collapsed[g.cat.name] ? "▸" : "▾"}</span>
      </button>
      <button type="button" class="add-type" title="새 타입" onclick={() => navigate("/types/new")}>+ 새 타입</button>
      {#if !collapsed[g.cat.name]}
        <ul>
          {#each g.types as type (type)}
            <li>
              <button type="button" onclick={() => navigate(`/browse/${encodeURIComponent(type)}`)}>{type}</button>
              <button type="button" class="add" title="추가" onclick={() => navigate(`/new/${encodeURIComponent(type)}`)}>+</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/each}
  <button type="button" class="reload" onclick={doReload}>스키마 리로드</button>
</nav>
