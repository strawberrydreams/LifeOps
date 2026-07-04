<script lang="ts">
  import type { SchemaMap } from "../types";
  let { schemas }: { schemas: SchemaMap } = $props();
</script>
<div class="home">
  <h1>LifeOps</h1>
  <p>왼쪽에서 타입을 선택하세요.</p>
  <ul>{#each Object.keys(schemas) as t}<li>{t}</li>{/each}</ul>
</div>
