<script module lang="ts">
  let nextProfileFieldPrefix = 0;
</script>

<script lang="ts">
  import type { PageBlock } from "../api";
  import type { Entity, SchemaMap } from "../types";
  import { ApiError, updateEntity } from "../api";
  import { navigate } from "../router.svelte";
  import ProvenanceTrigger from "../ProvenanceTrigger.svelte";
  import Widget from "./Widget.svelte";

  let { block, schemas }: { block: PageBlock; schemas: SchemaMap } = $props();

  const labelPrefix = `profile-field-label-${nextProfileFieldPrefix++}`;
  const schema = $derived(schemas[block.source]);
  const entity = $derived(block.entities[0] ?? null);
  const sections = $derived.by(() => {
    if (block.sections && block.sections.length > 0) return block.sections;
    return [{ title: block.view, fields: Object.keys(schema?.fields ?? {}) }];
  });

  let data = $state<Record<string, unknown>>({});
  let msg = $state<string | null>(null);

  $effect(() => {
    data = { ...(entity?.data ?? {}) };
    msg = null;
  });

  function set(name: string, value: unknown) {
    if (value === null || value === undefined || value === "") delete data[name];
    else data[name] = value;
  }

  function fieldLabelId(sectionIndex: number, fieldIndex: number) {
    return `${labelPrefix}-${sectionIndex}-${fieldIndex}`;
  }

  async function save() {
    if (!entity) return;
    try {
      const updated = await updateEntity(entity.id, { ...data });
      data = { ...updated.data };
      msg = "저장됨";
    } catch (err) {
      if (err instanceof ApiError) {
        const fieldMsgs = err.fields?.map((field) => `${field.field}: ${field.message}`).join(", ");
        msg = fieldMsgs ? `${err.message} (${fieldMsgs})` : err.message;
      } else {
        msg = "저장 실패";
      }
    }
  }
</script>

{#if !entity}
  <div class="profile-empty">
    <button type="button" onclick={() => navigate(`/new/${encodeURIComponent(block.source)}`)}>프로필 시작하기</button>
  </div>
{:else}
  <div class="profile">
    {#each sections as section, sectionIndex}
      <section class="profile-section">
        <h3>{section.title}</h3>
        <div class="profile-fields">
          {#each section.fields as name, fieldIndex}
            {@const field = schema?.fields?.[name]}
            {#if field}
              <div class="profile-field">
                <span>
                  <span id={fieldLabelId(sectionIndex, fieldIndex)}>{name}</span>
                  <ProvenanceTrigger {entity} fieldName={name} />
                </span>
                <Widget
                  field={field}
                  value={data[name]}
                  labelledby={fieldLabelId(sectionIndex, fieldIndex)}
                  onchange={(value) => set(name, value)}
                />
              </div>
            {/if}
          {/each}
        </div>
      </section>
    {/each}
    <div class="profile-actions">
      <button type="button" onclick={save}>저장</button>
      {#if msg}<span class="msg">{msg}</span>{/if}
    </div>
  </div>
{/if}
