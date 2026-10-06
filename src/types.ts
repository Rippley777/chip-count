export type Provider = 'claude' | 'codex';
export type Page =
  | 'Live'
  | 'Sessions'
  | 'Analytics'
  | 'Projects'
  | 'Compare'
  | 'Budgets & Limits'
  | 'Sources'
  | 'Settings';
export interface Filter {
  period?: string;
  search?: string;
  provider?: string;
  profile?: string;
  project?: string;
  model?: string;
  state?: string;
  from?: string;
  to?: string;
  tag?: string;
  min_tokens?: number;
  max_tokens?: number;
  min_cost?: number;
  max_cost?: number;
  pinned?: boolean;
  sort?: string;
  offset?: number;
  limit?: number;
}
export interface Usage {
  unknown_fields?: string[];
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  reasoning: number;
  total: number;
  cost: number;
  unpriced_tokens: number;
  inferred_price_tokens?: number;
  events: number;
}
export interface Session {
  id: string;
  parent_id: string | null;
  name: string;
  project: string;
  project_path: string;
  provider: Provider;
  profile: string;
  models: string[];
  state: 'active' | 'idle' | 'completed';
  first_at: string;
  last_at: string;
  usage: Usage;
  combined_usage: Usage;
  subagents: number;
  pinned: boolean;
  tags: string[];
  notes: string;
  sparkline: number[];
  source_path: string;
  elapsed_seconds: number;
  active_seconds: number;
  warnings: string[];
}
export interface Bucket extends Usage {
  x?: number;
  key: string;
  label: string;
}
export interface Source {
  id: string;
  provider: Provider;
  label: string;
  path: string;
  enabled: boolean;
  exclusions: string[];
  status: string;
  files: number;
  recognized: number;
  ignored: number;
  warnings: number;
  diagnostics?: { path: string; line: number; category: string; message: string }[];
  recovery_backup?: string;
  last_read: string | null;
  last_activity: string | null;
  message: string | null;
}
export interface Project {
  path: string;
  name: string;
  color: string;
  notes: string;
  favorite: boolean;
  aliases: string[];
  usage: Usage;
  sessions: number;
  models: string[];
  sparkline: number[];
}
export interface Budget {
  id: string;
  name: string;
  amount: number;
  unit: 'usd' | 'tokens';
  period: 'day' | 'month' | '5h' | 'window';
  window_minutes?: number;
  project: string | null;
  threshold: number;
  used: number;
  percentage: number;
  projected_at: string | null;
  sample_minutes: number;
  unpriced_tokens: number;
  unknown_fields: string[];
}
export interface Alert {
  id: string;
  budget_id: string;
  message: string;
  at: string;
}
export interface Price {
  model: string;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  version: string;
  source: string;
  retrieved_at: string;
  override: boolean;
  inferred?: boolean;
}
export interface Settings {
  theme: 'dark' | 'light' | 'system';
  density: 'comfortable' | 'compact';
  timezone: string;
  inactivity_minutes: number;
  close_to_tray: boolean;
  launch_at_login: boolean;
  notifications: boolean;
  retention_days: number;
  redact_paths: boolean;
  redact_labels: boolean;
  monthly_subscription: number | null;
}
export interface Limit {
  provider: string;
  scope: string;
  used_percent: number | null;
  resets_at: string | null;
  observed_at: string;
  window_minutes: number | null;
}
export interface ReportingRange {
  as_of: string;
  timezone: string;
  period: string;
  week_start: string;
  from: string | null;
  to: string | null;
  from_local: string | null;
  to_local: string | null;
  observed_from: string | null;
  observed_to: string | null;
  comparison: string | null;
  previous_from: string | null;
  previous_to: string | null;
  prior_complete_to: string | null;
}
export interface Snapshot {
  reporting: ReportingRange;
  top_sessions: Session[];
  previous_complete: Usage | null;
  sessions: Session[];
  total_sessions: number;
  totals: Usage;
  today: Usage;
  recent_rate: number;
  active_sessions: number;
  sources: Source[];
  projects: Project[];
  daily: Bucket[];
  models: Bucket[];
  providers: Bucket[];
  hours: Bucket[];
  weekdays: Bucket[];
  previous: Usage | null;
  budgets: Budget[];
  alerts: Alert[];
  prices: Price[];
  settings: Settings;
  limits: Limit[];
  indexed_at: string;
  warnings: string[];
  demo: boolean;
}
export interface UsageEvent {
  ingested_at?: string;
  id: string;
  session_id: string;
  timestamp: string;
  model: string;
  usage: Usage;
  scope: string;
  kind: string;
  request_id: string | null;
  turn_id: string | null;
  source_path: string;
  source_line: number;
  parser_version: string;
  pricing_version: string | null;
  reported: boolean;
  warnings: string[];
  duration_ms: number | null;
}
export interface Marker {
  timestamp: string;
  kind: string;
  label: string;
}
export interface Detail {
  session: Session;
  events: UsageEvent[];
  event_count: number;
  event_matches: number;
  event_offset: number;
  timeline: Bucket[];
  models: Bucket[];
  relationships: Session[];
  markers: Marker[];
  context: { used: number | null; capacity: number | null; observed_at: string | null };
  carry_in: Usage;
  request_duration_ms: number | null;
  coverage: string[];
}
export interface ComparisonItem {
  id?: string;
  label: string;
  usage: Usage;
  elapsed_seconds: number;
  active_seconds: number;
  models: string[];
  timeline: Bucket[];
}
export interface ExportResult {
  content: string;
  filename: string;
  mime: string;
}
export interface Commands {
  snapshot: { args: { filter?: Filter }; result: Snapshot };
  session: {
    args: { id: string; filter?: Filter; event_search?: string; event_offset?: number };
    result: Detail;
  };
  annotate: {
    args: { id: string; alias?: string; notes?: string; tags?: string[]; pinned?: boolean };
    result: { ok: boolean };
  };
  source_save: {
    args: {
      selection_id?: string;
      id?: string;
      provider: Provider;
      label: string;
      path: string;
      enabled: boolean;
      exclusions?: string[];
    };
    result: { ok: boolean };
  };
  source_remove: { args: { id: string }; result: { ok: boolean } };
  rescan: { args: { rebuild?: boolean }; result: { ok: boolean; backup?: string } };
  index_retry: { args: Record<string, never>; result: { ok: boolean; backup?: string } };
  index_preserve: { args: Record<string, never>; result: { ok: boolean; backup: string } };
  settings_save: { args: { settings: Partial<Settings> }; result: { ok: boolean } };
  budget_save: {
    args: {
      id?: string;
      name: string;
      amount: number;
      unit: 'usd' | 'tokens';
      period: 'day' | 'month' | '5h' | 'window';
      window_minutes?: number;
      project?: string;
      threshold: number;
    };
    result: { ok: boolean };
  };
  budget_remove: { args: { id: string }; result: { ok: boolean } };
  project_save: {
    args: {
      path: string;
      name?: string;
      color?: string;
      notes?: string;
      favorite?: boolean;
      aliases?: string[];
    };
    result: { ok: boolean };
  };
  pricing_save: {
    args: { model: string; input: number; output: number; cache_read: number; cache_write: number };
    result: { ok: boolean };
  };
  reprice: { args: Record<string, never>; result: { ok: boolean } };
  export: {
    args: {
      format: 'csv' | 'json';
      filter?: Filter;
      session_ids?: string[];
      redact_paths: boolean;
      redact_labels: boolean;
    };
    result: ExportResult;
  };
  compare: {
    args: {
      ids?: string[];
      ranges?: { from: string; to: string }[];
      filter?: Filter;
      alignment?: 'elapsed' | 'wall';
    };
    result: { items: ComparisonItem[] };
  };
}
