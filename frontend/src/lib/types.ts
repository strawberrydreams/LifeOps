export interface ResolvedField {
  kind: string;
  required: boolean;
  options?: string[] | null;
  target?: string | null;
  unit?: string | null;
}

export interface ResolvedSchema {
  name: string;
  extends?: string | null;
  fields: Record<string, ResolvedField>;
}

export type SchemaMap = Record<string, ResolvedSchema>;

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
