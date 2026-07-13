<script lang="ts">
  import type { PageBlock } from "./api";
  import type { ResolvedField, SchemaMap } from "./types";
  import { navigate } from "./router.svelte";
  import Chart from "./widgets/Chart.svelte";
  import ChecklistWidget from "./widgets/ChecklistWidget.svelte";
  import ProfileView from "./widgets/ProfileView.svelte";
  import QuickRecordWidget from "./widgets/QuickRecordWidget.svelte";
  import ValueCell from "./ValueCell.svelte";

  let { page, blocks, schemas, onedit }: { page: string; blocks: PageBlock[]; schemas: SchemaMap; onedit?: () => void } = $props();

  const textField: ResolvedField = { kind: "text", required: false };

  function browseUrl(block: PageBlock): string {
    const params = new URLSearchParams();
    for (const [field, cond] of Object.entries(block.filter ?? {})) {
      if (cond !== null && typeof cond === "object") {
        const [op, v] = Object.entries(cond as Record<string, unknown>)[0] ?? [];
        if (op) params.set(field, `${op}:${String(v)}`);
      } else {
        params.set(field, String(cond));
      }
    }
    if (block.sort) params.set("sort", block.sort);
    const q = params.toString();
    return `/browse/${encodeURIComponent(block.source)}${q ? `?${q}` : ""}`;
  }

  function cardColumns(data: Record<string, unknown>, columns?: string[] | null): string[] {
    return columns ?? Object.keys(data).filter((key) => !key.startsWith("$"));
  }
</script>

<div class="page">
  <div class="page-head">
    <h1>{page}</h1>
    {#if onedit}<button type="button" class="edit-page" onclick={onedit}>편집</button>{/if}
  </div>
  {#each blocks as block}
    <section class="block">
      {#if block.layout === "checklist" || block.layout === "profile"}
        <h2>{block.view}</h2>
      {:else}
        <h2><a href={browseUrl(block)} onclick={(e) => { e.preventDefault(); navigate(browseUrl(block)); }}>{block.view} ›</a></h2>
      {/if}
      {#if Object.keys(block.aggregates).length > 0}
        <div class="aggregates">
          {#each Object.entries(block.aggregates) as [k, v]}<span class="agg">{k}: {String(v)}</span>{/each}
        </div>
      {/if}
      {#if block.layout === "checklist"}
        <ChecklistWidget block={block} schemas={schemas} />
      {:else if block.layout === "profile"}
        <ProfileView block={block} schemas={schemas} />
      {:else if block.layout === "chart"}
        <Chart series={block.chart ?? []} chartType={block.chart_type === "bar" ? "bar" : "line"} />
      {:else if block.layout === "record"}
        <QuickRecordWidget block={block} schemas={schemas} />
      {:else if block.layout === "card"}
        <div class="cards">
          {#each block.entities as e}
            <div class="card">
              {#each cardColumns(e.data, block.columns) as c}
                <div class="card-field">{c}: <ValueCell field={schemas[e.type]?.fields?.[c] ?? textField} value={e.data[c]} schemas={schemas} entity={e} fieldName={c} /></div>
              {/each}
            </div>
          {/each}
        </div>
      {:else}
        <table>
          <thead><tr>{#each block.columns ?? [] as c}<th>{c}</th>{/each}</tr></thead>
          <tbody>
            {#each block.entities as e}
              <tr>{#each block.columns ?? [] as c}<td><ValueCell field={schemas[e.type]?.fields?.[c] ?? textField} value={e.data[c]} schemas={schemas} entity={e} fieldName={c} /></td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/each}
</div>
