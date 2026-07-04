<script lang="ts">
  import type { PageBlock } from "./api";
  import type { SchemaMap } from "./types";
  import { formatValue } from "./format";

  let { page, blocks, schemas }: { page: string; blocks: PageBlock[]; schemas: SchemaMap } = $props();

  function display(e: PageBlock["entities"][number], c: string): string {
    const field = schemas[e.type]?.fields?.[c];
    if (field) return formatValue(field, e.data[c]);
    const v = e.data[c];
    return typeof v === "object" && v !== null ? JSON.stringify(v) : String(v ?? "");
  }
</script>

<div class="page">
  <h1>{page}</h1>
  {#each blocks as block}
    <section class="block">
      <h2>{block.view}</h2>
      {#if Object.keys(block.aggregates).length > 0}
        <div class="aggregates">
          {#each Object.entries(block.aggregates) as [k, v]}<span class="agg">{k}: {String(v)}</span>{/each}
        </div>
      {/if}
      {#if block.layout === "checklist"}
        <ul class="checklist">
          {#each block.entities as e}
            <li><input type="checkbox" checked={e.data["완료"] === true} disabled /> {display(e, "내용") || display(e, "이름") || e.id}</li>
          {/each}
        </ul>
      {:else if block.layout === "card"}
        <div class="cards">
          {#each block.entities as e}
            <div class="card">
              {#each block.columns ?? Object.keys(e.data) as c}
                <div class="card-field">{c}: {display(e, c)}</div>
              {/each}
            </div>
          {/each}
        </div>
      {:else}
        <table>
          <thead><tr>{#each block.columns ?? [] as c}<th>{c}</th>{/each}</tr></thead>
          <tbody>
            {#each block.entities as e}
              <tr>{#each block.columns ?? [] as c}<td>{display(e, c)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/each}
</div>
