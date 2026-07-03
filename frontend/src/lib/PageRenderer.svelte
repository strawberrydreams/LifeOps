<script lang="ts">
  import type { PageBlock } from "./api";

  let { page, blocks }: { page: string; blocks: PageBlock[] } = $props();
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
            <li><input type="checkbox" checked={e.data["완료"] === true} disabled /> {String(e.data["내용"] ?? e.data["이름"] ?? e.id)}</li>
          {/each}
        </ul>
      {:else if block.layout === "card"}
        <div class="cards">
          {#each block.entities as e}
            <div class="card">
              {#each block.columns ?? Object.keys(e.data) as c}
                <div class="card-field">{c}: {String(e.data[c] ?? "")}</div>
              {/each}
            </div>
          {/each}
        </div>
      {:else}
        <table>
          <thead><tr>{#each block.columns ?? [] as c}<th>{c}</th>{/each}</tr></thead>
          <tbody>
            {#each block.entities as e}
              <tr>{#each block.columns ?? [] as c}<td>{String(e.data[c] ?? "")}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/each}
</div>
