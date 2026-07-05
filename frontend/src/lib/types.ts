export interface ResolvedField {
  kind: string;
  required: boolean;
  options?: string[] | null;
  target?: string | null;
  unit?: string | null;
}

export interface RecurrenceDef {
  flag: string;
  rule: string;
  date: string;
}

export interface ResolvedSchema {
  name: string;
  extends?: string | null;
  category?: string | null;
  behaviors?: { recurrence?: RecurrenceDef | null } | null;
  fields: Record<string, ResolvedField>;
}

export type SchemaMap = Record<string, ResolvedSchema>;

export interface Category {
  name: string;
  icon?: string | null;
  description?: string | null;
}

export interface SchemasResponse {
  types: SchemaMap;
  categories: Category[];
}

export interface Entity {
  id: string;
  type: string;
  data: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface RefEdge {
  from_id: string;
  from_type: string;
  field_name: string;
}

export interface FieldErrorItem {
  field: string;
  message: string;
}

export interface ApiErrorBody {
  error: {
    code: string;
    message: string;
    fields?: FieldErrorItem[];
    referrers?: RefEdge[];
  };
}

export interface SchemaFieldInput {
  kind: string;
  required?: boolean;
  options?: string[];
  target?: string | null;
  unit?: string | null;
}

export interface SchemaWriteBody {
  type?: string;
  category?: string | null;
  extends?: string | null;
  behaviors?: { recurrence?: RecurrenceDef | null } | null;
  fields: Record<string, SchemaFieldInput>;
  field_order?: string[] | null;
  renames?: Record<string, string>;
}

export interface RawSchemaResponse {
  type: string;
  category?: string | null;
  extends?: string | null;
  behaviors?: { recurrence?: RecurrenceDef | null } | null;
  field_order?: string[] | null;
  fields: Record<string, SchemaFieldInput>;
  inherited: Record<string, ResolvedField>;
}

export interface DryRunResult {
  affected_entities: number;
  warnings: string[];
}
