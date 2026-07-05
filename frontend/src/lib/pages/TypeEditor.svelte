<script lang="ts">
  import type { Category, SchemaMap, SchemasResponse, SchemaWriteBody } from "../types";
  import type { EditorRow } from "../FieldRowEditor.svelte";
  import FieldRowEditor from "../FieldRowEditor.svelte";
  import { createSchema, deleteSchema, getSchemaRaw, getSchemas, updateSchema } from "../api";
  import { navigate } from "../router.svelte";

  let {
    schemas,
    categories,
    mode,
    type,
    onreloaded,
  }: {
    schemas: SchemaMap;
    categories: Category[];
    mode: "new" | "edit";
    type?: string;
    onreloaded: (r: SchemasResponse) => void;
  } = $props();

  let name = $state("");
  let category = $state("");
  let extendsType = $state("");
  let rows = $state<EditorRow[]>([]);
  let inherited = $state<Record<string, { kind: string }>>({});
  let recurrenceOn = $state(false);
  let recFlag = $state("");
  let recRule = $state("");
  let recDate = $state("");
  let error = $state<string | null>(null);
  let pendingWarnings = $state<string[] | null>(null);
  let loaded = $state(false);
  let seq = 0;

  $effect(() => {
    if (mode === "new" && !loaded) {
      name = "";
      loaded = true;
    }
  });

  function nextId() {
    return `row-${seq++}`;
  }

  function rowFrom(
    fieldName: string,
    def: { kind: string; required?: boolean; options?: string[] | null; target?: string | null; unit?: string | null },
  ): EditorRow {
    return {
      localId: nextId(),
      originalName: fieldName,
      name: fieldName,
      kind: def.kind,
      required: def.required ?? false,
      options: def.options ?? [],
      target: def.target ?? null,
      unit: def.unit ?? null,
    };
  }

  $effect(() => {
    if (mode !== "edit" || !type || loaded) return;
    getSchemaRaw(type)
      .then((raw) => {
        name = raw.type;
        category = raw.category ?? "";
        extendsType = raw.extends ?? "";
        rows = Object.entries(raw.fields).map(([fieldName, def]) => rowFrom(fieldName, def));
        inherited = raw.inherited;
        const rec = raw.behaviors?.recurrence;
        if (rec) {
          recurrenceOn = true;
          recFlag = rec.flag;
          recRule = rec.rule;
          recDate = rec.date;
        }
        loaded = true;
      })
      .catch((err) => {
        error = err instanceof Error ? err.message : "로드 실패";
        loaded = true;
      });
  });

  $effect(() => {
    if (mode !== "new") return;
    inherited = extendsType && schemas[extendsType] ? schemas[extendsType].fields : {};
  });

  function addRow() {
    rows = [
      ...rows,
      {
        localId: nextId(),
        originalName: null,
        name: "",
        kind: "text",
        required: false,
        options: [],
        target: null,
        unit: null,
      },
    ];
  }

  function updateRow(index: number, row: EditorRow) {
    rows = rows.map((item, i) => (i === index ? row : item));
  }

  function removeRow(index: number) {
    rows = rows.filter((_, i) => i !== index);
  }

  function moveRow(index: number, dir: -1 | 1) {
    const nextIndex = index + dir;
    if (nextIndex < 0 || nextIndex >= rows.length) return;
    const next = [...rows];
    [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
    rows = next;
  }

  function buildBody(): SchemaWriteBody {
    const fields: SchemaWriteBody["fields"] = {};
    const renames: Record<string, string> = {};
    for (const row of rows) {
      const fieldName = row.name.trim();
      if (!fieldName) continue;
      fields[fieldName] = {
        kind: row.kind,
        required: row.required,
        ...(row.options.length > 0 ? { options: row.options } : {}),
        ...(row.target ? { target: row.target } : {}),
        ...(row.unit ? { unit: row.unit } : {}),
      };
      if (row.originalName && row.originalName !== fieldName) renames[row.originalName] = fieldName;
    }

    const body: SchemaWriteBody = {
      category: category || null,
      fields,
      field_order: rows.map((row) => row.name.trim()).filter(Boolean),
    };
    if (mode === "new") {
      body.type = name.trim();
      if (extendsType) body.extends = extendsType;
    }
    if (Object.keys(renames).length > 0) body.renames = renames;
    if (recurrenceOn) body.behaviors = { recurrence: { flag: recFlag, rule: recRule, date: recDate } };
    return body;
  }

  async function refreshAndGo(path: string) {
    const fresh = await getSchemas();
    onreloaded(fresh);
    navigate(path);
  }

  async function doSave() {
    error = null;
    const body = buildBody();
    try {
      if (mode === "new") {
        await createSchema(body);
        await refreshAndGo(`/browse/${encodeURIComponent(body.type ?? name)}`);
        return;
      }

      const impact = await updateSchema(name, body, { dryRun: true });
      if ("warnings" in impact && impact.warnings.length > 0 && pendingWarnings === null) {
        pendingWarnings = impact.warnings;
        return;
      }
      await updateSchema(name, body);
      pendingWarnings = null;
      await refreshAndGo(`/browse/${encodeURIComponent(name)}`);
    } catch (err) {
      error = err instanceof Error ? err.message : "저장 실패";
    }
  }

  async function confirmSave() {
    try {
      await updateSchema(name, buildBody());
      pendingWarnings = null;
      await refreshAndGo(`/browse/${encodeURIComponent(name)}`);
    } catch (err) {
      error = err instanceof Error ? err.message : "저장 실패";
    }
  }

  async function doDelete() {
    try {
      await deleteSchema(name);
      const fresh = await getSchemas();
      onreloaded(fresh);
      navigate("/");
    } catch (err) {
      error = err instanceof Error ? err.message : "삭제 실패";
    }
  }
</script>

<section class="type-editor">
  <h1>{mode === "new" ? "새 타입" : `${name} 설정`}</h1>

  {#if !loaded}
    <p>불러오는 중…</p>
  {:else}
    <div class="basic">
      {#if mode === "new"}
        <label>
          타입명
          <input aria-label="타입명" bind:value={name} />
        </label>
        <label>
          부모 타입
          <select aria-label="부모 타입" bind:value={extendsType}>
            <option value="">(없음)</option>
            {#each Object.keys(schemas) as schemaName (schemaName)}
              <option value={schemaName}>{schemaName}</option>
            {/each}
          </select>
        </label>
      {:else}
        <label>
          타입명
          <input aria-label="타입명" value={name} disabled />
        </label>
        {#if extendsType}<p class="parent">부모: {extendsType} (변경 불가)</p>{/if}
      {/if}

      <label>
        카테고리
        <select aria-label="카테고리" bind:value={category}>
          <option value="">(없음)</option>
          {#each categories as cat (cat.name)}
            <option value={cat.name}>{cat.name}</option>
          {/each}
        </select>
      </label>
    </div>

    {#if Object.keys(inherited).length > 0}
      <section class="inherited">
        <h2>상속 필드</h2>
        <ul>
          {#each Object.entries(inherited) as [fieldName, field] (fieldName)}
            <li>{fieldName} <span>{field.kind}</span></li>
          {/each}
        </ul>
      </section>
    {/if}

    <section class="fields">
      <h2>필드</h2>
      {#each rows as row, index (row.localId)}
        <FieldRowEditor
          {row}
          {schemas}
          onchange={(next) => updateRow(index, next)}
          onremove={() => removeRow(index)}
          onmove={(dir) => moveRow(index, dir)}
        />
      {/each}
      <button type="button" onclick={addRow}>+ 필드</button>
    </section>

    <section class="recurrence">
      <label><input type="checkbox" bind:checked={recurrenceOn} /> 반복 사용</label>
      {#if recurrenceOn}
        <label>완료 플래그 <input aria-label="rec-flag" bind:value={recFlag} /></label>
        <label>반복 규칙 <input aria-label="rec-rule" bind:value={recRule} /></label>
        <label>기준일 <input aria-label="rec-date" bind:value={recDate} /></label>
      {/if}
    </section>

    {#if error}<p class="error">{error}</p>{/if}

    {#if pendingWarnings}
      <div class="impact-modal" role="dialog" aria-label="영향 확인">
        <p>다음 영향이 있습니다.</p>
        <ul>
          {#each pendingWarnings as warning}
            <li>{warning}</li>
          {/each}
        </ul>
        <button type="button" onclick={confirmSave}>계속</button>
        <button type="button" onclick={() => (pendingWarnings = null)}>취소</button>
      </div>
    {/if}

    <div class="actions">
      <button type="button" onclick={doSave}>저장</button>
      {#if mode === "edit"}
        <button type="button" class="danger" onclick={doDelete}>타입 삭제</button>
      {/if}
    </div>
  {/if}
</section>
