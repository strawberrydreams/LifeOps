<script lang="ts">
  import type { Entity } from "./types";
  import { updateEntity } from "./api";
  import { resolveProvenance, formatTimestamp, type Sensitivity } from "./provenance";

  let {
    entity,
    fieldName,
    onupdate,
  }: {
    entity: Entity;
    fieldName: string;
    onupdate?: (e: Entity) => void;
  } = $props();

  let current = $state<Entity | null>(null);
  $effect(() => {
    current = entity;
  });

  let open = $state(false);
  const activeEntity = $derived(current ?? entity);
  const prov = $derived(resolveProvenance(activeEntity, fieldName));

  let source = $state("manual");
  let confidence = $state<string | number | null>("");
  let sensitivity = $state<Sensitivity>("normal");
  let validationError = $state("");

  function sync() {
    source = prov.source;
    confidence = prov.confidence === null ? "" : String(prov.confidence);
    sensitivity = prov.sensitivity;
    validationError = "";
  }

  function toggle() {
    if (!open) sync();
    open = !open;
  }

  function stopTriggerKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.stopPropagation();
    }
  }

  function stopPropagationBoundary(node: HTMLElement) {
    const stop = (e: Event) => e.stopPropagation();
    node.addEventListener("click", stop);
    node.addEventListener("pointerdown", stop);
    return {
      destroy() {
        node.removeEventListener("click", stop);
        node.removeEventListener("pointerdown", stop);
      },
    };
  }

  async function save() {
    const entry: Record<string, unknown> = { source, sensitivity };
    const confidenceText = confidence === null ? "" : String(confidence).trim();
    if (confidenceText === "") {
      entry.confidence = null;
    } else {
      const c = Number(confidenceText);
      if (!Number.isFinite(c) || c < 0 || c > 1) {
        validationError = "신뢰도는 0과 1 사이여야 합니다";
        return;
      }
      entry.confidence = c;
    }

    const updated = await updateEntity(activeEntity.id, { $meta: { [fieldName]: entry } });
    current = updated;
    onupdate?.(updated);
    open = false;
  }
</script>

<span class="prov">
  <button
    type="button"
    class="prov-trigger"
    aria-label="출처 정보"
    onkeydowncapture={stopTriggerKeydown}
    onclickcapture={(e) => {
      e.stopPropagation();
      toggle();
    }}>ⓘ</button
  >
  {#if open}
    <div
      class="prov-popover"
      role="dialog"
      tabindex="-1"
      use:stopPropagationBoundary
      onkeydown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          open = false;
        }
      }}
    >
      <label
        >출처
        <select bind:value={source}>
          {#if source !== "manual" && source !== "imported"}
            <option value={source}>{source}</option>
          {/if}
          <option value="manual">manual</option>
          <option value="imported">imported</option>
        </select>
      </label>
      <label
        >신뢰도
        <input type="number" min="0" max="1" step="0.1" bind:value={confidence} placeholder="-" />
      </label>
      {#if validationError}
        <div class="prov-error">{validationError}</div>
      {/if}
      <label
        >민감도
        <select bind:value={sensitivity}>
          <option value="normal">normal</option>
          <option value="sensitive">sensitive</option>
        </select>
      </label>
      <div class="prov-updated">갱신 {formatTimestamp(prov.updatedAt)}</div>
      <div class="prov-actions">
        <button
          type="button"
          onclickcapture={(e) => {
            e.stopPropagation();
            save();
          }}>저장</button
        >
        <button
          type="button"
          onclickcapture={(e) => {
            e.stopPropagation();
            open = false;
          }}>닫기</button
        >
      </div>
    </div>
  {/if}
</span>
