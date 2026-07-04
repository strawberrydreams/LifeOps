import type { Entity, FieldErrorItem, RefEdge, SchemasResponse } from "./types";

interface ApiErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    fields?: FieldErrorItem[];
    referrers?: RefEdge[];
  };
}

export interface PageBlock {
  view: string;
  source: string;
  filter?: Record<string, unknown> | null;
  sort?: string | null;
  layout: "table" | "checklist" | "card";
  columns?: string[] | null;
  entities: Entity[];
  aggregates: Record<string, unknown>;
}

export type UpdatedEntity = Entity & { spawned?: Entity; recurrence_warning?: string };

export class ApiError extends Error {
  code: string;
  status: number;
  fields?: FieldErrorItem[];
  referrers?: RefEdge[];

  constructor(status: number, code: string, message: string, extra?: { fields?: FieldErrorItem[]; referrers?: RefEdge[] }) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.fields = extra?.fields;
    this.referrers = extra?.referrers;
  }
}

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    headers: body !== undefined ? { "content-type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (res.status === 204) {
    return undefined as T;
  }

  let json: unknown;
  try {
    json = await res.json();
  } catch {
    json = undefined;
  }

  if (!res.ok) {
    const error = (json as ApiErrorEnvelope | undefined)?.error;
    throw new ApiError(res.status, error?.code ?? "unknown", error?.message ?? `HTTP ${res.status}`, {
      fields: error?.fields,
      referrers: error?.referrers,
    });
  }

  return json as T;
}

export function getSchemas(): Promise<SchemasResponse> {
  return request("GET", "/api/schemas");
}

export function listEntities(type: string, params: Record<string, string> = {}): Promise<Entity[]> {
  const query = new URLSearchParams(type ? { type, ...params } : params);
  return request("GET", `/api/entities?${query.toString()}`);
}

export function createEntity(type: string, data: Record<string, unknown>): Promise<Entity> {
  return request("POST", "/api/entities", { type, data });
}

export function getEntity(id: string): Promise<{ entity: Entity; backlinks: RefEdge[] }> {
  return request("GET", `/api/entities/${encodeURIComponent(id)}`);
}

export function updateEntity(id: string, patch: Record<string, unknown>): Promise<UpdatedEntity> {
  return request("PATCH", `/api/entities/${encodeURIComponent(id)}`, patch);
}

export function deleteEntity(id: string): Promise<void> {
  return request("DELETE", `/api/entities/${encodeURIComponent(id)}`);
}

export function getPage(name: string): Promise<{ page: string; blocks: PageBlock[] }> {
  return request("GET", `/api/pages/${encodeURIComponent(name)}`);
}

export function getExport(): Promise<Record<string, Entity[]>> {
  return request("GET", "/api/export");
}

export function reload(): Promise<{ ok: boolean }> {
  return request("POST", "/api/reload");
}
