<script lang="ts">
  import { getPage, ApiError, type PageBlock } from "../api";
  import type { SchemaMap } from "../types";
  import { navigate } from "../router.svelte";
  import PageRenderer from "../PageRenderer.svelte";

  let { pageName, schemas }: { pageName: string; schemas: SchemaMap } = $props();

  let blocks = $state<PageBlock[]>([]);
  let error = $state<string | null>(null);
  $effect(() => {
    getPage(pageName).then((p) => (blocks = p.blocks)).catch((e) => {
      error = e instanceof ApiError ? e.message : "페이지 로드 실패";
    });
  });
</script>

{#if error}
  <p class="error">{error}</p>
{:else}
  <PageRenderer page={pageName} blocks={blocks} schemas={schemas} onedit={() => navigate(`/pages/${encodeURIComponent(pageName)}/edit`)} />
{/if}
