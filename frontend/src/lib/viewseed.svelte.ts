import type { ViewBlockDef } from "./api";

const OPS = ["lte", "gte", "lt", "gt", "month"] as const;

/** Browse URL 값 하나를 필터 조건으로 변환. "gte:200000" → {gte:200000}, 숫자면 숫자, 그 외 문자열. */
export function conditionFromParam(raw: string): unknown {
  for (const op of OPS) {
    const prefix = `${op}:`;
    if (raw.startsWith(prefix)) {
      return { [op]: coerce(raw.slice(prefix.length)) };
    }
  }
  return coerce(raw);
}

function coerce(v: string): unknown {
  if (v === "") return "";
  if (v === "true") return true;
  if (v === "false") return false;
  const n = Number(v);
  return Number.isNaN(n) ? v : n;
}

/** Browse의 (타입, URL 파라미터)를 단일 뷰 블록으로 변환("뷰로 저장" 진입점). */
export function blockFromBrowseParams(source: string, params: Record<string, string>): ViewBlockDef {
  const filter: Record<string, unknown> = {};
  let sort: string | null = null;
  for (const [k, v] of Object.entries(params)) {
    if (k === "sort") {
      sort = v;
      continue;
    }
    filter[k] = conditionFromParam(v);
  }
  return {
    view: source,
    source,
    filter: Object.keys(filter).length ? filter : null,
    sort,
    layout: "table",
  };
}

/** Browse → PageEditor 씨앗 블록 전달용 모듈 스토어(일회성 소비). */
export const pageSeed = $state<{ block: ViewBlockDef | null }>({ block: null });

export function setPageSeed(block: ViewBlockDef) {
  pageSeed.block = block;
}

export function takePageSeed(): ViewBlockDef | null {
  const b = pageSeed.block;
  pageSeed.block = null;
  return b;
}
