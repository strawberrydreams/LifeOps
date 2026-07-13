<script lang="ts">
  import type { SchemaMap } from "../types";
  import { getPage, ApiError, type PageBlock } from "../api";
  import PageRenderer from "../PageRenderer.svelte";
  import { navigate } from "../router.svelte";

  let { schemas }: { schemas: SchemaMap } = $props();

  let blocks = $state<PageBlock[] | null>(null);
  let missing = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    getPage("홈")
      .then((p) => (blocks = p.blocks))
      .catch((e) => {
        if (e instanceof ApiError && e.status === 404) missing = true;
        else error = e instanceof Error ? e.message : "홈 로드 실패";
      });
  });
</script>

{#if error}
  <p class="error">{error}</p>
{:else if missing}
  <div class="home">
    <h1>LifeOps</h1>
    <p>왼쪽에서 타입을 선택하세요. (views/홈.yaml을 만들면 이 자리가 대시보드가 됩니다)</p>
    <ul>{#each Object.keys(schemas) as t}<li>{t}</li>{/each}</ul>
  </div>
{:else if blocks}
  <PageRenderer page="홈" blocks={blocks} schemas={schemas} onedit={() => navigate("/pages/홈/edit")} />
{:else}
  <p>불러오는 중…</p>
{/if}
