import {
  useState,
  useEffect,
  useRef,
  useCallback,
  useMemo,
  createContext,
  useContext,
  type ReactNode,
  type FormEvent,
  type CSSProperties,
} from 'react';
import {
  Activity,
  ArrowDown,
  ArrowDownToLine,
  ArrowRight,
  ArrowUpRight,
  Bell,
  Check,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Clipboard,
  Columns3,
  Command,
  Database,
  FileJson,
  Filter as FilterIcon,
  Folder,
  FolderOpen,
  GitBranch,
  Layers3,
  LayoutGrid,
  ListFilter,
  LoaderCircle,
  Maximize2,
  Moon,
  Pause,
  Play,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Star,
  Sun,
  Tag,
  Trash2,
  Wallet,
  X,
  Zap,
  ChartNoAxesCombined,
  PanelRightClose,
  Pencil,
  Download,
  CircleAlert,
  CheckCircle2,
  Monitor,
  Network,
} from 'lucide-react';
import {
  AreaChart,
  Area,
  BarChart,
  Bar,
  CartesianGrid,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Brush,
  LineChart,
  Line,
} from 'recharts';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useReactTable, getCoreRowModel, flexRender, type ColumnDef } from '@tanstack/react-table';
import * as Dialog from '@radix-ui/react-dialog';
import {
  api,
  userError,
  restoreIndex,
  type UserFacingError,
  pickPath,
  saveExport,
  revealPath,
  setMonitoring,
  openCompact,
  isDesktop,
  isAppStore,
  getMonitoring,
} from './api';
import type {
  Page,
  Filter,
  Session,
  Snapshot,
  Detail,
  Usage,
  Bucket,
  Source,
  Settings,
  Budget,
  Project,
  Price,
  ComparisonItem,
  UsageEvent,
  Commands,
} from './types';

const PAGES: { name: Page; icon: typeof Activity; key: string }[] = [
  { name: 'Live', icon: Radio, key: '1' },
  { name: 'Sessions', icon: Layers3, key: '2' },
  { name: 'Analytics', icon: ChartNoAxesCombined, key: '3' },
  { name: 'Projects', icon: Folder, key: '4' },
  { name: 'Compare', icon: Columns3, key: '5' },
  { name: 'Budgets & Limits', icon: Wallet, key: '6' },
  { name: 'Sources', icon: Database, key: '7' },
  { name: 'Settings', icon: Settings2, key: '8' },
];
const CHART_COLORS = ['#dfab70', '#8fc6b4', '#a7a4db', '#728caa', '#c99b94'];
const ZERO: Usage = {
  input: 0,
  output: 0,
  cache_read: 0,
  cache_write: 0,
  reasoning: 0,
  total: 0,
  cost: 0,
  unpriced_tokens: 0,
  events: 0,
};
const n = (value: number) => new Intl.NumberFormat('en-US').format(value || 0);
const compact = (value: number) =>
  new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 }).format(
    value || 0,
  );
const money = (value: number, precision = 2) =>
  '$' +
  (value || 0).toLocaleString('en-US', {
    minimumFractionDigits: precision,
    maximumFractionDigits: precision,
  });
const usageCost = (u: Usage, precision = 2) =>
  u.unpriced_tokens > 0 && u.unpriced_tokens === u.total
    ? 'Unpriced'
    : (u.inferred_price_tokens ? '≈' : '') +
      money(u.cost, precision) +
      (u.unpriced_tokens ? '*' : '');
const costCoverage = (u: Usage, complete: string) =>
  u.unpriced_tokens
    ? compact(u.unpriced_tokens) +
      ' tokens unpriced' +
      (u.inferred_price_tokens ? ' · auto-review estimates' : '')
    : u.inferred_price_tokens
      ? 'Includes auto-review estimates · model inferred'
      : complete;
const category = (
  u: Usage,
  key: 'input' | 'output' | 'cache_read' | 'cache_write' | 'reasoning',
) => (u.unknown_fields?.includes(key) ? (u[key] ? '≥ ' + n(u[key]) : 'Unavailable') : n(u[key]));
let displayTimezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
const time = (
  value: string | null,
  options: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit' },
) =>
  value && !Number.isNaN(Date.parse(value))
    ? new Date(value).toLocaleString('en-US', { ...options, timeZone: displayTimezone })
    : 'Unavailable';
const fmtDate = (value: string) =>
  time(value, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    timeZoneName: 'short',
  });
const date = (value: string) => time(value, { month: 'short', day: 'numeric' });
const elapsed = (s: number) =>
  s >= 3600
    ? Math.floor(s / 3600) + 'h ' + Math.floor((s % 3600) / 60) + 'm'
    : Math.floor(s / 60) + 'm';
const ago = (value: string | null) => {
  if (!value) return 'Not yet';
  const seconds = Math.max(0, (Date.now() - Date.parse(value)) / 1000);
  return seconds < 60
    ? 'Just now'
    : seconds < 3600
      ? Math.floor(seconds / 60) + 'm ago'
      : seconds < 86400
        ? Math.floor(seconds / 3600) + 'h ago'
        : date(value);
};
const modelName = (m: string) =>
  m
    .replace(/^claude-/, '')
    .replace(/-20\d{6}$/, '')
    .replace(/^gpt-/, 'GPT-')
    .replace(/-/g, ' ');
const providerName = (p: string) => (p === 'claude' ? 'Claude Code' : p === 'codex' ? 'Codex' : p);
function useStored<T>(key: string, initial: T): [T, (next: T | ((value: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const item = localStorage.getItem('chip-count:' + key);
      return item ? JSON.parse(item) : initial;
    } catch {
      return initial;
    }
  });
  const update = useCallback(
    (next: T | ((value: T) => T)) =>
      setValue((old) => {
        const result = typeof next === 'function' ? (next as (value: T) => T)(old) : next;
        localStorage.setItem('chip-count:' + key, JSON.stringify(result));
        return result;
      }),
    [key],
  );
  return [value, update];
}
function IconButton({
  children,
  label,
  onClick,
  className = '',
  disabled = false,
}: {
  children: ReactNode;
  label: string;
  onClick?: () => void;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <button
      className={'icon-button ' + className}
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
function Badge({ children, tone = 'neutral' }: { children: ReactNode; tone?: string }) {
  return <span className={'badge ' + tone}>{children}</span>;
}
function ProviderIcon({ provider }: { provider: string }) {
  return (
    <span className={'provider-icon ' + provider}>
      {provider === 'claude' ? <Sparkles size={14} /> : <Command size={13} />}
    </span>
  );
}
function State({ value }: { value: string }) {
  return (
    <span className={'state ' + value}>
      <i />
      {value === 'active' ? 'Active' : value === 'completed' ? 'Completed' : 'Idle'}
    </span>
  );
}
function Sparkline({
  values,
  color = 'var(--mint)',
  width = 94,
  height = 26,
}: {
  values: number[];
  color?: string;
  width?: number;
  height?: number;
}) {
  values = values.length > 1 ? values : [0, 0];
  const max = Math.max(1, ...values);
  const points = values
    .map(
      (v, i) =>
        (i / (values.length - 1 || 1)) * width + ',' + (height - 3 - (v / max) * (height - 6)),
    )
    .join(' ');
  return (
    <svg
      className="sparkline"
      width={width}
      height={height}
      viewBox={'0 0 ' + width + ' ' + height}
      role="img"
      aria-label="Recent usage trend"
    >
      <path
        d={
          'M0 ' + height + ' L' + points.replaceAll(' ', ' L') + ' L' + width + ' ' + height + ' Z'
        }
        fill={color}
        opacity=".07"
      />
      <polyline
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        points={points}
      />
    </svg>
  );
}
function Empty({
  icon: Icon = Layers3,
  title,
  description,
  action,
}: {
  icon?: typeof Activity;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <span className="empty-icon">
        <Icon size={25} />
      </span>
      <h3>{title}</h3>
      <p>{description}</p>
      {action}
    </div>
  );
}
const FailureContext = createContext<{
  error: UserFacingError | null;
  clear: () => void;
  setError: (error: UserFacingError | null) => void;
}>({
  error: null,
  clear: () => {},
  setError: () => {},
});
function ErrorNotice({ error, clear }: { error: UserFacingError; clear?: () => void }) {
  return (
    <div className="error-notice" role="alert">
      <strong>{error.data.message}</strong>
      <p>{error.data.next_steps}</p>
      {error.data.diagnostics && (
        <details>
          <summary>Diagnostics</summary>
          <pre>{error.data.diagnostics}</pre>
        </details>
      )}
      {clear && (
        <button className="text-button" onClick={clear}>
          Dismiss error
        </button>
      )}
    </div>
  );
}
function Modal({
  open,
  onClose,
  title,
  description,
  children,
  wide = false,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  wide?: boolean;
}) {
  const opener = useRef<HTMLElement | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const failure = useContext(FailureContext);
  return (
    <Dialog.Root open={open} onOpenChange={(v) => !v && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content
          ref={contentRef}
          className={'dialog ' + (wide ? 'wide' : '')}
          onOpenAutoFocus={(e) => {
            opener.current =
              document.activeElement instanceof HTMLElement ? document.activeElement : null;
            failure.clear();
            const input = contentRef.current?.querySelector<HTMLElement>(
              'input:not([type="checkbox"]), select, textarea',
            );
            if (input) {
              e.preventDefault();
              input.focus();
            }
          }}
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            const target = opener.current?.isConnected
              ? opener.current
              : document.querySelector<HTMLElement>('.nav-item.active, .session-row-main');
            target?.focus();
          }}
        >
          {failure.error && <ErrorNotice error={failure.error} clear={failure.clear} />}
          <div className="dialog-heading">
            <div>
              <Dialog.Title>{title}</Dialog.Title>
              <Dialog.Description>
                {description || 'Chip Count · Local session analytics'}
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <IconButton label="Close dialog">
                <X size={17} />
              </IconButton>
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}
function Toggle({
  value,
  onChange,
  label,
  description,
  disabled = false,
}: {
  value: boolean;
  onChange: (v: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        {description && <p>{description}</p>}
      </div>
      <button
        disabled={disabled}
        className={'toggle ' + (value ? 'on' : '')}
        role="switch"
        aria-checked={value}
        aria-label={label}
        onClick={() => onChange(!value)}
      >
        <span />
      </button>
    </div>
  );
}
function ChartTooltip({
  active,
  payload,
  label,
  formatLabel,
}: {
  active?: boolean;
  payload?: {
    color?: string;
    name?: string | number;
    value?: number | string | (number | string)[];
    payload?: Partial<Usage>;
  }[];
  label?: string | number;
  formatLabel?: (value: string | number) => string;
}) {
  if (!active || !payload?.length) return null;
  return (
    <div className="chart-tooltip">
      <strong>{formatLabel ? formatLabel(label ?? '') : label}</strong>
      {payload.map((p, i) => (
        <div key={i}>
          <i style={{ background: p.color }} />
          <span>{p.name}</span>
          <b>
            {String(p.name).toLowerCase().includes('cost')
              ? money(Number(p.value), 4)
              : n(Number(p.value)) +
                (String(p.name).includes('/ minute') ? ' tokens/min' : ' tokens')}
          </b>
        </div>
      ))}
      {payload.some((p) => String(p.name).toLowerCase().includes('cost')) &&
        payload[0]?.payload && (
          <p className="small-note">
            {payload[0].payload.unpriced_tokens ||
            payload[0].payload.unknown_fields?.some((k) => k !== 'reasoning')
              ? 'Partial estimate · '
              : ''}
            {costCoverage({ ...ZERO, ...payload[0].payload }, 'Known prices for observed events')}
          </p>
        )}
    </div>
  );
}
function UsageChart({
  data,
  height = 190,
  onClick,
  cost = false,
  stacked = false,
  onInterval,
  rate = false,
}: {
  data: Bucket[];
  height?: number;
  onClick?: (key: string) => void;
  cost?: boolean;
  stacked?: boolean;
  rate?: boolean;
  onInterval?: (from: string, to: string) => void;
}) {
  const [bucketKey, setBucketKey] = useState('');
  const [intervalStart, setIntervalStart] = useState('');
  const [intervalEnd, setIntervalEnd] = useState('');
  const timeAxis = data.length > 0 && data.every((b) => typeof b.x === 'number');
  const timeLabel = (x: number) =>
    data.find((b) => b.x === x)?.label ||
    (data[0]?.key.endsWith('m')
      ? Math.round(x / 60000) + ' min'
      : data[0]?.key.length === 10
        ? new Date(x).toISOString().slice(0, 10)
        : fmtDate(new Date(x).toISOString()));
  const coverage = data.reduce(
    (u, b) => ({
      ...u,
      total: u.total + b.total,
      unpriced_tokens: u.unpriced_tokens + b.unpriced_tokens,
      inferred_price_tokens: (u.inferred_price_tokens || 0) + (b.inferred_price_tokens || 0),
      unknown_fields: [...new Set([...(u.unknown_fields || []), ...(b.unknown_fields || [])])],
    }),
    { ...ZERO },
  );
  return (
    <div>
      <div className="chart" style={{ height }}>
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart
            data={data}
            margin={{ top: 15, right: 8, left: -14, bottom: 0 }}
            onClick={(e) => {
              const index = Number(e?.activeTooltipIndex);
              const bucket = Number.isFinite(index)
                ? data[index]
                : data.find((d) => d.label === e?.activeLabel);
              if (bucket && onClick) onClick(bucket.key);
            }}
          >
            <defs>
              <linearGradient id="amberFill" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="var(--amber)" stopOpacity={0.24} />
                <stop offset="95%" stopColor="var(--amber)" stopOpacity={0.015} />
              </linearGradient>
              <linearGradient id="mintFill" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="var(--mint)" stopOpacity={0.2} />
                <stop offset="95%" stopColor="var(--mint)" stopOpacity={0.01} />
              </linearGradient>
            </defs>
            <CartesianGrid stroke="var(--chart-grid)" vertical={false} strokeDasharray="3 5" />
            <XAxis
              dataKey={timeAxis ? 'x' : 'label'}
              type={timeAxis ? 'number' : 'category'}
              scale={timeAxis ? 'linear' : 'auto'}
              domain={timeAxis ? ['dataMin', 'dataMax'] : undefined}
              tickFormatter={timeAxis ? timeLabel : undefined}
              ticks={timeAxis && data[0]?.key.length === 10 ? data.map((b) => b.x!) : undefined}
              tick={{ fill: 'var(--text-muted)', fontSize: 10 }}
              axisLine={false}
              tickLine={false}
              minTickGap={36}
            />
            <YAxis
              tick={{ fill: 'var(--text-muted)', fontSize: 10, fontFamily: 'var(--mono)' }}
              axisLine={false}
              tickLine={false}
              tickFormatter={(v) => (cost ? money(v, 1) : compact(v))}
            />
            <Tooltip
              content={
                <ChartTooltip
                  formatLabel={(label) => (timeAxis ? timeLabel(Number(label)) : String(label))}
                />
              }
            />
            {stacked ? (
              <>
                <Area
                  isAnimationActive={false}
                  type="linear"
                  dataKey="input"
                  name="Input tokens"
                  stackId="tokens"
                  stroke="var(--amber)"
                  fill="url(#amberFill)"
                  strokeWidth={2}
                />
                <Area
                  isAnimationActive={false}
                  type="linear"
                  dataKey="output"
                  name="Output tokens"
                  stackId="tokens"
                  stroke="var(--mint)"
                  fill="url(#mintFill)"
                  strokeWidth={2}
                />
                <Area
                  isAnimationActive={false}
                  type="linear"
                  dataKey="cache_read"
                  name="Cache-read tokens"
                  stackId="tokens"
                  stroke="#a6a2ca"
                  fill="#a6a2ca"
                  fillOpacity={0.1}
                  strokeWidth={1.5}
                />
                <Area
                  isAnimationActive={false}
                  type="linear"
                  dataKey="cache_write"
                  name="Cache-write tokens"
                  stackId="tokens"
                  stroke="#809bb9"
                  fill="#809bb9"
                  fillOpacity={0.1}
                  strokeWidth={1.5}
                />
              </>
            ) : (
              <Area
                isAnimationActive={false}
                type="linear"
                dataKey={cost ? 'cost' : 'total'}
                name={cost ? 'Estimated cost' : rate ? 'Tokens / minute' : 'Observed tokens'}
                stroke="var(--amber)"
                fill="url(#amberFill)"
                strokeWidth={2}
              />
            )}
            {onInterval && data.length > 1 && (
              <Brush
                dataKey="label"
                height={22}
                stroke="var(--amber)"
                fill="var(--surface-raised)"
                ariaLabel="Select timeline interval"
                travellerWidth={9}
                onDragEnd={({ startIndex, endIndex }) => {
                  const first = data[startIndex ?? 0]?.key;
                  const last = data[endIndex ?? data.length - 1]?.key;
                  if (first && last) onInterval(first, last);
                }}
              />
            )}
          </AreaChart>
        </ResponsiveContainer>
      </div>
      {onClick && data.length > 0 && (
        <form
          className="chart-controls"
          onSubmit={(e) => {
            e.preventDefault();
            onClick(data.some((b) => b.key === bucketKey) ? bucketKey : data[0].key);
          }}
        >
          <Field label="Usage date">
            <select
              aria-label="Usage date"
              value={data.some((b) => b.key === bucketKey) ? bucketKey : data[0].key}
              onChange={(e) => setBucketKey(e.target.value)}
            >
              {data.map((b) => (
                <option key={b.key} value={b.key}>
                  {b.label}
                </option>
              ))}
            </select>
          </Field>
          <button className="button small" type="submit">
            Inspect day
          </button>
        </form>
      )}
      {onInterval && data.length > 1 && (
        <form
          className="chart-controls"
          onSubmit={(e) => {
            e.preventDefault();
            const start = data.findIndex((b) => b.key === intervalStart);
            const end = data.findIndex((b) => b.key === intervalEnd);
            const first = start < 0 ? 0 : start,
              last = end < 0 ? data.length - 1 : end;
            onInterval(data[Math.min(first, last)].key, data[Math.max(first, last)].key);
          }}
        >
          <Field label="First interval">
            <select
              aria-label="First interval"
              value={data.some((b) => b.key === intervalStart) ? intervalStart : data[0].key}
              onChange={(e) => setIntervalStart(e.target.value)}
            >
              {data.map((b) => (
                <option key={b.key} value={b.key}>
                  {b.label}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Last interval">
            <select
              aria-label="Last interval"
              value={
                data.some((b) => b.key === intervalEnd) ? intervalEnd : data[data.length - 1].key
              }
              onChange={(e) => setIntervalEnd(e.target.value)}
            >
              {data.map((b) => (
                <option key={b.key} value={b.key}>
                  {b.label}
                </option>
              ))}
            </select>
          </Field>
          <button className="button small" type="submit">
            Inspect interval
          </button>
        </form>
      )}
      {cost && (
        <p className="small-note">
          API-equivalent estimate in USD ·{' '}
          {coverage.unpriced_tokens > 0 || coverage.unknown_fields?.some((k) => k !== 'reasoning')
            ? 'Partial estimate · '
            : ''}
          {costCoverage(coverage, 'Known prices for observed events')} · not a bill.
        </p>
      )}
    </div>
  );
}
function SectionHeading({
  eyebrow,
  title,
  children,
}: {
  eyebrow?: string;
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="section-heading">
      <div>
        {eyebrow && <span className="eyebrow">{eyebrow}</span>}
        <h2>{title}</h2>
      </div>
      {children}
    </div>
  );
}
type Run = <K extends keyof Commands>(
  command: K,
  args: Commands[K]['args'],
  message?: string,
) => Promise<Commands[K]['result'] | undefined>;
type View = { name: string; filter: Filter };

export default function AppRoot() {
  const [error, setError] = useState<UserFacingError | null>(null);
  return (
    <FailureContext.Provider value={{ error, setError, clear: () => setError(null) }}>
      <App />
    </FailureContext.Provider>
  );
}
function App() {
  const [page, setPage] = useStored<Page>('page', 'Live');
  const [demo, setDemo] = useStored('demo', false);
  const [filter, setFilter] = useStored<Filter>('filter', {});
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState('');
  const [indexError, setIndexError] = useState<UserFacingError | null>(null);
  const { error: failure, setError: setFailure } = useContext(FailureContext);
  const [recoveryCopy, setRecoveryCopy] = useState('');
  const [helpOpen, setHelpOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [paused, setPaused] = useState(false);
  const [selected, setSelected] = useStored<string | null>('selected', null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [detailError, setDetailError] = useState('');
  const [detailRange, setDetailRange] = useState<Filter>({});
  const [eventQuery, setEventQuery] = useState({ search: '', offset: 0 });
  const [inspectorWidth, setInspectorWidth] = useStored('inspector-width', 380);
  const [toast, setToast] = useState('');
  const [command, setCommand] = useState(false);
  const [commandSearch, setCommandSearch] = useState('');
  const [exportOpen, setExportOpen] = useState(false);
  const [exportStatus, setExportStatus] = useState('');
  const [exportIds, setExportIds] = useState<string[] | undefined>();
  const [exportFormat, setExportFormat] = useState<'csv' | 'json'>('csv');
  const [redactPaths, setRedactPaths] = useState(true);
  const [redactLabels, setRedactLabels] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [columnOpen, setColumnOpen] = useState(false);
  const [columns, setColumns] = useStored('columns', {
    model: true,
    tokens: true,
    cost: true,
    activity: true,
  });
  const [savedViews, setSavedViews] = useStored<View[]>('saved-views', []);
  const [saveViewOpen, setSaveViewOpen] = useState(false);
  const [viewName, setViewName] = useState('');
  const [compareIds, setCompareIds] = useStored<string[]>('compare', []);
  const [sourceOpen, setSourceOpen] = useState(false);
  const [sourceEdit, setSourceEdit] = useState<Source | undefined>();
  const compactMode = ['true', '1'].includes(
    new URLSearchParams(window.location.search).get('compact') || '',
  );
  const requestId = useRef(0);
  const detailRequestId = useRef(0);
  const latest = useRef({ filter, demo, selected, detailRange, paused, eventQuery });
  latest.current = { filter, demo, selected, detailRange, paused, eventQuery };
  const refresh = useCallback(async (initial = false) => {
    const id = ++requestId.current;
    try {
      if (initial) setLoading(true);
      const data = await api('snapshot', { filter: latest.current.filter }, latest.current.demo);
      if (id !== requestId.current) return;
      setSnapshot(data);
      setError('');
      setIndexError(null);
    } catch (e) {
      if (id === requestId.current) {
        setError(String(userError(e)));
        setIndexError(userError(e));
      }
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);
  const refreshDetail = useCallback(async () => {
    const request = ++detailRequestId.current;
    const { selected: id, demo, filter, detailRange, eventQuery } = latest.current;
    if (!id) {
      setDetail(null);
      return;
    }
    try {
      const data = await api(
        'session',
        {
          id,
          filter: {
            ...filter,
            ...detailRange,
            period: detailRange.from || detailRange.to ? undefined : filter.period,
          },
          event_search: eventQuery.search,
          event_offset: eventQuery.offset,
        },
        demo,
      );
      if (id === latest.current.selected && request === detailRequestId.current) {
        setDetail(data);
        setDetailError('');
      }
    } catch (e) {
      if (id === latest.current.selected && request === detailRequestId.current)
        setDetailError(String(e));
    }
  }, []);
  useEffect(() => {
    setLoading(true);
    const timer = setTimeout(() => refresh(true), 180);
    return () => clearTimeout(timer);
  }, [filter, demo, refresh]);
  useEffect(() => {
    setDetail(null);
    setDetailError('');
    void refreshDetail();
  }, [selected, demo, detailRange, filter, refreshDetail]);
  useEffect(() => {
    const timer = setTimeout(() => void refreshDetail(), 180);
    return () => clearTimeout(timer);
  }, [eventQuery, refreshDetail]);
  useEffect(() => {
    setEventQuery({ search: '', offset: 0 });
  }, [selected, detailRange, filter, demo]);
  useEffect(() => {
    const timer = setInterval(() => {
      if (!latest.current.paused) {
        void refresh();
        if (latest.current.selected) void refreshDetail();
      }
    }, 3000);
    return () => clearInterval(timer);
  }, [refresh, refreshDetail]);
  useEffect(() => {
    void getMonitoring()
      .then((v) => setPaused(v.paused))
      .catch(() => {});
    if (!isDesktop) return;
    let disposed = false;
    const cleanups: (() => void)[] = [];
    void import('@tauri-apps/api/event').then(async ({ listen }) => {
      const stop = await listen<{ paused: boolean }>('index-updated', (event) => {
        setPaused(event.payload.paused);
        if (!event.payload.paused) {
          void refresh();
          void refreshDetail();
        }
      });
      if (disposed) stop();
      else cleanups.push(stop);
      const stopNavigate = await listen<string>('navigate', (event) => {
        setCommand(false);
        setSourceOpen(false);
        setExportOpen(false);
        if (event.payload === 'help') setHelpOpen(true);
        else setPage(event.payload === 'settings' ? 'Settings' : 'Sources');
      });
      if (disposed) stopNavigate();
      else cleanups.push(stopNavigate);
      const stopError = await listen<import('./api').UserErrorData>('index-error', (event) => {
        setError(String(userError(event.payload)));
        setIndexError(userError(event.payload));
      });
      if (disposed) stopError();
      else cleanups.push(stopError);
    });
    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
    };
  }, [refresh, refreshDetail, setPage]);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(''), 4000);
    return () => clearTimeout(timer);
  }, [toast]);
  useEffect(() => {
    if (snapshot) {
      displayTimezone = snapshot.settings.timezone;
      document.documentElement.dataset.theme = snapshot.settings.theme;
      document.documentElement.dataset.density = snapshot.settings.density;
      setRedactPaths(snapshot.settings.redact_paths);
      setRedactLabels(snapshot.settings.redact_labels);
    }
  }, [
    snapshot?.settings.theme,
    snapshot?.settings.density,
    snapshot?.settings.timezone,
    snapshot?.settings.redact_paths,
    snapshot?.settings.redact_labels,
  ]);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ',') {
        if (!document.querySelector('[role="dialog"]')) {
          e.preventDefault();
          setPage('Settings');
        }
        return;
      }
      if (document.querySelector('[role="dialog"]') && e.key !== 'Escape') return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setCommand((v) => !v);
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'e') {
        e.preventDefault();
        setExportIds(undefined);
        setExportOpen(true);
      }
      if ((e.metaKey || e.ctrlKey) && /^[1-8]$/.test(e.key)) {
        e.preventDefault();
        setPage(PAGES[Number(e.key) - 1].name);
      }
      if (e.key === 'Escape') {
        setCommand(false);
        setColumnOpen(false);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [setPage]);
  const run: Run = useCallback(
    async (command, args, message) => {
      setBusy(true);
      setFailure(null);
      try {
        const result = await api(command, args, latest.current.demo);
        if (message) setToast(message);
        await refresh();
        await refreshDetail();
        return result;
      } catch (e) {
        setFailure(userError(e));
        return undefined;
      } finally {
        setBusy(false);
      }
    },
    [refresh, refreshDetail],
  );
  const updateFilter = (next: Partial<Filter>) => setFilter({ ...filter, ...next, offset: 0 });
  const openSession = (id: string) => {
    setDetailRange({});
    setSelected(id);
    if (!['Live', 'Sessions'].includes(page)) setPage('Sessions');
  };
  const changeDemo = (value: boolean) => {
    setDemo(value);
    setSelected(null);
    setFilter({});
    setCompareIds([]);
    setSnapshot(null);
  };
  const compare = (id: string) => {
    if (compareIds.includes(id)) {
      setToast('Already in your comparison');
      return;
    }
    if (compareIds.length === 4) {
      setToast('Compare up to four sessions. Remove one to add another.');
      return;
    }
    setCompareIds([...compareIds, id]);
    setToast('Added to Compare');
  };
  const exportView = (ids?: string[]) => {
    setExportIds(ids);
    setExportOpen(true);
  };
  const saveExportFile = async () => {
    setExportStatus('');
    const opener = document.activeElement as HTMLElement | null;
    setBusy(true);
    setFailure(null);
    try {
      const result = await api(
        'export',
        {
          format: exportFormat,
          filter,
          session_ids: exportIds,
          redact_paths: redactPaths,
          redact_labels: redactLabels,
        },
        demo,
      );
      if (!(await saveExport(result))) {
        setExportStatus('Export cancelled. You can choose a destination when ready.');
        return;
      }
      setExportOpen(false);
      setToast('Export ready');
    } catch (e) {
      setFailure(userError(e));
    } finally {
      setBusy(false);
      requestAnimationFrame(() => {
        if (opener?.isConnected && document.querySelector('[role="dialog"]')) opener.focus();
      });
    }
  };
  const togglePause = async () => {
    try {
      await setMonitoring(!paused);
      setPaused(!paused);
    } catch (e) {
      setToast(String(e));
    }
  };
  const drill = (next: Partial<Filter>) => {
    setFilter({ ...filter, ...next, offset: 0 });
    setPage('Sessions');
    setSelected(null);
  };
  const editSource = (source?: Source) => {
    setSourceEdit(source);
    setSourceOpen(true);
  };
  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const start = e.clientX,
      width = inspectorWidth;
    const move = (event: PointerEvent) =>
      setInspectorWidth(Math.max(330, Math.min(640, width + start - event.clientX)));
    const end = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      document.body.style.cursor = '';
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    document.body.style.cursor = 'col-resize';
  };
  const latestSource =
    snapshot?.sources
      .map((s) => s.last_read)
      .filter(Boolean)
      .sort()
      .at(-1) || null;
  const profiles = [...new Set(snapshot?.sources.map((s) => s.label) || [])];
  const allModels = [...new Set(snapshot?.sessions.flatMap((s) => s.models) || [])];
  const activeFilters = Object.entries(filter).filter(
    ([key, value]) =>
      !['sort', 'offset', 'limit'].includes(key) &&
      value !== undefined &&
      value !== '' &&
      value !== false,
  );
  const recover = async (action: 'index_retry' | 'index_preserve' | 'restore') => {
    const opener = document.activeElement as HTMLElement | null;
    setBusy(true);
    setFailure(null);
    try {
      if (action === 'restore') {
        const result = await restoreIndex();
        if (!result.restored) return;
        setRecoveryCopy(result.backup || '');
      } else if (action === 'index_preserve') {
        const result = await api(action, {}, false);
        setRecoveryCopy(result.backup);
      } else {
        const result = await api(action, {}, false);
        if (result.backup) setRecoveryCopy(result.backup);
      }
      await refresh(true);
    } catch (e) {
      setFailure(userError(e));
    } finally {
      setBusy(false);
      requestAnimationFrame(() => {
        if (opener?.isConnected) opener.focus();
      });
    }
  };
  if (!snapshot && indexError)
    return (
      <main className="recovery-screen">
        <img src="/chip.svg" alt="" />
        <h1>Open your local index</h1>
        <ErrorNotice error={indexError} />
        {failure && <ErrorNotice error={failure} clear={() => setFailure(null)} />}
        <p>
          Your existing database and local notes remain on this device. Recovery never creates an
          empty replacement index.
        </p>
        <div className="empty-actions">
          <button
            className="button primary"
            disabled={busy}
            onClick={() => void recover('index_retry')}
          >
            Retry opening index
          </button>
          <button className="button" disabled={busy} onClick={() => void recover('index_preserve')}>
            Preserve recovery copy
          </button>
          {isDesktop && (
            <button className="button" disabled={busy} onClick={() => void recover('restore')}>
              Select backup to restore…
            </button>
          )}
          <button className="button" onClick={() => changeDemo(true)}>
            Explore demo workspace
          </button>
        </div>
        <p>
          Restoring validates the selected backup and preserves the current database, sidecars, and
          notification metadata first. Notes added after that backup remain in the recovery copy.
        </p>
        {recoveryCopy && (
          <p role="status">
            Recovery copy saved: <code>{recoveryCopy}</code>
          </p>
        )}
      </main>
    );
  const compactMonitor = compactMode && snapshot;
  if (compactMonitor)
    return (
      <div className="compact-monitor">
        <div className="compact-brand">
          <img src="/chip.svg" alt="" />
          <strong>Chip Count</strong>
          <Badge tone="mint">{paused ? 'Paused' : 'Live'}</Badge>
          <IconButton
            label={paused ? 'Resume monitoring' : 'Pause monitoring'}
            onClick={togglePause}
          >
            {paused ? <Play size={14} /> : <Pause size={14} />}
          </IconButton>
        </div>
        {demo && <div className="compact-demo">DEMO DATA</div>}
        <div className="compact-totals">
          <div>
            <small>TOKENS TODAY</small>
            <strong>{compact(snapshot.today.total)}</strong>
          </div>
          <div>
            <small>API ESTIMATE</small>
            <strong>{usageCost(snapshot.today)}</strong>
          </div>
          <div>
            <small>TOKENS / MIN</small>
            <strong>{compact(snapshot.recent_rate)}</strong>
          </div>
        </div>
        <div className="compact-sessions">
          {snapshot.sessions
            .filter((s) => s.pinned || s.state === 'active')
            .slice(0, 6)
            .map((s) => (
              <div key={s.id}>
                <ProviderIcon provider={s.provider} />
                <span>
                  {s.name}
                  <small>{s.project}</small>
                </span>
                <b>{compact(s.usage.total)}</b>
                <State value={s.state} />
              </div>
            ))}
        </div>
        {snapshot.limits.map((l, i) => (
          <small className="compact-limit" key={i}>
            {l.provider} · {l.scope}:{' '}
            {l.used_percent === null ? 'Unavailable' : l.used_percent + '% reported used'}
          </small>
        ))}
        <footer>
          <i className="live-dot" />
          {paused ? 'Monitoring paused' : 'Local monitoring · ' + ago(latestSource)}
        </footer>
      </div>
    );
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a
          className="brand"
          href="#live"
          onClick={(e) => {
            e.preventDefault();
            setPage('Live');
          }}
        >
          <img src="/chip.svg" alt="Chip Count logo" />
          <div>
            Chip Count<span>LOCAL INTELLIGENCE</span>
          </div>
        </a>
        <button className="workspace-switch" onClick={() => setPage('Sources')}>
          <span className="workspace-avatar">{demo ? 'D' : 'L'}</span>
          <span>
            {demo ? 'Demo workspace' : 'Local workspace'}
            <small>{demo ? 'Sample data' : 'Private by design'}</small>
          </span>
          <ChevronDown size={13} />
        </button>
        <div className="nav-label">WORKSPACE</div>
        <nav aria-label="Main navigation">
          {PAGES.slice(0, 6).map(({ name, icon: Icon }) => (
            <button
              key={name}
              className={'nav-item ' + (page === name ? 'active' : '')}
              onClick={() => setPage(name)}
            >
              <Icon size={17} />
              <span>{name}</span>
              {name === 'Live' && !!snapshot?.active_sessions && (
                <b className="nav-count">{snapshot.active_sessions}</b>
              )}
              {name === 'Compare' && !!compareIds.length && (
                <b className="nav-count">{compareIds.length}</b>
              )}
              {name === 'Live' && page === 'Live' && <i className="nav-live" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="sidebar-insight">
            <span>
              <ShieldCheck size={14} /> ONLY ON YOUR DEVICE
            </span>
            <p>
              Your sessions stay yours.
              <br />
              No uploads. No API keys.
            </p>
          </div>
          <nav>
            {PAGES.slice(6).map(({ name, icon: Icon }) => (
              <button
                key={name}
                className={'nav-item ' + (page === name ? 'active' : '')}
                onClick={() => setPage(name)}
              >
                <Icon size={17} />
                <span>{name}</span>
                {name === 'Sources' && (
                  <span className="source-count">
                    {snapshot?.sources.filter((s) => s.enabled).length || 0}
                  </span>
                )}
              </button>
            ))}
          </nav>
          <button className="nav-item" onClick={() => setHelpOpen(true)}>
            <CircleHelp size={17} />
            Help
          </button>
          <button className="command-trigger" onClick={() => setCommand(true)}>
            <Command size={15} />
            <span>Quick actions</span>
            <kbd>⌘ K</kbd>
          </button>
          <div className="sidebar-footer">
            <span className="labs-mark">R</span>
            <span>Rippley Labs</span>
            <small>v0.1.1</small>
          </div>
        </div>
      </aside>
      <main className="main-shell">
        <header className="topbar">
          <div className="breadcrumbs">
            <span>Workspace</span>
            <ChevronRight size={12} />
            <strong>{page}</strong>
            {demo && <Badge tone="amber">Demo</Badge>}
          </div>
          <div className="topbar-actions">
            <span className={'monitor-status ' + (paused ? 'paused' : '')}>
              <i />
              {paused ? 'Monitoring paused' : error ? 'Connection issue' : 'Monitoring locally'}
            </span>
            <span className="top-divider" />
            <IconButton
              label={paused ? 'Resume monitoring' : 'Pause monitoring'}
              onClick={togglePause}
            >
              {paused ? <Play size={15} /> : <Pause size={15} />}
            </IconButton>
            <IconButton
              label="Refresh indexed data"
              onClick={() => run('rescan', {}, 'Sources rescanned')}
              disabled={busy}
            >
              <RefreshCw size={15} className={busy ? 'spin' : ''} />
            </IconButton>
            {isDesktop && (
              <IconButton
                label="Open compact monitor"
                onClick={() => openCompact().catch((e) => setToast(String(e)))}
              >
                <Maximize2 size={15} />
              </IconButton>
            )}
            <button className="button subtle export-top" onClick={() => exportView()}>
              <ArrowDownToLine size={14} />
              Export
            </button>
          </div>
        </header>
        {demo && (
          <div className="demo-banner">
            <span>
              <Sparkles size={13} />
              <b>Demo workspace</b>
              <span>Explore sample sessions. Your local index is separate.</span>
            </span>
            <button onClick={() => changeDemo(false)}>
              Return to my data <ArrowRight size={13} />
            </button>
          </div>
        )}
        {failure && <ErrorNotice error={failure} clear={() => setFailure(null)} />}
        {recoveryCopy && (
          <p className="small-note" role="status">
            Recovery copy saved: {recoveryCopy}
          </p>
        )}
        {error && (
          <div className="error-banner">
            <CircleAlert size={15} />
            <span>
              <strong>Index needs attention.</strong> {error}
            </span>
            <button onClick={() => refresh(true)}>Retry</button>
            <button onClick={() => setPage('Sources')}>Source diagnostics</button>
          </div>
        )}
        <div
          className={
            'page-content ' + (['Live', 'Sessions'].includes(page) ? 'workspace-page' : '')
          }
        >
          {snapshot && (
            <p className="small-note" aria-label="Reporting range">
              {snapshot.reporting.timezone}
              {' · '}
              {snapshot.reporting.from_local ||
                (snapshot.reporting.observed_from
                  ? fmtDate(snapshot.reporting.observed_from)
                  : 'Start of observed history')}
              {' → '}
              {snapshot.reporting.to_local ||
                (snapshot.reporting.observed_to
                  ? fmtDate(snapshot.reporting.observed_to)
                  : 'End of observed history')}
              {snapshot.reporting.to ? ' (end exclusive)' : ' (observed history)'}
            </p>
          )}
          {!['Live', 'Sessions'].includes(page) && activeFilters.length > 0 && (
            <div className="filter-chips" aria-label="Active workspace filters">
              {activeFilters.map(([key, value]) => (
                <button
                  className="filter-chip"
                  key={key}
                  onClick={() => updateFilter({ [key]: undefined })}
                >
                  {key.replaceAll('_', ' ')}: {String(value).split('/').at(-1)} <X size={11} />
                </button>
              ))}
              <button className="text-button" onClick={() => setFilter({})}>
                Clear filters
              </button>
            </div>
          )}
          {(page === 'Live' || page === 'Sessions') && (
            <>
              <div className="page-heading">
                <div>
                  <div className="heading-kicker">
                    <span className={page === 'Live' ? 'live-dot' : 'tiny-dot'} />
                    {page === 'Live' ? 'RIGHT NOW' : 'YOUR LOCAL HISTORY'}
                  </div>
                  <h1>
                    {page === 'Live' ? 'Live sessions' : 'Session explorer'}
                    <span className="heading-count">{snapshot?.total_sessions ?? '—'}</span>
                  </h1>
                  <p>
                    {page === 'Live'
                      ? 'A clear view of every session, across every project.'
                      : 'Find the sessions behind your usage. Every token accounted for.'}
                  </p>
                </div>
                <div className="heading-actions">
                  {page === 'Sessions' && (
                    <button className="button" onClick={() => setSaveViewOpen(true)}>
                      <Plus size={14} />
                      Save view
                    </button>
                  )}
                  <button className="button" onClick={() => setAdvanced((v) => !v)}>
                    <SlidersHorizontal size={14} />
                    <span>Filters</span>
                    {activeFilters.length > 0 && (
                      <b className="button-count">{activeFilters.length}</b>
                    )}
                  </button>
                  <button
                    className="button subtle"
                    onClick={() => setColumnOpen((v) => !v)}
                    aria-expanded={columnOpen}
                  >
                    <Columns3 size={14} />
                    <span className="optional-label">View</span>
                    <ChevronDown size={12} />
                  </button>
                  {columnOpen && (
                    <div className="column-menu">
                      <span className="eyebrow">SHOW IN SESSION ROWS</span>
                      {Object.keys(columns).map((key) => (
                        <label key={key}>
                          <input
                            type="checkbox"
                            checked={columns[key as keyof typeof columns]}
                            onChange={(e) => setColumns({ ...columns, [key]: e.target.checked })}
                          />
                          {key[0].toUpperCase() + key.slice(1)}
                        </label>
                      ))}
                    </div>
                  )}
                </div>
              </div>
              <div className="metrics-strip">
                <Metric
                  icon={Radio}
                  label="Active sessions"
                  value={snapshot ? n(snapshot.active_sessions) : '—'}
                  note={
                    paused
                      ? 'Monitoring paused'
                      : 'Across ' + (snapshot?.projects.length ?? 0) + ' projects'
                  }
                  live
                />
                <Metric
                  icon={Layers3}
                  label="Tokens today"
                  value={snapshot ? compact(snapshot.today.total) : '—'}
                  note={
                    snapshot
                      ? compact(snapshot.today.input) +
                        ' input · ' +
                        compact(snapshot.today.output) +
                        ' output'
                      : 'Waiting for the local index'
                  }
                />
                <Metric
                  icon={Wallet}
                  label="Estimated cost today"
                  value={
                    snapshot
                      ? snapshot.today.unpriced_tokens === snapshot.today.total &&
                        snapshot.today.total > 0
                        ? 'Unpriced'
                        : usageCost(snapshot.today)
                      : '—'
                  }
                  note={
                    snapshot
                      ? costCoverage(snapshot.today, 'API equivalent · not a bill')
                      : 'API equivalent · not a bill'
                  }
                  accent
                />
                <Metric
                  icon={Zap}
                  label="Recent token rate"
                  value={snapshot ? compact(snapshot.recent_rate) : '—'}
                  suffix="/ min"
                  note="Observed in the last 5 minutes"
                />
              </div>
              <div className="session-filters">
                <div className="search-input">
                  <Search size={15} />
                  <input
                    aria-label="Search sessions"
                    value={filter.search || ''}
                    onChange={(e) => updateFilter({ search: e.target.value })}
                    placeholder="Search sessions, projects, or tags…"
                  />
                  {filter.search ? (
                    <IconButton label="Clear search" onClick={() => updateFilter({ search: '' })}>
                      <X size={13} />
                    </IconButton>
                  ) : (
                    <kbd>⌕</kbd>
                  )}
                </div>
                <FilterSelect
                  label="provider"
                  value={filter.provider}
                  onChange={(v) => updateFilter({ provider: v })}
                  options={[
                    ['claude', 'Claude Code'],
                    ['codex', 'Codex'],
                  ]}
                />
                <FilterSelect
                  label="project"
                  value={filter.project}
                  onChange={(v) => updateFilter({ project: v })}
                  options={(snapshot?.projects || []).map((p) => [p.path, p.name])}
                />
                <FilterSelect
                  label="state"
                  value={filter.state}
                  onChange={(v) => updateFilter({ state: v })}
                  options={['active', 'idle', 'completed'].map((s) => [
                    s,
                    s[0].toUpperCase() + s.slice(1),
                  ])}
                />
              </div>
              {advanced && (
                <div className="advanced-filters">
                  <FilterSelect
                    label="profile"
                    value={filter.profile}
                    onChange={(v) => updateFilter({ profile: v })}
                    options={profiles.map((p) => [p, p])}
                  />
                  <FilterSelect
                    label="model"
                    value={filter.model}
                    onChange={(v) => updateFilter({ model: v })}
                    options={allModels.map((m) => [m, modelName(m)])}
                  />
                  <Field label="From">
                    <input
                      type="date"
                      value={filter.from?.slice(0, 10) || ''}
                      onChange={(e) =>
                        updateFilter({ period: undefined, from: e.target.value || undefined })
                      }
                    />
                  </Field>
                  <Field label="Through">
                    <input
                      type="date"
                      value={filter.to?.slice(0, 10) || ''}
                      onChange={(e) =>
                        updateFilter({ period: undefined, to: e.target.value || undefined })
                      }
                    />
                  </Field>
                  <Field label="Tag">
                    <input
                      value={filter.tag || ''}
                      placeholder="Any tag"
                      onChange={(e) => updateFilter({ tag: e.target.value })}
                    />
                  </Field>
                  <Field label="Min. tokens">
                    <input
                      type="number"
                      min="0"
                      value={filter.min_tokens ?? ''}
                      placeholder="0"
                      onChange={(e) =>
                        updateFilter({
                          min_tokens: e.target.value ? Number(e.target.value) : undefined,
                        })
                      }
                    />
                  </Field>
                  <Field label="Max. tokens">
                    <input
                      type="number"
                      min="0"
                      value={filter.max_tokens ?? ''}
                      placeholder="No limit"
                      onChange={(e) =>
                        updateFilter({
                          max_tokens: e.target.value ? Number(e.target.value) : undefined,
                        })
                      }
                    />
                  </Field>
                  <Field label="Min. cost ($)">
                    <input
                      type="number"
                      min="0"
                      step=".01"
                      value={filter.min_cost ?? ''}
                      placeholder="0.00"
                      onChange={(e) =>
                        updateFilter({
                          min_cost: e.target.value ? Number(e.target.value) : undefined,
                        })
                      }
                    />
                  </Field>
                  <Field label="Max. cost ($)">
                    <input
                      type="number"
                      min="0"
                      step=".01"
                      value={filter.max_cost ?? ''}
                      placeholder="No limit"
                      onChange={(e) =>
                        updateFilter({
                          max_cost: e.target.value ? Number(e.target.value) : undefined,
                        })
                      }
                    />
                  </Field>
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={!!filter.pinned}
                      onChange={(e) => updateFilter({ pinned: e.target.checked })}
                    />
                    Pinned only
                  </label>
                </div>
              )}
              {(activeFilters.length > 0 || savedViews.length > 0) && (
                <div className="filter-chips">
                  {activeFilters.map(([key, value]) => (
                    <button
                      key={key}
                      className="filter-chip"
                      onClick={() => updateFilter({ [key]: undefined })}
                    >
                      {key.replace('_', ' ')}:{' '}
                      {String(value).length > 28 ? String(value).split('/').at(-1) : String(value)}{' '}
                      <X size={11} />
                    </button>
                  ))}
                  {activeFilters.length > 0 && (
                    <button className="text-button" onClick={() => setFilter({})}>
                      Clear filters
                    </button>
                  )}
                  {savedViews.map((v, i) => (
                    <span className="saved-view" key={i}>
                      <button onClick={() => setFilter(v.filter)}>
                        <ListFilter size={12} />
                        {v.name}
                      </button>
                      <button
                        aria-label={'Delete saved view ' + v.name}
                        onClick={() => setSavedViews(savedViews.filter((_, j) => j !== i))}
                      >
                        <X size={11} />
                      </button>
                    </span>
                  ))}
                </div>
              )}
              <div
                className={'session-workspace ' + (selected ? 'with-inspector' : '')}
                style={{ '--inspector-width': inspectorWidth + 'px' } as CSSProperties}
              >
                <section className="session-list-panel">
                  <div className="list-toolbar">
                    <div>
                      <span className="eyebrow">
                        {page === 'Live' ? 'SESSION ACTIVITY' : 'ALL SESSIONS'}
                      </span>
                      <span className="toolbar-count">{snapshot?.total_sessions || 0}</span>
                    </div>
                    <label className="sort-control">
                      Sort by{' '}
                      <select
                        aria-label="Sort sessions"
                        value={filter.sort || 'recent'}
                        onChange={(e) => updateFilter({ sort: e.target.value })}
                      >
                        <option value="recent">Last activity</option>
                        <option value="tokens">Tokens</option>
                        <option value="cost">Est. cost</option>
                        <option value="name">Name</option>
                      </select>
                      <ArrowDown size={11} />
                    </label>
                  </div>
                  {loading && !snapshot ? (
                    <div className="skeleton-list">
                      {[1, 2, 3, 4, 5].map((i) => (
                        <div className="skeleton-row" key={i}>
                          <i />
                          <span />
                          <b />
                        </div>
                      ))}
                    </div>
                  ) : snapshot?.sessions.length ? (
                    <SessionList
                      sessions={snapshot.sessions}
                      selected={selected}
                      onSelect={openSession}
                      onPin={(s) => run('annotate', { id: s.id, pinned: !s.pinned })}
                      columns={columns}
                      density={snapshot.settings.density}
                    />
                  ) : (
                    <Empty
                      icon={Database}
                      title={
                        activeFilters.length || filter.search
                          ? 'No matching sessions'
                          : 'Your sessions start here'
                      }
                      description={
                        activeFilters.length || filter.search
                          ? 'Try adjusting your filters to see more of your local history.'
                          : snapshot?.sources.length
                            ? 'No supported usage events were found in your configured sources. Add a folder or inspect source diagnostics.'
                            : 'Connect your Claude Code or Codex logs. Your usage is indexed locally and never uploaded.'
                      }
                      action={
                        <div className="empty-actions">
                          {activeFilters.length || filter.search ? (
                            <button className="button primary" onClick={() => setFilter({})}>
                              Clear filters
                            </button>
                          ) : (
                            <>
                              <button className="button primary" onClick={() => editSource()}>
                                <FolderOpen size={14} />
                                Add a source
                              </button>
                              {!demo && (
                                <button className="button" onClick={() => changeDemo(true)}>
                                  Explore demo
                                </button>
                              )}
                              <button className="text-button" onClick={() => setPage('Sources')}>
                                Source diagnostics <ArrowRight size={12} />
                              </button>
                            </>
                          )}
                        </div>
                      }
                    />
                  )}
                  <div className="list-footer">
                    <span>
                      <i className="live-dot" />
                      {paused ? 'Paused' : 'Updated ' + ago(latestSource)}
                    </span>
                    <span>
                      {snapshot?.totals.unpriced_tokens
                        ? compact(snapshot.totals.unpriced_tokens) + ' tokens unpriced'
                        : 'All usage stays local'}
                    </span>
                    {snapshot && snapshot.total_sessions > (filter.limit || 500) && (
                      <>
                        <span role="status">
                          Sessions {(filter.offset || 0) + 1}–
                          {Math.min(
                            snapshot.total_sessions,
                            (filter.offset || 0) + (filter.limit || 500),
                          )}{' '}
                          of {snapshot.total_sessions}
                        </span>
                        <button
                          className="text-button"
                          aria-label="Previous session page"
                          disabled={!filter.offset}
                          onClick={() => {
                            const offset = Math.max(
                              0,
                              (filter.offset || 0) - (filter.limit || 500),
                            );
                            setFilter({ ...filter, offset });
                            if (offset === 0)
                              requestAnimationFrame(() =>
                                document
                                  .querySelector<HTMLButtonElement>(
                                    '[aria-label="Next session page"]',
                                  )
                                  ?.focus(),
                              );
                          }}
                        >
                          Previous
                        </button>
                        <button
                          className="text-button"
                          aria-label="Next session page"
                          disabled={
                            (filter.offset || 0) + (filter.limit || 500) >= snapshot.total_sessions
                          }
                          onClick={() => {
                            const offset = (filter.offset || 0) + (filter.limit || 500);
                            setFilter({ ...filter, offset });
                            if (offset + (filter.limit || 500) >= snapshot.total_sessions)
                              requestAnimationFrame(() =>
                                document
                                  .querySelector<HTMLButtonElement>(
                                    '[aria-label="Previous session page"]',
                                  )
                                  ?.focus(),
                              );
                          }}
                        >
                          Next {filter.limit || 500}
                        </button>
                      </>
                    )}
                  </div>
                </section>
                {selected && (
                  <>
                    <div
                      className="resize-handle"
                      role="separator"
                      aria-label="Resize session inspector"
                      aria-orientation="vertical"
                      tabIndex={0}
                      onPointerDown={startResize}
                      onKeyDown={(e) => {
                        if (e.key === 'ArrowLeft')
                          setInspectorWidth(Math.min(640, inspectorWidth + 20));
                        if (e.key === 'ArrowRight')
                          setInspectorWidth(Math.max(330, inspectorWidth - 20));
                      }}
                    />
                    <Inspector
                      detail={detail}
                      error={detailError}
                      onClose={() => {
                        const id = selected;
                        setSelected(null);
                        requestAnimationFrame(() => {
                          const rows =
                            document.querySelectorAll<HTMLButtonElement>('.session-row-main');
                          (
                            Array.from(rows).find((row) => row.dataset.sessionId === id) || rows[0]
                          )?.focus();
                        });
                      }}
                      run={run}
                      onCompare={compare}
                      onExport={(id) => exportView([id])}
                      onNavigate={(id) => setSelected(id)}
                      onToast={setToast}
                      range={detailRange}
                      onRange={setDetailRange}
                      eventQuery={eventQuery}
                      onEventQuery={setEventQuery}
                    />
                  </>
                )}
              </div>
              <div className="workspace-footnote">
                <CircleHelp size={12} />
                <span>
                  Activity is inferred from recent events. Sessions become idle after{' '}
                  {snapshot?.settings.inactivity_minutes ?? 5} minutes. Costs are API-equivalent
                  estimates.
                </span>
              </div>
            </>
          )}
          {page === 'Analytics' && (
            <Analytics
              snapshot={snapshot}
              filter={filter}
              onFilter={updateFilter}
              onDrill={drill}
              onSession={openSession}
              onExport={() => exportView()}
            />
          )}
          {page === 'Projects' && (
            <Projects snapshot={snapshot} onDrill={drill} run={run} onToast={setToast} />
          )}
          {page === 'Compare' && (
            <Compare
              snapshot={snapshot}
              ids={compareIds}
              setIds={setCompareIds}
              demo={demo}
              filter={filter}
              onSession={openSession}
            />
          )}
          {page === 'Budgets & Limits' && <Budgets snapshot={snapshot} run={run} />}
          {page === 'Sources' && (
            <Sources
              snapshot={snapshot}
              run={run}
              busy={busy}
              onEdit={editSource}
              onDemo={() => changeDemo(!demo)}
              demo={demo}
              paused={paused}
              onPause={togglePause}
            />
          )}
          {page === 'Settings' && (
            <SettingsPage
              snapshot={snapshot}
              run={run}
              onSources={() => setPage('Sources')}
              onDemo={() => changeDemo(!demo)}
              demo={demo}
              onExport={() => exportView()}
            />
          )}
        </div>
      </main>
      {toast && (
        <div className="toast" role="status">
          <CheckCircle2 size={16} />
          <span>{toast}</span>
          <IconButton label="Dismiss notification" onClick={() => setToast('')}>
            <X size={13} />
          </IconButton>
        </div>
      )}
      <Modal
        open={command}
        onClose={() => setCommand(false)}
        title="Quick actions"
        description="Navigate your workspace, find a session, or take an action."
      >
        <div className="command-search">
          <Search size={18} />
          <input
            autoFocus
            placeholder="Search pages, sessions, and actions…"
            value={commandSearch}
            onChange={(e) => setCommandSearch(e.target.value)}
          />
          <kbd>ESC</kbd>
        </div>
        <div className="command-results">
          {PAGES.filter((p) => p.name.toLowerCase().includes(commandSearch.toLowerCase())).map(
            ({ name, icon: Icon, key }) => (
              <button
                key={name}
                onClick={() => {
                  setPage(name);
                  setCommand(false);
                }}
              >
                <Icon size={16} />
                <span>Go to {name}</span>
                <kbd>⌘ {key}</kbd>
              </button>
            ),
          )}
          {[
            { name: 'Export current view', icon: Download, action: () => exportView() },
            {
              name: paused ? 'Resume monitoring' : 'Pause monitoring',
              icon: paused ? Play : Pause,
              action: () => void togglePause(),
            },
            {
              name: 'Rescan sources',
              icon: RefreshCw,
              action: () => void run('rescan', {}, 'Sources rescanned'),
            },
            {
              name: demo ? 'Leave demo workspace' : 'Explore demo workspace',
              icon: Sparkles,
              action: () => changeDemo(!demo),
            },
            { name: 'Add a source', icon: FolderOpen, action: () => editSource() },
          ]
            .filter((a) => a.name.toLowerCase().includes(commandSearch.toLowerCase()))
            .map(({ name, icon: Icon, action }) => (
              <button
                key={name}
                onClick={() => {
                  action();
                  setCommand(false);
                }}
              >
                <Icon size={16} />
                <span>{name}</span>
                <ArrowUpRight size={13} />
              </button>
            ))}
          {commandSearch &&
            snapshot?.sessions
              .filter((s) =>
                (s.name + ' ' + s.project).toLowerCase().includes(commandSearch.toLowerCase()),
              )
              .slice(0, 8)
              .map((s) => (
                <button
                  key={s.id}
                  onClick={() => {
                    openSession(s.id);
                    setCommand(false);
                  }}
                >
                  <ProviderIcon provider={s.provider} />
                  <span>
                    {s.name}
                    <small>{s.project}</small>
                  </span>
                  <ArrowUpRight size={13} />
                </button>
              ))}
        </div>
      </Modal>
      <Modal
        open={exportOpen}
        onClose={() => setExportOpen(false)}
        title="Export usage"
        description={
          exportIds
            ? 'Export the selected session with accounting metadata.'
            : 'Export your current filtered view with accounting metadata.'
        }
      >
        <div className="dialog-body">
          {exportStatus && (
            <p className="small-note" role="status">
              {exportStatus}
            </p>
          )}
          <Field label="File format">
            <select
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value as 'csv' | 'json')}
            >
              <option value="csv">CSV · Spreadsheet compatible</option>
              <option value="json">JSON · Structured usage metadata</option>
            </select>
          </Field>
          <Toggle
            label="Redact file paths"
            description="Keep local directories out of the export."
            value={redactPaths}
            onChange={setRedactPaths}
          />
          <Toggle
            label="Redact custom labels"
            description="Remove personal source, project, and session labels."
            value={redactLabels}
            onChange={setRedactLabels}
          />
          <div className="info-box">
            <ShieldCheck size={16} />
            <span>
              Includes units, timezone, selected filters, pricing and coverage metadata. Source
              transcripts are never exported.
            </span>
          </div>
        </div>
        <div className="dialog-footer">
          <button className="button" onClick={() => setExportOpen(false)}>
            Cancel
          </button>
          <button className="button primary" disabled={busy} onClick={saveExportFile}>
            <Download size={14} />
            {busy ? 'Preparing…' : 'Export ' + exportFormat.toUpperCase()}
          </button>
        </div>
      </Modal>
      <Modal
        open={saveViewOpen}
        onClose={() => setSaveViewOpen(false)}
        title="Save this view"
        description="Keep your current combination of filters and sorting."
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!viewName.trim()) return;
            setSavedViews([...savedViews, { name: viewName.trim(), filter }]);
            setViewName('');
            setSaveViewOpen(false);
            setToast('View saved');
          }}
        >
          <div className="dialog-body">
            <Field label="View name">
              <input
                autoFocus
                required
                maxLength={50}
                value={viewName}
                onChange={(e) => setViewName(e.target.value)}
                placeholder="e.g. Codex · this week"
              />
            </Field>
          </div>
          <div className="dialog-footer">
            <button className="button primary" type="submit">
              Save view
            </button>
          </div>
        </form>
      </Modal>
      <SourceDialog
        open={sourceOpen}
        source={sourceEdit}
        demo={demo}
        onClose={() => setSourceOpen(false)}
        run={run}
        onToast={setToast}
      />
      <Modal
        open={helpOpen}
        onClose={() => setHelpOpen(false)}
        title="Chip Count Help"
        description="Read-only local usage accounting and keyboard navigation."
      >
        <div className="dialog-body">
          <p>
            Configure Claude Code or Codex logs in Sources. Use Configure to reselect a missing
            folder or file, Scan to retry reads, and file/line diagnostics to find malformed JSONL.
          </p>
          <p>
            Search and filter history in Sessions. Tab to a session, press Enter to inspect it, and
            use ↑ / ↓, Home / End or Page Up / Page Down to move through the list. Use the paging
            buttons to load more history.
          </p>
          <p>
            Charts with actions also provide selectors and buttons. Date filters use your reporting
            timezone. Export includes the current filters; cancelling the native dialog keeps the
            report ready.
          </p>
          <p>
            ⌘ / Ctrl , opens Settings. ⌘ / Ctrl 1–8 changes pages; K opens quick actions; E exports.
            Escape closes dialogs and returns focus to the opening control.
          </p>
          <p>
            Rebuild preserves a SQLite backup before re-reading logs. Notes, local model rates,
            budgets, and previous observations are retained. A corrupt index opens recovery; restore
            a known-good backup after preserving the original.
          </p>
          <button
            className="button"
            onClick={() => {
              setHelpOpen(false);
              setPage('Sources');
            }}
          >
            Open source diagnostics
          </button>
        </div>
      </Modal>
    </div>
  );
}

function Metric({
  icon: Icon,
  label,
  value,
  note,
  suffix,
  live,
  accent,
  spark,
}: {
  icon: typeof Activity;
  label: string;
  value: string;
  note: string;
  suffix?: string;
  live?: boolean;
  accent?: boolean;
  spark?: number[];
}) {
  return (
    <div className={'metric ' + (accent ? 'accent' : '')}>
      <div className="metric-label">
        <Icon size={13} />
        {label}
        {live && <span className="metric-live">LIVE</span>}
      </div>
      <div className="metric-value">
        {value}
        {suffix && <small>{suffix}</small>}
        {spark && <Sparkline values={spark} color="var(--mint)" width={65} height={22} />}
      </div>
      <p>
        {live && <i className="live-dot" />}
        {note}
      </p>
    </div>
  );
}
function FilterSelect({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value?: string;
  onChange: (v: string) => void;
  options: string[][];
}) {
  return (
    <div className="filter-select">
      <select
        aria-label={'Filter by ' + label}
        value={value || ''}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">All {label === 'state' ? 'states' : label + 's'}</option>
        {options.map(([v, l]) => (
          <option key={v} value={v}>
            {l}
          </option>
        ))}
      </select>
      <ChevronDown size={12} />
    </div>
  );
}
function SessionList({
  sessions,
  selected,
  onSelect,
  onPin,
  columns,
  density,
}: {
  sessions: Session[];
  selected: string | null;
  onSelect: (id: string) => void;
  onPin: (s: Session) => void;
  columns: { model: boolean; tokens: boolean; cost: boolean; activity: boolean };
  density: Settings['density'];
}) {
  const parent = useRef<HTMLDivElement>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const pendingFocus = useRef<number | null>(null);
  const virtual = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => parent.current,
    estimateSize: () => (density === 'compact' ? 90 : 104),
    overscan: 8,
  });
  useEffect(() => {
    virtual.measure();
  }, [density]);
  useEffect(
    () => () => {
      if (pendingFocus.current !== null) cancelAnimationFrame(pendingFocus.current);
    },
    [],
  );
  const visible = virtual.getVirtualItems();
  const entryId = visible.some((v) => sessions[v.index]?.id === focusedId)
    ? focusedId
    : sessions[visible[0]?.index || 0]?.id;
  return (
    <div
      className="session-list"
      ref={parent}
      aria-label="Session history"
      onKeyDown={(e) => {
        const button = (e.target as HTMLElement).closest<HTMLButtonElement>('.session-row-main');
        if (!button || !sessions.length) return;
        const current = Number(button.dataset.index);
        const next =
          e.key === 'ArrowDown'
            ? current + 1
            : e.key === 'ArrowUp'
              ? current - 1
              : e.key === 'Home'
                ? 0
                : e.key === 'End'
                  ? sessions.length - 1
                  : e.key === 'PageDown'
                    ? current + 8
                    : e.key === 'PageUp'
                      ? current - 8
                      : undefined;
        if (next === undefined) return;
        e.preventDefault();
        const index = Math.max(0, Math.min(sessions.length - 1, next));
        setFocusedId(sessions[index].id);
        onSelect(sessions[index].id);
        virtual.scrollToIndex(index, { align: 'auto' });
        if (pendingFocus.current !== null) cancelAnimationFrame(pendingFocus.current);
        let attempts = 0;
        const focus = () => {
          const target = parent.current?.querySelector<HTMLButtonElement>(
            '[data-index="' + index + '"]',
          );
          if (target) {
            target.focus({ preventScroll: true });
            pendingFocus.current = null;
          } else if (++attempts < 20) pendingFocus.current = requestAnimationFrame(focus);
        };
        pendingFocus.current = requestAnimationFrame(focus);
      }}
    >
      <div style={{ height: virtual.getTotalSize(), position: 'relative', width: '100%' }}>
        {visible.map((v) => {
          const s = sessions[v.index];
          return (
            <div
              key={s.id}
              className={'session-row ' + (selected === s.id ? 'selected' : '')}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: v.size,
                transform: 'translateY(' + v.start + 'px)',
              }}
            >
              <button
                className="session-row-main"
                data-index={v.index}
                data-session-id={s.id}
                tabIndex={s.id === entryId ? 0 : -1}
                aria-pressed={selected === s.id}
                onFocus={() => setFocusedId(s.id)}
                onClick={() => onSelect(s.id)}
              >
                <div className="session-row-top">
                  <span className="project-icon">
                    <Folder size={14} />
                  </span>
                  <strong className="session-project">{s.project || 'Unassigned project'}</strong>
                  <State value={s.state} />
                  <span className="session-time">{ago(s.last_at)}</span>
                </div>
                <div className="session-name-line">
                  <span className="session-name">{s.name || s.id.slice(0, 24)}</span>
                  {s.subagents > 0 && (
                    <span className="subagent-count" title="Identified subagents">
                      <GitBranch size={11} />
                      {s.subagents}
                    </span>
                  )}
                </div>
                <div className="session-row-bottom">
                  <span className="provider-label">
                    <ProviderIcon provider={s.provider} />
                    {providerName(s.provider)}
                  </span>
                  {columns.model && (
                    <span className="row-model">
                      {modelName(s.models[0] || 'Unknown model')}
                      {s.models.length > 1 ? ' +' + (s.models.length - 1) : ''}
                    </span>
                  )}
                  <span className="row-metrics">
                    {columns.tokens && (
                      <span
                        title={
                          'Input ' +
                          n(s.usage.input) +
                          ' · Output ' +
                          n(s.usage.output) +
                          ' · Cache read ' +
                          n(s.usage.cache_read)
                        }
                      >
                        {compact(s.usage.total)}
                        <small> tokens</small>
                      </span>
                    )}
                    {columns.cost && (
                      <b title={costCoverage(s.usage, 'API-equivalent estimated cost')}>
                        {usageCost(s.usage)}
                      </b>
                    )}
                  </span>
                  {columns.activity && (
                    <span
                      title="Observed tokens in 3-minute buckets during the hour ending at the session’s last activity"
                      aria-label="Tokens during the session’s last hour"
                    >
                      <Sparkline
                        values={s.sparkline}
                        color={s.state === 'active' ? 'var(--mint)' : 'var(--chart-muted)'}
                        width={67}
                        height={20}
                      />
                    </span>
                  )}
                </div>
              </button>
              <button
                className={'pin-session ' + (s.pinned ? 'pinned' : '')}
                tabIndex={s.id === entryId ? 0 : -1}
                aria-label={s.pinned ? 'Unpin session' : 'Pin session'}
                title={s.pinned ? 'Unpin session' : 'Pin to Live'}
                onClick={() => onPin(s)}
              >
                <Star size={13} fill={s.pinned ? 'currentColor' : 'none'} />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Inspector({
  detail,
  error,
  onClose,
  run,
  onCompare,
  onExport,
  onNavigate,
  onToast,
  range,
  onRange,
  eventQuery,
  onEventQuery,
}: {
  detail: Detail | null;
  error: string;
  onClose: () => void;
  run: Run;
  onCompare: (id: string) => void;
  onExport: (id: string) => void;
  onNavigate: (id: string) => void;
  onToast: (s: string) => void;
  range: Filter;
  onRange: (v: Filter) => void;
  eventQuery: { search: string; offset: number };
  onEventQuery: (query: { search: string; offset: number }) => void;
}) {
  const [tab, setTab] = useStored('inspector-tab', 'Overview');
  const [editing, setEditing] = useState(false);
  const [alias, setAlias] = useState('');
  const [notes, setNotes] = useState('');
  const [tags, setTags] = useState('');
  const [event, setEvent] = useState<UsageEvent | null>(null);
  const [cumulative, setCumulative] = useState(false);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  useEffect(() => {
    if (detail) {
      setAlias(detail.session.name);
      setNotes(detail.session.notes);
      setTags(detail.session.tags.join(', '));
      setEditing(false);
      setEvent(null);
    }
  }, [detail?.session.id]);
  const s = detail?.session,
    u = s?.usage || ZERO;
  const inputTotal = u.input + u.cache_read + u.cache_write;
  const cacheRatio =
    inputTotal > 0 &&
    !u.unknown_fields?.some((k) => ['input', 'cache_read', 'cache_write'].includes(k))
      ? (u.cache_read / inputTotal) * 100
      : null;
  const chart = useMemo(() => {
    let total = 0,
      cost = 0;
    return (detail?.timeline || []).map((b) => {
      total += b.total;
      cost += b.cost;
      return cumulative ? { ...b, total, cost } : b;
    });
  }, [detail?.timeline, cumulative]);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!s) return;
    const ok = await run(
      'annotate',
      {
        id: s.id,
        alias: alias.trim(),
        notes,
        tags: tags
          .split(',')
          .map((t) => t.trim())
          .filter(Boolean),
      },
      'Session details saved',
    );
    if (ok) setEditing(false);
  };
  const reveal = async (path: string) => {
    try {
      await revealPath(path);
    } catch (e) {
      onToast(String(e));
    }
  };
  return (
    <aside className="inspector">
      <div className="inspector-bar">
        <span>
          <PanelRightClose size={13} />
          SESSION INSPECTOR
        </span>
        <IconButton label="Close session inspector" onClick={onClose}>
          <X size={16} />
        </IconButton>
      </div>
      {error ? (
        <Empty icon={CircleAlert} title="Session unavailable" description={error} />
      ) : !detail || !s ? (
        <div className="inspector-loading">
          <LoaderCircle size={23} className="spin" />
          <span>Reading session metadata…</span>
        </div>
      ) : (
        <>
          <div className="inspector-header">
            <div className="inspector-project">
              <Folder size={13} />
              {s.project}
              <State value={s.state} />
            </div>
            <h2>{s.name}</h2>
            <div className="inspector-meta">
              <ProviderIcon provider={s.provider} />
              <span>{providerName(s.provider)}</span>
              <span className="dot-separator">·</span>
              <span>{s.profile}</span>
              <span className="dot-separator">·</span>
              <span className="mono">{s.id.replace(/^(claude|codex):/, '').slice(0, 8)}</span>
              <IconButton
                label="Copy session ID"
                onClick={() =>
                  navigator.clipboard
                    .writeText(s.id.replace(/^(claude|codex):/, ''))
                    .then(() => onToast('Session ID copied'))
                    .catch(() => onToast('Clipboard unavailable'))
                }
              >
                <Clipboard size={12} />
              </IconButton>
            </div>
            <div className="inspector-actions">
              <button
                className={'button small ' + (s.pinned ? 'amber' : '')}
                onClick={() => run('annotate', { id: s.id, pinned: !s.pinned })}
              >
                <Star size={12} fill={s.pinned ? 'currentColor' : 'none'} />
                {s.pinned ? 'Pinned' : 'Pin session'}
              </button>
              <button className="button small" onClick={() => onCompare(s.id)}>
                <Columns3 size={12} />
                Compare
              </button>
              <IconButton label="Export this session" onClick={() => onExport(s.id)}>
                <Download size={14} />
              </IconButton>
              <IconButton label="Reveal source file" onClick={() => reveal(s.source_path)}>
                <FolderOpen size={14} />
              </IconButton>
            </div>
          </div>
          <div className="inspector-tabs" role="tablist" aria-label="Session inspector sections">
            {['Overview', 'Timeline', 'Events', 'Models', 'Context', 'Relations'].map((t) => (
              <button
                role="tab"
                aria-selected={tab === t}
                key={t}
                className={tab === t ? 'active' : ''}
                onClick={() => setTab(t)}
              >
                {t}
              </button>
            ))}
          </div>
          <div className="inspector-body">
            {Object.keys(range).length > 0 && (
              <div className="interval-banner">
                <FilterIcon size={12} />
                Selected interval
                <button
                  onClick={() => {
                    onRange({});
                    setFrom('');
                    setTo('');
                  }}
                >
                  Reset <X size={11} />
                </button>
              </div>
            )}
            {tab === 'Overview' && (
              <>
                <div className="inspector-stats">
                  <div>
                    <span>OBSERVED TOKENS</span>
                    <strong>
                      {u.unknown_fields?.length ? '≥ ' : ''}
                      {compact(u.total)}
                    </strong>
                    <small>{n(u.events)} usage events</small>
                  </div>
                  <div>
                    <span>API-EQUIVALENT COST</span>
                    <strong className="amber-text">
                      {u.unpriced_tokens === u.total && u.total > 0 ? 'Unpriced' : usageCost(u)}
                    </strong>
                    <small>{costCoverage(u, 'Known model rates')}</small>
                  </div>
                </div>
                <div className="inspector-section">
                  <div className="mini-heading">
                    <h3>Usage over time</h3>
                    <span>{elapsed(s.elapsed_seconds)} span</span>
                  </div>
                  <UsageChart data={detail.timeline} height={138} />
                  <div className="chart-legend">
                    <span>
                      <i style={{ background: 'var(--amber)' }} />
                      Observed tokens
                    </span>
                    <span>
                      {date(s.first_at)} · {time(s.first_at)}–{time(s.last_at)}
                    </span>
                  </div>
                </div>
                <div className="inspector-section">
                  <div className="mini-heading">
                    <h3>Token breakdown</h3>
                    <span>Own usage</span>
                  </div>
                  <div className="composition-bar">
                    {[
                      { key: 'input', value: u.input, color: 'var(--amber)' },
                      { key: 'cache_read', value: u.cache_read, color: 'var(--mint)' },
                      { key: 'cache_write', value: u.cache_write, color: 'var(--purple)' },
                      { key: 'output', value: u.output, color: 'var(--blue)' },
                    ].map((c) => (
                      <span
                        key={c.key}
                        style={{
                          width: (c.value / (u.total || 1)) * 100 + '%',
                          background: c.color,
                        }}
                        title={c.key + ': ' + n(c.value) + ' tokens'}
                      />
                    ))}
                  </div>
                  <UsageRow
                    color="var(--amber)"
                    label="Input · uncached"
                    value={u.input}
                    unknown={u.unknown_fields?.includes('input')}
                  />
                  <UsageRow
                    color="var(--mint)"
                    label="Cache read"
                    value={u.cache_read}
                    unknown={u.unknown_fields?.includes('cache_read')}
                  />
                  <UsageRow
                    color="var(--purple)"
                    label="Cache write"
                    value={u.cache_write}
                    unknown={u.unknown_fields?.includes('cache_write')}
                  />
                  <UsageRow
                    color="var(--blue)"
                    label="Output"
                    value={u.output}
                    unknown={u.unknown_fields?.includes('output')}
                  />
                  {u.reasoning > 0 && (
                    <div className="usage-subrow">
                      <span>Includes reasoning</span>
                      <span>{n(u.reasoning)}</span>
                    </div>
                  )}
                  <div className="cache-callout">
                    <span className="cache-icon">
                      <Zap size={15} />
                    </span>
                    <div>
                      <strong>
                        {cacheRatio === null
                          ? 'Cache ratio unavailable'
                          : cacheRatio.toFixed(1) + '% of input served from cache'}
                      </strong>
                      <small>Cache reads ÷ all input tokens, including cache writes.</small>
                    </div>
                  </div>
                </div>
                <div className="inspector-section">
                  <div className="mini-heading">
                    <h3>Session details</h3>
                  </div>
                  <dl className="detail-list">
                    <div>
                      <dt>First activity</dt>
                      <dd>
                        {date(s.first_at)}, {time(s.first_at)}
                      </dd>
                    </div>
                    <div>
                      <dt>Last activity</dt>
                      <dd>
                        {date(s.last_at)}, {time(s.last_at)}
                      </dd>
                    </div>
                    <div>
                      <dt>Elapsed span</dt>
                      <dd>{elapsed(s.elapsed_seconds)}</dd>
                    </div>
                    <div>
                      <dt>Estimated activity</dt>
                      <dd>{elapsed(s.active_seconds)}</dd>
                    </div>
                    <div>
                      <dt>Reported requests</dt>
                      <dd>
                        {detail.request_duration_ms === null
                          ? 'Duration unavailable'
                          : (detail.request_duration_ms / 1000).toFixed(1) + ' sec'}
                      </dd>
                    </div>
                    <div>
                      <dt>Subagents</dt>
                      <dd>
                        {s.subagents ? (
                          <button className="text-button" onClick={() => setTab('Relations')}>
                            {s.subagents} identified <ArrowRight size={11} />
                          </button>
                        ) : (
                          'None identified'
                        )}
                      </dd>
                    </div>
                    <div>
                      <dt>Models</dt>
                      <dd>{s.models.map(modelName).join(', ') || 'Unknown'}</dd>
                    </div>
                  </dl>
                  <p className="small-note">
                    Estimated activity sums bounded intervals between observed events. It is not a
                    measure of human working time.
                  </p>
                </div>
                <div className="inspector-section">
                  <div className="mini-heading">
                    <h3>Your notes & labels</h3>
                    <button className="text-button" onClick={() => setEditing((v) => !v)}>
                      <Pencil size={12} />
                      {editing ? 'Cancel' : 'Edit'}
                    </button>
                  </div>
                  {editing ? (
                    <form className="annotation-form" onSubmit={save}>
                      <Field label="Session alias">
                        <input
                          value={alias}
                          onChange={(e) => setAlias(e.target.value)}
                          maxLength={180}
                        />
                      </Field>
                      <Field label="Tags" hint="Separate tags with commas.">
                        <input
                          value={tags}
                          onChange={(e) => setTags(e.target.value)}
                          placeholder="frontend, research"
                        />
                      </Field>
                      <Field label="Notes">
                        <textarea
                          value={notes}
                          onChange={(e) => setNotes(e.target.value)}
                          rows={3}
                          placeholder="What should future you know?"
                        />
                      </Field>
                      <button className="button primary small" type="submit">
                        Save locally
                      </button>
                    </form>
                  ) : (
                    <>
                      <div className="session-tags">
                        {s.tags.map((t) => (
                          <Badge key={t}>
                            <Tag size={10} />
                            {t}
                          </Badge>
                        ))}
                      </div>
                      <p className={'notes-text ' + (!s.notes ? 'muted' : '')}>
                        {s.notes || 'Add a note to give this session a little context.'}
                      </p>
                    </>
                  )}
                </div>
                <Coverage detail={detail} />
              </>
            )}
            {tab === 'Timeline' && (
              <>
                <div className="mini-heading">
                  <h3>Usage timeline</h3>
                  <div className="segmented small">
                    <button
                      className={!cumulative ? 'active' : ''}
                      onClick={() => setCumulative(false)}
                    >
                      Intervals
                    </button>
                    <button
                      className={cumulative ? 'active' : ''}
                      onClick={() => setCumulative(true)}
                    >
                      Cumulative
                    </button>
                  </div>
                </div>
                <UsageChart
                  data={chart}
                  height={210}
                  rate={!cumulative}
                  onInterval={(start, end) => {
                    // Rust minute keys include the selected timezone's UTC offset, including DST.
                    const parseMinute = (key: string) =>
                      new Date(key.replace(' ', 'T').replace(/ ([+-]\d{2}:\d{2})$/, ':00$1'));
                    const first = parseMinute(start),
                      last = parseMinute(end);
                    if (!Number.isNaN(first.getTime()) && !Number.isNaN(last.getTime()))
                      onRange({
                        from: first.toISOString(),
                        to: new Date(last.getTime() + 60000).toISOString(),
                      });
                  }}
                />
                <p className="small-note">
                  Drag the handles to zoom and inspect an interval. Each interval is one minute;
                  gaps have no observed events.
                </p>
                {(range.from || range.to) && (
                  <button
                    className="button small"
                    onClick={() => {
                      onRange({});
                      setFrom('');
                      setTo('');
                    }}
                  >
                    Reset interval
                  </button>
                )}
                <div className="mini-heading">
                  <h3>{cumulative ? 'Cumulative' : 'Interval'} estimated cost</h3>
                </div>
                <UsageChart data={chart} cost height={150} />
                <form
                  className="timeline-range"
                  onSubmit={(e) => {
                    e.preventDefault();
                    onRange({
                      from: from ? new Date(from).toISOString() : undefined,
                      to: to ? new Date(to).toISOString() : undefined,
                    });
                  }}
                >
                  <div className="mini-heading">
                    <h3>Inspect an interval</h3>
                    <FilterIcon size={13} />
                  </div>
                  <Field label="From (device local time)">
                    <input
                      type="datetime-local"
                      value={from}
                      onChange={(e) => setFrom(e.target.value)}
                    />
                  </Field>
                  <Field label="Through (device local time)">
                    <input
                      type="datetime-local"
                      value={to}
                      onChange={(e) => setTo(e.target.value)}
                    />
                  </Field>
                  <button className="button small" type="submit">
                    Apply interval <ArrowRight size={12} />
                  </button>
                </form>
                <div className="interval-summary">
                  <span>{compact(u.total)} tokens</span>
                  <span>{usageCost(u)} estimated</span>
                  <span>{n(u.events)} events</span>
                </div>
                <div className="inspector-section">
                  <div className="mini-heading">
                    <h3>Recorded markers</h3>
                    <span>{detail.markers.length}</span>
                  </div>
                  {detail.markers.length ? (
                    detail.markers.map((m, i) => (
                      <div className="timeline-marker" key={i}>
                        <span className="marker-dot" />
                        <div>
                          <strong>{m.label}</strong>
                          <small>
                            {m.kind} ·{' '}
                            {time(m.timestamp, {
                              month: 'short',
                              day: 'numeric',
                              hour: '2-digit',
                              minute: '2-digit',
                            })}
                          </small>
                        </div>
                      </div>
                    ))
                  ) : (
                    <p className="small-note">
                      No model-switch, compaction, error, or retry markers were established in this
                      source.
                    </p>
                  )}
                </div>
              </>
            )}
            {tab === 'Events' && (
              <>
                <div className="mini-heading">
                  <h3>Usage events</h3>
                  <span>{n(detail.event_count)} recorded</span>
                </div>
                <div className="search-input inspector-search">
                  <Search size={13} />
                  <input
                    aria-label="Search usage events"
                    placeholder="Search model, ID, or scope…"
                    value={eventQuery.search}
                    onChange={(e) => onEventQuery({ search: e.target.value, offset: 0 })}
                  />
                </div>
                <EventTable events={detail.events} onSelect={setEvent} selected={event?.id} />
                <div className="event-pagination">
                  <span>
                    {detail.event_matches ? detail.event_offset + 1 : 0}–
                    {Math.min(detail.event_offset + detail.events.length, detail.event_matches)} of{' '}
                    {n(detail.event_matches)} matching events
                  </span>
                  <button
                    className="text-button"
                    disabled={!eventQuery.offset}
                    onClick={() =>
                      onEventQuery({ ...eventQuery, offset: Math.max(0, eventQuery.offset - 200) })
                    }
                  >
                    Previous events
                  </button>
                  <button
                    className="text-button"
                    disabled={eventQuery.offset + detail.events.length >= detail.event_matches}
                    onClick={() => onEventQuery({ ...eventQuery, offset: eventQuery.offset + 200 })}
                  >
                    Next events
                  </button>
                </div>
                {event && (
                  <div className="event-detail">
                    <div className="mini-heading">
                      <h3>Event provenance</h3>
                      <IconButton label="Close event details" onClick={() => setEvent(null)}>
                        <X size={12} />
                      </IconButton>
                    </div>
                    <dl className="detail-list">
                      <div>
                        <dt>ID</dt>
                        <dd className="break-all mono">{event.id}</dd>
                      </div>
                      <div>
                        <dt>Model</dt>
                        <dd>{event.model}</dd>
                      </div>
                      <div>
                        <dt>Scope / kind</dt>
                        <dd>
                          {event.scope} / {event.kind}
                        </dd>
                      </div>
                      <div>
                        <dt>Tokens</dt>
                        <dd>{n(event.usage.total)}</dd>
                      </div>
                      <div>
                        <dt>Input / output</dt>
                        <dd>
                          {category(event.usage, 'input')} / {category(event.usage, 'output')}
                        </dd>
                      </div>
                      <div>
                        <dt>Cache read / write</dt>
                        <dd>
                          {category(event.usage, 'cache_read')} /{' '}
                          {category(event.usage, 'cache_write')}
                        </dd>
                      </div>
                      <div>
                        <dt>Estimated cost</dt>
                        <dd>{usageCost(event.usage, 6)}</dd>
                      </div>
                      <div>
                        <dt>Request</dt>
                        <dd className="break-all mono">{event.request_id || 'Unavailable'}</dd>
                      </div>
                      <div>
                        <dt>Turn</dt>
                        <dd className="break-all mono">{event.turn_id || 'Unavailable'}</dd>
                      </div>
                      <div>
                        <dt>Usage</dt>
                        <dd>{event.reported ? 'Provider reported' : 'Derived increment'}</dd>
                      </div>
                      <div>
                        <dt>Parser</dt>
                        <dd>{event.parser_version}</dd>
                      </div>
                      <div>
                        <dt>Pricing version</dt>
                        <dd>{event.pricing_version || 'Unpriced'}</dd>
                      </div>
                      <div>
                        <dt>Source</dt>
                        <dd className="break-all mono">
                          {event.source_path}:{event.source_line}
                        </dd>
                      </div>
                    </dl>
                    {event.warnings.map((w, i) => (
                      <p key={i} className="warning-note">
                        {w}
                      </p>
                    ))}
                    <p className="small-note">
                      This canonical event contributes {n(event.usage.total)} tokens once. Cache
                      categories are exclusive of uncached input; reasoning is included in output.
                    </p>
                  </div>
                )}
                <p className="small-note">
                  Usage metadata only. Prompt bodies and tool arguments are not indexed.
                </p>
              </>
            )}
            {tab === 'Models' && (
              <>
                <div className="mini-heading">
                  <h3>Model contributions</h3>
                  <span>{detail.models.length} models</span>
                </div>
                {detail.models.map((m, i) => (
                  <div className="model-card" key={m.key}>
                    <div>
                      <span className="model-color" style={{ background: CHART_COLORS[i % 5] }} />
                      <strong>{modelName(m.label)}</strong>
                      <b>{usageCost(m)}</b>
                    </div>
                    <div className="progress-track">
                      <i
                        style={{
                          width: (m.total / (u.total || 1)) * 100 + '%',
                          background: CHART_COLORS[i % 5],
                        }}
                      />
                    </div>
                    <small>
                      {compact(m.total)} tokens ·{' '}
                      {u.total ? ((m.total / u.total) * 100).toFixed(1) : '0'}% of session
                    </small>
                    {m.unpriced_tokens > 0 && (
                      <p className="warning-note">
                        {n(m.unpriced_tokens)} tokens have no known price.
                      </p>
                    )}
                  </div>
                ))}
                <div className="inspector-section">
                  <h3>Cache behavior</h3>
                  <div className="cache-value">
                    {cacheRatio === null ? 'Unavailable' : cacheRatio.toFixed(1) + '%'}
                    <span>input served from cache</span>
                  </div>
                  <p className="small-note">
                    {n(u.cache_read)} cache-read tokens divided by {n(inputTotal)} total input
                    tokens. Total input includes uncached input, cache reads, and cache writes.
                  </p>
                  <p className="small-note">
                    Estimated cost uses each event’s recorded pricing version. No hypothetical cache
                    savings are inferred.
                  </p>
                </div>
                <Coverage detail={detail} />
              </>
            )}
            {tab === 'Context' && (
              <>
                <div className="context-card">
                  <Network size={25} />
                  <span className="eyebrow">LAST OBSERVED CONTEXT</span>
                  <strong>
                    {detail.context.used === null ? 'Unavailable' : compact(detail.context.used)}
                  </strong>
                  {detail.context.capacity !== null && detail.context.used !== null ? (
                    <>
                      <div className="progress-track">
                        <i
                          style={{
                            width:
                              Math.min(100, (detail.context.used / detail.context.capacity) * 100) +
                              '%',
                          }}
                        />
                      </div>
                      <p>
                        {((detail.context.used / detail.context.capacity) * 100).toFixed(1)}% of{' '}
                        {compact(detail.context.capacity)} tokens
                      </p>
                    </>
                  ) : (
                    <p>
                      {detail.context.used === null
                        ? 'This source does not report reliable context occupancy.'
                        : 'Context capacity is unavailable. A percentage cannot be calculated.'}
                    </p>
                  )}
                  {detail.context.observed_at && (
                    <small>
                      Observed {date(detail.context.observed_at)} at{' '}
                      {time(detail.context.observed_at)}
                    </small>
                  )}
                </div>
                <div className="info-box">
                  <CircleHelp size={16} />
                  <span>
                    Lifetime token throughput ({compact(u.total)}) is different from current context
                    occupancy. Repeated and cached input can be counted across many requests.
                  </span>
                </div>
                {detail.carry_in.total > 0 && (
                  <div className="inspector-section">
                    <h3>Historical carry-in</h3>
                    <strong className="large-number">
                      {compact(detail.carry_in.total)} tokens
                    </strong>
                    <p className="small-note">
                      Usage already present in a cumulative counter at the beginning of this
                      excerpt. Kept separate from observed increments in this session.
                    </p>
                  </div>
                )}
              </>
            )}
            {tab === 'Relations' && (
              <>
                <div className="mini-heading">
                  <h3>Session relationships</h3>
                  <GitBranch size={15} />
                </div>
                <div className="own-combined">
                  <div>
                    <small>OWN TOKENS</small>
                    <strong>{compact(u.total)}</strong>
                  </div>
                  <ArrowRight size={16} />
                  <div>
                    <small>WITH DESCENDANTS</small>
                    <strong>{compact(s.combined_usage.total)}</strong>
                  </div>
                </div>
                <div className="relationship current">
                  <ProviderIcon provider={s.provider} />
                  <div>
                    <strong>{s.name}</strong>
                    <small>Selected session · {compact(u.total)} tokens</small>
                  </div>
                  <Badge>You are here</Badge>
                </div>
                {detail.relationships.map((r) => (
                  <button key={r.id} className="relationship" onClick={() => onNavigate(r.id)}>
                    <GitBranch size={16} />
                    <div>
                      <strong>{r.name}</strong>
                      <small>
                        {r.id === s.parent_id ? 'Parent session' : 'Related / subagent'} ·{' '}
                        {compact(r.usage.total)} tokens
                      </small>
                    </div>
                    <ChevronRight size={14} />
                  </button>
                ))}
                {!detail.relationships.length && (
                  <p className="small-note">
                    No parent or subagent relationships were identified in the available metadata.
                  </p>
                )}
                <div className="info-box">
                  <Layers3 size={15} />
                  <span>
                    Combined usage includes this session and identified descendants. Global totals
                    count each canonical event once.
                  </span>
                </div>
              </>
            )}
          </div>
          <div className="inspector-footer">
            <ShieldCheck size={11} />
            LOCAL METADATA · SOURCE LOGS UNCHANGED
          </div>
        </>
      )}
    </aside>
  );
}
function UsageRow({
  label,
  value,
  color,
  unknown,
}: {
  label: string;
  value: number;
  color: string;
  unknown?: boolean;
}) {
  return (
    <div className="usage-row">
      <span>
        <i style={{ background: color }} />
        {label}
      </span>
      <strong>{unknown ? 'Unavailable' : n(value)}</strong>
    </div>
  );
}
function Coverage({ detail }: { detail: Detail }) {
  return (
    <div className="coverage">
      <div>
        <ShieldCheck size={13} />
        <strong>Source coverage</strong>
      </div>
      {detail.coverage.map((c, i) => (
        <p key={i}>{c}</p>
      ))}
      {detail.session.warnings.map((w, i) => (
        <p key={i} className="warning-note">
          {w}
        </p>
      ))}
      {(detail.session.usage.unknown_fields?.length || 0) > 0 && (
        <p className="warning-note">
          Unknown categories: {(detail.session.usage.unknown_fields || []).join(', ')}. Totals are a
          lower bound.
        </p>
      )}
    </div>
  );
}
function EventTable({
  events,
  onSelect,
  selected,
}: {
  events: UsageEvent[];
  onSelect: (e: UsageEvent) => void;
  selected?: string;
}) {
  const columns = useMemo<ColumnDef<UsageEvent>[]>(
    () => [
      {
        header: 'Time',
        accessorKey: 'timestamp',
        cell: (i) =>
          time(i.getValue() as string, { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
      },
      { header: 'Model', accessorKey: 'model', cell: (i) => modelName(i.getValue() as string) },
      {
        header: 'Tokens',
        accessorFn: (e) => e.usage.total,
        cell: (i) => compact(i.getValue() as number),
      },
      {
        header: 'Est. cost',
        accessorFn: (e) => e.usage.cost,
        cell: (i) => usageCost(i.row.original.usage, 3),
      },
    ],
    [],
  );
  const table = useReactTable({ data: events, columns, getCoreRowModel: getCoreRowModel() });
  return (
    <div className="event-table-wrap">
      <table className="data-table event-table">
        <thead>
          {table.getHeaderGroups().map((g) => (
            <tr key={g.id}>
              {g.headers.map((h) => (
                <th key={h.id}>{flexRender(h.column.columnDef.header, h.getContext())}</th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((r) => (
            <tr
              key={r.id}
              className={selected === r.original.id ? 'selected' : ''}
              onClick={() => onSelect(r.original)}
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelect(r.original);
                }
              }}
            >
              {r.getVisibleCells().map((c) => (
                <td key={c.id}>{flexRender(c.column.columnDef.cell, c.getContext())}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {!events.length && <p className="small-note">No matching usage events.</p>}
    </div>
  );
}

function Analytics({
  snapshot,
  filter,
  onFilter,
  onDrill,
  onSession,
  onExport,
}: {
  snapshot: Snapshot | null;
  filter: Filter;
  onFilter: (v: Partial<Filter>) => void;
  onDrill: (v: Partial<Filter>) => void;
  onSession: (id: string) => void;
  onExport: () => void;
}) {
  const [metric, setMetric] = useState<'tokens' | 'cost'>('tokens');
  const period = filter.period || (filter.from || filter.to ? 'custom' : 'all');
  const selectPeriod = (p: string) => {
    onFilter({ period: p, from: undefined, to: undefined });
  };
  if (!snapshot)
    return (
      <Empty
        icon={ChartNoAxesCombined}
        title="Reading your usage"
        description="Analytics are calculated from the local index."
      />
    );
  const u = snapshot.totals,
    previous = snapshot.previous?.total;
  const complete =
    !u.unknown_fields?.some((k) => k !== 'reasoning') &&
    !snapshot.previous?.unknown_fields?.some((k) => k !== 'reasoning');
  const change = previous && complete ? ((u.total - previous) / previous) * 100 : null;
  const dayDrill = (key: string) =>
    onDrill({ period: undefined, from: key.slice(0, 10), to: key.slice(0, 10) });
  return (
    <div className="standard-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">THE BIGGER PICTURE</div>
          <h1>Usage analytics</h1>
          <p>Understand your patterns. Make every token count.</p>
        </div>
        <button className="button" onClick={onExport}>
          <Download size={14} />
          Export
        </button>
      </div>
      <div className="analytics-filterbar">
        <div className="segmented">
          {[
            ['all', 'All time'],
            ['today', 'Today'],
            ['week', 'This Week'],
            ['month', 'This Month'],
            ['7', '7 days'],
            ['30', '30 days'],
            ['90', '90 days'],
            ['custom', 'Custom'],
          ].map(([v, l]) => (
            <button
              key={v}
              className={period === v ? 'active' : ''}
              onClick={() => selectPeriod(v)}
            >
              {l}
            </button>
          ))}
        </div>
        {period === 'custom' && (
          <>
            <input
              aria-label="Analytics start date"
              type="date"
              value={filter.from?.slice(0, 10) || ''}
              onChange={(e) => onFilter({ period: undefined, from: e.target.value || undefined })}
            />
            <span>to</span>
            <input
              aria-label="Analytics end date"
              type="date"
              value={filter.to?.slice(0, 10) || ''}
              onChange={(e) => onFilter({ period: undefined, to: e.target.value || undefined })}
            />
          </>
        )}
        <span className="muted small-text">Weeks start Monday</span>
      </div>
      <div className="metrics-strip analytics-metrics">
        <Metric
          icon={Layers3}
          label="Observed tokens"
          value={compact(u.total)}
          note={
            change === null
              ? !complete
                ? 'Comparison unavailable: incomplete token categories'
                : previous === 0
                  ? 'Prior period has zero observed tokens'
                  : 'No comparable prior period'
              : (change >= 0 ? '+' : '') +
                change.toFixed(1) +
                '% · ' +
                snapshot.reporting.comparison
          }
        />
        <Metric
          icon={Wallet}
          label="API-equivalent cost"
          value={u.unpriced_tokens === u.total && u.total > 0 ? 'Unpriced' : usageCost(u)}
          note={costCoverage(u, 'Known prices for observed events')}
          accent
        />
        <Metric
          icon={Activity}
          label="Sessions"
          value={n(snapshot.total_sessions)}
          note={
            compact(snapshot.total_sessions ? u.total / snapshot.total_sessions : 0) +
            ' tokens per session on average'
          }
        />
        <Metric
          icon={Zap}
          label="Input served from cache"
          value={
            u.input + u.cache_read + u.cache_write &&
            !u.unknown_fields?.some((k) => ['input', 'cache_read', 'cache_write'].includes(k))
              ? ((u.cache_read / (u.input + u.cache_read + u.cache_write)) * 100).toFixed(1) + '%'
              : '—'
          }
          note="Cache reads ÷ all input tokens"
        />
      </div>
      {snapshot.previous_complete && (
        <p className="small-note">
          Complete prior calendar period: {n(snapshot.previous_complete.total)} observed tokens
          {' · '}
          {snapshot.reporting.previous_from && fmtDate(snapshot.reporting.previous_from)}
          {' → '}
          {snapshot.reporting.prior_complete_to && fmtDate(snapshot.reporting.prior_complete_to)}
          {' (end exclusive). Percentage comparisons use matching progress to date.'}
        </p>
      )}
      <section className="panel trend-panel">
        <SectionHeading eyebrow="OVER TIME" title="A little perspective">
          <div className="segmented small">
            <button
              className={metric === 'tokens' ? 'active' : ''}
              onClick={() => setMetric('tokens')}
            >
              Tokens
            </button>
            <button className={metric === 'cost' ? 'active' : ''} onClick={() => setMetric('cost')}>
              Est. cost
            </button>
          </div>
        </SectionHeading>
        <UsageChart
          data={snapshot.daily}
          height={235}
          cost={metric === 'cost'}
          stacked={metric === 'tokens'}
          onClick={dayDrill}
        />
        <div className="chart-legend">
          {metric === 'cost' ? (
            <span>
              <i style={{ background: 'var(--amber)' }} />
              Estimated cost (USD)
            </span>
          ) : (
            <>
              <span>
                <i style={{ background: 'var(--amber)' }} />
                Input
              </span>
              <span>
                <i style={{ background: 'var(--mint)' }} />
                Output
              </span>
              <span>
                <i style={{ background: 'var(--purple)' }} />
                Cache read
              </span>
              <span>
                <i style={{ background: 'var(--blue)' }} />
                Cache write
              </span>
            </>
          )}
          <span className="legend-hint">Click a day to explore sessions</span>
        </div>
      </section>
      <div className="two-column-grid">
        <section className="panel">
          <SectionHeading eyebrow="MODEL MIX" title="Where your tokens go" />
          <Rankings
            data={snapshot.models}
            onClick={(key) => onDrill({ model: key })}
            total={u.total}
          />
        </section>
        <section className="panel">
          <SectionHeading eyebrow="BY PROJECT" title="Your busiest workspaces" />
          {snapshot.projects
            .slice()
            .sort((a, b) => b.usage.total - a.usage.total)
            .slice(0, 6)
            .map((p, i) => (
              <button
                className="ranking project-ranking"
                key={p.path}
                onClick={() => onDrill({ project: p.path })}
              >
                <span className="ranking-index">{String(i + 1).padStart(2, '0')}</span>
                <span
                  className="project-color"
                  style={{ background: p.color || CHART_COLORS[i % 5] }}
                />
                <div>
                  <strong>{p.name}</strong>
                  <span>{p.sessions} sessions</span>
                </div>
                <Sparkline values={p.sparkline} width={65} />
                <b>{compact(p.usage.total)}</b>
                <ArrowUpRight size={13} />
              </button>
            ))}
        </section>
      </div>
      <div className="two-column-grid">
        <section className="panel">
          <SectionHeading eyebrow="DAILY RHYTHM" title="By hour of day" />
          <BucketBars data={snapshot.hours} />
        </section>
        <section className="panel">
          <SectionHeading eyebrow="WEEKLY RHYTHM" title="By day of week" />
          <BucketBars data={snapshot.weekdays} />
        </section>
      </div>
      <div className="two-column-grid">
        <section className="panel">
          <SectionHeading eyebrow="PROVIDERS" title="Across your tools" />
          <Rankings
            data={snapshot.providers}
            onClick={(key) => onDrill({ provider: key })}
            total={u.total}
          />
        </section>
        <section className="panel">
          <SectionHeading eyebrow="TAKE A CLOSER LOOK" title="Highest-usage sessions" />
          {snapshot.top_sessions.map((s) => (
            <button key={s.id} className="ranking" onClick={() => onSession(s.id)}>
              <ProviderIcon provider={s.provider} />
              <div>
                <strong>{s.name}</strong>
                <span>{s.project}</span>
              </div>
              <b>{compact(s.usage.total)}</b>
              <ChevronRight size={13} />
            </button>
          ))}
        </section>
      </div>
    </div>
  );
}
function Rankings({
  data,
  total,
  onClick,
}: {
  data: Bucket[];
  total: number;
  onClick: (key: string) => void;
}) {
  return (
    <div className="rankings">
      {data.map((b, i) => (
        <button className="breakdown-row" key={b.key} onClick={() => onClick(b.key)}>
          <div>
            <span className="model-color" style={{ background: CHART_COLORS[i % 5] }} />
            <strong>{modelName(b.label)}</strong>
            <b>{compact(b.total)}</b>
            <span>{total ? ((b.total / total) * 100).toFixed(0) : 0}%</span>
            <ArrowUpRight size={12} />
          </div>
          <div className="progress-track">
            <i
              style={{
                width: (b.total / (total || 1)) * 100 + '%',
                background: CHART_COLORS[i % 5],
              }}
            />
          </div>
        </button>
      ))}
      {!data.length && <p className="small-note">No usage in this view.</p>}
    </div>
  );
}
function BucketBars({ data }: { data: Bucket[] }) {
  return (
    <div className="chart" style={{ height: 180 }}>
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={data} margin={{ left: -18, right: 3, top: 12, bottom: 0 }}>
          <CartesianGrid vertical={false} stroke="var(--chart-grid)" strokeDasharray="3 5" />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            tick={{ fill: 'var(--text-muted)', fontSize: 10 }}
          />
          <YAxis
            tickFormatter={compact}
            tickLine={false}
            axisLine={false}
            tick={{ fill: 'var(--text-muted)', fontSize: 10 }}
          />
          <Tooltip content={<ChartTooltip />} />
          <Bar
            isAnimationActive={false}
            dataKey="total"
            name="Observed tokens"
            fill="#8fc6b4"
            radius={[3, 3, 0, 0]}
            maxBarSize={26}
          />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

function Projects({
  snapshot,
  onDrill,
  run,
  onToast,
}: {
  snapshot: Snapshot | null;
  onDrill: (f: Partial<Filter>) => void;
  run: Run;
  onToast: (s: string) => void;
}) {
  const [search, setSearch] = useState('');
  const [edit, setEdit] = useState<Project | null>(null);
  const [name, setName] = useState('');
  const [color, setColor] = useState('#8fc6b4');
  const [notes, setNotes] = useState('');
  const [aliases, setAliases] = useState('');
  const open = (p: Project) => {
    setEdit(p);
    setName(p.name);
    setColor(p.color || '#8fc6b4');
    setNotes(p.notes);
    setAliases(p.aliases.join('\n'));
  };
  return (
    <div className="standard-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">ACROSS YOUR WORKSPACES</div>
          <h1>
            Projects<span className="heading-count">{snapshot?.projects.length || 0}</span>
          </h1>
          <p>See the shape of your work, one repository at a time.</p>
        </div>
        <div className="search-input project-search">
          <Search size={15} />
          <input
            aria-label="Search projects"
            placeholder="Find a project…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      </div>
      <div className="project-grid">
        {snapshot?.projects
          .filter((p) => (p.name + ' ' + p.path).toLowerCase().includes(search.toLowerCase()))
          .sort((a, b) => Number(b.favorite) - Number(a.favorite) || b.usage.total - a.usage.total)
          .map((p) => (
            <article className="panel project-card" key={p.path}>
              <div className="project-card-heading">
                <span
                  className="folder-tile"
                  style={{
                    color: p.color || 'var(--mint)',
                    background:
                      'color-mix(in srgb, ' + (p.color || 'var(--mint)') + ' 8%, transparent)',
                  }}
                >
                  <Folder size={22} />
                </span>
                <div>
                  <button className="project-title" onClick={() => onDrill({ project: p.path })}>
                    {p.name}
                    <ArrowUpRight size={13} />
                  </button>
                  <span>
                    {p.sessions} sessions · {p.models.length} models
                  </span>
                </div>
                <IconButton
                  label={p.favorite ? 'Unfavorite project' : 'Favorite project'}
                  className={p.favorite ? 'amber-text' : ''}
                  onClick={() => run('project_save', { path: p.path, favorite: !p.favorite })}
                >
                  <Star size={15} fill={p.favorite ? 'currentColor' : 'none'} />
                </IconButton>
              </div>
              <div className="project-card-metrics">
                <div>
                  <span>OBSERVED TOKENS</span>
                  <strong>{compact(p.usage.total)}</strong>
                </div>
                <div>
                  <span>API ESTIMATE</span>
                  <strong>{usageCost(p.usage)}</strong>
                </div>
                <Sparkline
                  values={p.sparkline}
                  width={100}
                  height={40}
                  color={p.color || 'var(--mint)'}
                />
              </div>
              <div className="project-models">
                {p.models.slice(0, 3).map((m) => (
                  <Badge key={m}>{modelName(m)}</Badge>
                ))}
                {p.models.length > 3 && <Badge>+{p.models.length - 3}</Badge>}
              </div>
              {p.notes && <p className="project-notes">{p.notes}</p>}
              <div className="project-path" title={p.path}>
                <Folder size={11} />
                <span>{p.path}</span>
              </div>
              {p.aliases.length > 0 && (
                <p className="small-note">Grouped with {p.aliases.length} explicit path aliases</p>
              )}
              <div className="project-card-footer">
                <button className="text-button" onClick={() => onDrill({ project: p.path })}>
                  Explore sessions <ArrowRight size={12} />
                </button>
                <IconButton label="Edit project" onClick={() => open(p)}>
                  <Pencil size={13} />
                </IconButton>
                <IconButton
                  label="Reveal project folder"
                  onClick={() => revealPath(p.path).catch((e) => onToast(String(e)))}
                >
                  <FolderOpen size={13} />
                </IconButton>
              </div>
            </article>
          ))}
      </div>
      {!snapshot?.projects.length && (
        <Empty
          icon={Folder}
          title="Every project has a story"
          description="Projects appear automatically from the working-directory metadata in your indexed sessions."
        />
      )}
      <Modal
        open={!!edit}
        onClose={() => setEdit(null)}
        title="Edit project"
        description="Personalize this workspace. Source logs remain unchanged."
      >
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!edit) return;
            const result = await run(
              'project_save',
              {
                path: edit.path,
                name,
                color,
                notes,
                aliases: aliases
                  .split('\n')
                  .map((a) => a.trim())
                  .filter(Boolean),
              },
              'Project updated',
            );
            if (result) setEdit(null);
          }}
        >
          <div className="dialog-body">
            <Field label="Display name">
              <input required value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field label="Project color">
              <input type="color" value={color} onChange={(e) => setColor(e.target.value)} />
            </Field>
            <Field label="Notes">
              <textarea rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} />
            </Field>
            <Field
              label="Related absolute paths"
              hint="One exact path per line. Explicitly group worktrees; shared folder names never merge automatically."
            >
              <textarea
                rows={3}
                value={aliases}
                onChange={(e) => setAliases(e.target.value)}
                placeholder="/Users/you/Code/project-worktree"
              />
            </Field>
          </div>
          <div className="dialog-footer">
            <button type="button" className="button" onClick={() => setEdit(null)}>
              Cancel
            </button>
            <button type="submit" className="button primary">
              Save project
            </button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function Compare({
  snapshot,
  ids,
  setIds,
  demo,
  filter,
  onSession,
}: {
  snapshot: Snapshot | null;
  ids: string[];
  setIds: (v: string[]) => void;
  demo: boolean;
  filter: Filter;
  onSession: (id: string) => void;
}) {
  const [mode, setMode] = useState('sessions');
  const [items, setItems] = useState<ComparisonItem[]>([]);
  const [alignment, setAlignment] = useState<'elapsed' | 'wall'>('elapsed');
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const [ranges, setRanges] = useState([
    { from: '', to: '' },
    { from: '', to: '' },
  ]);
  const [rangeVersion, setRangeVersion] = useState(0);
  const [search, setSearch] = useState('');
  useEffect(() => {
    let active = true;
    if (mode === 'sessions' && ids.length === 0) {
      setItems([]);
      return;
    }
    if (mode === 'ranges' && ranges.some((r) => !r.from || !r.to)) return;
    setPending(true);
    api(
      'compare',
      {
        ids: mode === 'sessions' ? ids : undefined,
        ranges: mode === 'ranges' ? ranges : undefined,
        filter,
        alignment,
      },
      demo,
    )
      .then((r) => {
        if (active) {
          setItems(r.items);
          setError('');
        }
      })
      .catch((e) => active && setError(String(e)))
      .finally(() => active && setPending(false));
    return () => {
      active = false;
    };
  }, [ids, demo, filter, alignment, mode, rangeVersion]);
  const base = items[0];
  const difference = (value: number, old: number) =>
    !old
      ? value
        ? 'No baseline'
        : '0%'
      : (((value - old) / old) * 100 >= 0 ? '+' : '') +
        (((value - old) / old) * 100).toFixed(1) +
        '%';
  const chartRows = useMemo(() => {
    const records: Record<
      string,
      { key: string; label: string; x: number; [series: string]: string | number }
    > = {};
    items.forEach((item, i) =>
      item.timeline.forEach((b) => {
        const key = b.key;
        if (!records[key])
          records[key] = {
            key,
            x: b.x ?? 0,
            label: alignment === 'elapsed' ? parseInt(b.key, 10) + ' min' : b.label,
          };
        records[key]['series' + i] = b.total;
      }),
    );
    return Object.values(records)
      .map((row) => {
        items.forEach((_, i) => {
          row['series' + i] ??= 0;
        });
        return row;
      })
      .sort((a, b) => a.x - b.x);
  }, [items, alignment]);
  return (
    <div className="standard-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">SIDE BY SIDE</div>
          <h1>Compare</h1>
          <p>A clearer look at the differences, without the guesswork.</p>
        </div>
        <div className="segmented">
          <button
            className={mode === 'sessions' ? 'active' : ''}
            onClick={() => setMode('sessions')}
          >
            Sessions
          </button>
          <button className={mode === 'ranges' ? 'active' : ''} onClick={() => setMode('ranges')}>
            Date ranges
          </button>
        </div>
      </div>
      {mode === 'sessions' ? (
        <section className="panel compare-picker">
          <div className="mini-heading">
            <h3>Choose 2–4 sessions</h3>
            <span>{ids.length} selected</span>
          </div>
          <div className="search-input">
            <Search size={14} />
            <input
              aria-label="Find sessions to compare"
              placeholder="Search sessions to compare…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <div className="compare-options">
            {snapshot?.sessions
              .filter((s) =>
                (s.name + ' ' + s.project).toLowerCase().includes(search.toLowerCase()),
              )
              .slice(0, search ? 40 : 12)
              .map((s) => (
                <label
                  key={s.id}
                  className={'compare-option ' + (ids.includes(s.id) ? 'selected' : '')}
                >
                  <input
                    type="checkbox"
                    checked={ids.includes(s.id)}
                    disabled={!ids.includes(s.id) && ids.length >= 4}
                    onChange={(e) =>
                      setIds(e.target.checked ? [...ids, s.id] : ids.filter((id) => id !== s.id))
                    }
                  />
                  <ProviderIcon provider={s.provider} />
                  <span>
                    {s.name}
                    <small>
                      {s.project} · {compact(s.usage.total)} tokens
                    </small>
                  </span>
                </label>
              ))}
          </div>
          {ids.length > 0 && (
            <button className="text-button" onClick={() => setIds([])}>
              Clear selection
            </button>
          )}
        </section>
      ) : (
        <form
          className="panel range-compare-form"
          onSubmit={(e) => {
            e.preventDefault();
            setRangeVersion((v) => v + 1);
          }}
        >
          {ranges.map((r, i) => (
            <div key={i}>
              <h3>{i === 0 ? 'Baseline' : 'Comparison'} range</h3>
              <Field label="From">
                <input
                  required
                  type="date"
                  value={r.from}
                  onChange={(e) =>
                    setRanges(ranges.map((v, j) => (j === i ? { ...v, from: e.target.value } : v)))
                  }
                />
              </Field>
              <Field label="Through">
                <input
                  required
                  type="date"
                  min={r.from}
                  value={r.to}
                  onChange={(e) =>
                    setRanges(ranges.map((v, j) => (j === i ? { ...v, to: e.target.value } : v)))
                  }
                />
              </Field>
            </div>
          ))}
          <button className="button primary" type="submit">
            Compare ranges
          </button>
        </form>
      )}
      {error && (
        <div className="info-box warning">
          <CircleAlert size={15} />
          {error}
        </div>
      )}
      {pending && (
        <div className="inline-loading">
          <LoaderCircle className="spin" size={15} />
          Calculating comparison…
        </div>
      )}
      {items.length > 0 && (
        <>
          <div
            className="comparison-grid"
            style={{
              gridTemplateColumns: 'repeat(' + Math.min(items.length, 4) + ', minmax(0,1fr))',
            }}
          >
            {items.map((item, i) => (
              <section className="panel comparison-card" key={i}>
                <div className="mini-heading">
                  <Badge tone={i === 0 ? 'amber' : 'mint'}>
                    {i === 0 ? 'Baseline' : 'Comparison ' + i}
                  </Badge>
                  {mode === 'sessions' && (
                    <IconButton
                      label="Remove from comparison"
                      onClick={() => setIds(ids.filter((id) => id !== item.id))}
                    >
                      <X size={13} />
                    </IconButton>
                  )}
                </div>
                <h3>{item.label}</h3>
                <strong className="compare-total">
                  {compact(item.usage.total)}
                  <small>tokens</small>
                </strong>
                {i > 0 && base && (
                  <div className="compare-delta">
                    {difference(item.usage.total, base.usage.total)}{' '}
                    <span>
                      ({item.usage.total >= base.usage.total ? '+' : ''}
                      {compact(item.usage.total - base.usage.total)} tokens)
                    </span>
                  </div>
                )}
                <dl className="detail-list">
                  <div>
                    <dt>Uncached input</dt>
                    <dd>{compact(item.usage.input)}</dd>
                  </div>
                  <div>
                    <dt>Output</dt>
                    <dd>{compact(item.usage.output)}</dd>
                  </div>
                  <div>
                    <dt>Cache read</dt>
                    <dd>{compact(item.usage.cache_read)}</dd>
                  </div>
                  <div>
                    <dt>Cache write</dt>
                    <dd>{compact(item.usage.cache_write)}</dd>
                  </div>
                  <div>
                    <dt>Reasoning (in output)</dt>
                    <dd>{compact(item.usage.reasoning)}</dd>
                  </div>
                  <div>
                    <dt>Est. cost</dt>
                    <dd>{usageCost(item.usage)}</dd>
                  </div>
                  <div>
                    <dt>Unpriced tokens</dt>
                    <dd>{compact(item.usage.unpriced_tokens)}</dd>
                  </div>
                  <div>
                    <dt>Elapsed span</dt>
                    <dd>{elapsed(item.elapsed_seconds)}</dd>
                  </div>
                  <div>
                    <dt>Estimated activity</dt>
                    <dd>{elapsed(item.active_seconds)}</dd>
                  </div>
                </dl>
                {i > 0 &&
                  base &&
                  item.usage.unpriced_tokens < item.usage.total &&
                  base.usage.unpriced_tokens < base.usage.total && (
                    <p className="small-note">
                      Estimated cost difference:{' '}
                      {item.usage.inferred_price_tokens || base.usage.inferred_price_tokens
                        ? '≈'
                        : ''}
                      {money(item.usage.cost - base.usage.cost)} (
                      {difference(item.usage.cost, base.usage.cost)}).
                    </p>
                  )}
                <div className="project-models">
                  {item.models.map((m) => (
                    <Badge key={m}>{modelName(m)}</Badge>
                  ))}
                </div>
                {mode === 'sessions' && item.id && (
                  <button className="text-button" onClick={() => onSession(item.id!)}>
                    Inspect session <ArrowRight size={12} />
                  </button>
                )}
              </section>
            ))}
          </div>
          <section className="panel">
            <SectionHeading title="Usage timelines">
              <div className="segmented small">
                <button
                  className={alignment === 'elapsed' ? 'active' : ''}
                  onClick={() => setAlignment('elapsed')}
                >
                  Elapsed time
                </button>
                <button
                  className={alignment === 'wall' ? 'active' : ''}
                  onClick={() => setAlignment('wall')}
                >
                  Wall clock
                </button>
              </div>
            </SectionHeading>
            <div className="chart" style={{ height: 220 }}>
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={chartRows} margin={{ left: -12, right: 8, top: 10, bottom: 0 }}>
                  <CartesianGrid
                    vertical={false}
                    stroke="var(--chart-grid)"
                    strokeDasharray="3 5"
                  />
                  <XAxis
                    dataKey="x"
                    type="number"
                    scale="linear"
                    domain={['dataMin', 'dataMax']}
                    tickFormatter={(x: number) =>
                      alignment === 'elapsed'
                        ? Math.round(x / 60000) + ' min'
                        : fmtDate(new Date(x).toISOString())
                    }
                    tick={{ fill: 'var(--text-muted)', fontSize: 10 }}
                    axisLine={false}
                    tickLine={false}
                  />
                  <YAxis
                    tickFormatter={compact}
                    tick={{ fill: 'var(--text-muted)', fontSize: 10 }}
                    axisLine={false}
                    tickLine={false}
                  />
                  <Tooltip
                    content={
                      <ChartTooltip
                        formatLabel={(x) =>
                          chartRows.find((b) => b.x === Number(x))?.label || String(x)
                        }
                      />
                    }
                  />
                  {items.map((item, i) => (
                    <Line
                      isAnimationActive={false}
                      key={i}
                      dataKey={'series' + i}
                      name={item.label}
                      stroke={CHART_COLORS[i]}
                      dot={false}
                      strokeWidth={2}
                      connectNulls={false}
                    />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            </div>
            <div className="chart-legend">
              {items.map((item, i) => (
                <span key={i}>
                  <i style={{ background: CHART_COLORS[i] }} />
                  {item.label}
                </span>
              ))}
            </div>
          </section>
          <p className="workspace-footnote">
            <CircleHelp size={12} />
            Usage comparisons describe observed activity. They are not productivity scores.
          </p>
        </>
      )}
      {!items.length && !pending && (
        <Empty
          icon={Columns3}
          title="Find the meaningful differences"
          description="Select sessions above, or compare two date ranges, to see tokens, cache behavior, estimated cost, and activity side by side."
        />
      )}
    </div>
  );
}

function budgetIncomplete(b: Budget) {
  return (
    (b.unit === 'usd' && b.unpriced_tokens > 0) ||
    b.unknown_fields?.some((k) => b.unit === 'usd' || k !== 'reasoning')
  );
}
function Budgets({ snapshot, run }: { snapshot: Snapshot | null; run: Run }) {
  const [open, setOpen] = useState(false);
  const [edit, setEdit] = useState<Budget | null>(null);
  const [name, setName] = useState('');
  const [amount, setAmount] = useState('100');
  const [unit, setUnit] = useState<'usd' | 'tokens'>('usd');
  const [period, setPeriod] = useState<'day' | 'month' | '5h' | 'window'>('month');
  const [windowMinutes, setWindowMinutes] = useState('300');
  const [project, setProject] = useState('');
  const [threshold, setThreshold] = useState('80');
  const show = (b?: Budget) => {
    setEdit(b || null);
    setName(b?.name || '');
    setAmount(String(b?.amount || 100));
    setUnit(b?.unit || 'usd');
    setPeriod(b?.period || 'month');
    setWindowMinutes(String(b?.window_minutes || 300));
    setProject(b?.project || '');
    setThreshold(String(b?.threshold || 80));
    setOpen(true);
  };
  return (
    <div className="standard-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">STAY IN THE KNOW</div>
          <h1>Budgets & limits</h1>
          <p>Your own guardrails, with a clear view of what is actually reported.</p>
        </div>
        <button className="button primary" onClick={() => show()}>
          <Plus size={15} />
          Create budget
        </button>
      </div>
      <div className="info-box">
        <CircleHelp size={16} />
        <span>
          Local budgets track observed usage. API-equivalent estimates are not billed charges, and
          budgets do not control or stop coding sessions.
        </span>
      </div>
      <SectionHeading eyebrow="YOUR GUARDRAILS" title="Local budgets" />
      <div className="budget-grid">
        {snapshot?.budgets.map((b) => (
          <section className="panel budget-card" key={b.id}>
            <div className="mini-heading">
              <span className="budget-icon">
                <Wallet size={19} />
              </span>
              <Badge tone={b.percentage >= b.threshold ? 'amber' : 'mint'}>
                {b.percentage >= 100
                  ? 'Over budget'
                  : b.percentage >= b.threshold
                    ? 'Threshold reached'
                    : budgetIncomplete(b)
                      ? 'Partial data'
                      : 'On track'}
              </Badge>
              <IconButton label="Edit budget" onClick={() => show(b)}>
                <Pencil size={13} />
              </IconButton>
            </div>
            <h3>{b.name}</h3>
            <p className="muted small-text">
              {b.period === 'window'
                ? 'Rolling ' + (b.window_minutes || 300) + '-minute window'
                : b.period === '5h'
                  ? 'Rolling 5-hour window'
                  : b.period === 'day'
                    ? 'Daily budget'
                    : 'Monthly budget'}
              {b.project
                ? ' · ' + (snapshot.projects.find((p) => p.path === b.project)?.name || b.project)
                : ' · All projects'}
            </p>
            <div className="budget-values">
              <strong>
                {budgetIncomplete(b) ? '≥ ' : ''}
                {b.unit === 'usd' ? money(b.used) : compact(b.used)}
              </strong>
              <span>
                of {b.unit === 'usd' ? money(b.amount, 0) : compact(b.amount) + ' tokens'}
              </span>
              <b>
                {budgetIncomplete(b) ? '≥ ' : ''}
                {b.percentage.toFixed(0)}%
              </b>
            </div>
            <div
              className={
                'progress-track budget-track ' + (b.percentage >= b.threshold ? 'warning' : '')
              }
            >
              <i style={{ width: Math.min(100, b.percentage) + '%' }} />
              <span
                style={{ left: Math.min(100, b.threshold) + '%' }}
                title={'Alert threshold: ' + b.threshold + '%'}
              />
            </div>
            <div className="budget-notes">
              <span>
                <Bell size={11} />
                Alert at {b.threshold}%
              </span>
              <button
                className="text-button"
                onClick={() => run('budget_remove', { id: b.id }, 'Budget removed')}
              >
                <Trash2 size={11} />
                Remove
              </button>
            </div>
            {budgetIncomplete(b) && (
              <p className="warning-note">
                {b.unit === 'usd' && b.unpriced_tokens
                  ? compact(b.unpriced_tokens) + ' tokens are unpriced. '
                  : ''}
                Usage is incomplete; the budget shows the known lower bound.
              </p>
            )}
            <p className="small-note">
              {b.projected_at
                ? 'Estimated threshold: ' +
                  date(b.projected_at) +
                  ' at ' +
                  time(b.projected_at) +
                  ' · based on ' +
                  b.sample_minutes +
                  ' recent minutes.'
                : 'Projection unavailable: incomplete, insufficient, or stale recent usage.'}
            </p>
          </section>
        ))}
      </div>
      {!snapshot?.budgets.length && (
        <Empty
          icon={Wallet}
          title="A little planning goes a long way"
          description="Create a token or estimated-cost budget for your day, month, project, or a rolling five-hour window."
          action={
            <button className="button" onClick={() => show()}>
              <Plus size={14} />
              Create your first budget
            </button>
          }
        />
      )}
      <section className="panel">
        <SectionHeading eyebrow="FROM THE SOURCE" title="Provider-reported limits" />
        {snapshot?.limits.length ? (
          <div className="limits-grid">
            {snapshot.limits.map((l, i) => (
              <div key={i} className="limit-card">
                <div className="mini-heading">
                  <h3>{providerName(l.provider)}</h3>
                  <Badge>{l.scope}</Badge>
                </div>
                <strong>
                  {l.used_percent === null ? 'Unavailable' : l.used_percent.toFixed(1) + '%'}
                  <small> reported used</small>
                </strong>
                {l.used_percent !== null && (
                  <div className="progress-track">
                    <i style={{ width: Math.min(100, l.used_percent) + '%' }} />
                  </div>
                )}
                <dl className="detail-list">
                  <div>
                    <dt>Reported reset</dt>
                    <dd>
                      {l.resets_at ? date(l.resets_at) + ' ' + time(l.resets_at) : 'Unavailable'}
                    </dd>
                  </div>
                  <div>
                    <dt>Observed</dt>
                    <dd>{ago(l.observed_at)}</dd>
                  </div>
                  <div>
                    <dt>Window</dt>
                    <dd>
                      {l.window_minutes === null ? 'Unspecified' : elapsed(l.window_minutes * 60)}
                    </dd>
                  </div>
                </dl>
              </div>
            ))}
          </div>
        ) : (
          <div className="unavailable-inline">
            <ShieldCheck size={24} />
            <div>
              <h3>No reliable limit reports available</h3>
              <p>
                Provider percentages and reset times appear only when source records report them.
                Local logs do not establish complete account-wide usage.
              </p>
            </div>
          </div>
        )}
      </section>
      <section className="panel">
        <SectionHeading eyebrow="LOCAL ALERT HISTORY" title="Recent notifications" />
        {snapshot?.alerts.length ? (
          <div className="alert-list">
            {snapshot.alerts.map((a) => (
              <div key={a.id}>
                <span className="alert-icon">
                  <Bell size={15} />
                </span>
                <div>
                  <strong>{a.message}</strong>
                  <small>
                    {date(a.at)} · {time(a.at)}
                  </small>
                </div>
                <Badge>Budget</Badge>
              </div>
            ))}
          </div>
        ) : (
          <p className="small-note">
            No budget thresholds have been reached. Alerts will appear here and remain in your local
            history.
          </p>
        )}
      </section>
      <Modal
        open={open}
        onClose={() => setOpen(false)}
        title={edit ? 'Edit budget' : 'Create a local budget'}
        description="An informational guardrail for the usage visible on this device."
      >
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            const ok = await run(
              'budget_save',
              {
                id: edit?.id,
                name,
                amount: Number(amount),
                unit,
                period,
                window_minutes: period === 'window' ? Number(windowMinutes) : undefined,
                project: project || undefined,
                threshold: Number(threshold),
              },
              'Budget saved',
            );
            if (ok) setOpen(false);
          }}
        >
          <div className="dialog-body">
            <Field label="Budget name">
              <input
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Monthly coding budget"
              />
            </Field>
            <div className="form-grid">
              <Field label="Budget amount">
                <input
                  required
                  type="number"
                  min={unit === 'usd' ? '0.01' : '1'}
                  step={unit === 'usd' ? '0.01' : '1'}
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                />
              </Field>
              <Field label="Unit">
                <select value={unit} onChange={(e) => setUnit(e.target.value as typeof unit)}>
                  <option value="usd">USD · API equivalent</option>
                  <option value="tokens">Observed tokens</option>
                </select>
              </Field>
              <Field label="Period">
                <select value={period} onChange={(e) => setPeriod(e.target.value as typeof period)}>
                  <option value="day">Calendar day</option>
                  <option value="month">Calendar month</option>
                  <option value="5h">Rolling 5 hours</option>
                  <option value="window">Custom rolling window</option>
                </select>
              </Field>
              {period === 'window' && (
                <Field label="Window length (minutes)">
                  <input
                    type="number"
                    min="1"
                    max="525600"
                    required
                    value={windowMinutes}
                    onChange={(e) => setWindowMinutes(e.target.value)}
                  />
                </Field>
              )}
              <Field label="Alert threshold (%)">
                <input
                  required
                  type="number"
                  min="1"
                  max="100"
                  value={threshold}
                  onChange={(e) => setThreshold(e.target.value)}
                />
              </Field>
            </div>
            <Field label="Project">
              <select value={project} onChange={(e) => setProject(e.target.value)}>
                <option value="">All projects</option>
                {snapshot?.projects.map((p) => (
                  <option value={p.path} key={p.path}>
                    {p.name}
                  </option>
                ))}
              </select>
            </Field>
          </div>
          <div className="dialog-footer">
            <button className="button" type="button" onClick={() => setOpen(false)}>
              Cancel
            </button>
            <button className="button primary" type="submit">
              Save budget
            </button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function Sources({
  snapshot,
  run,
  busy,
  onEdit,
  onDemo,
  demo,
  paused,
  onPause,
}: {
  snapshot: Snapshot | null;
  run: Run;
  busy: boolean;
  onEdit: (source?: Source) => void;
  onDemo: () => void;
  demo: boolean;
  paused: boolean;
  onPause: () => void;
}) {
  const [rebuild, setRebuild] = useState(false);
  const [backup, setBackup] = useState('');
  return (
    <div className="standard-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">YOUR DATA, ON YOUR DEVICE</div>
          <h1>Sources</h1>
          <p>Connect your coding tools. Keep everything local.</p>
        </div>
        <button className="button primary" onClick={() => onEdit()}>
          <Plus size={15} />
          Add source
        </button>
      </div>
      <div className="source-summary">
        <span className="source-summary-icon">
          <Database size={22} />
        </span>
        <div>
          <h3>
            {snapshot?.sources.filter(
              (s) => s.enabled && s.status !== 'unavailable' && s.status !== 'missing',
            ).length || 0}{' '}
            connected roots
          </h3>
          <p>
            {n(snapshot?.sources.reduce((a, s) => a + s.files, 0) || 0)} files discovered ·{' '}
            {n(snapshot?.sources.reduce((a, s) => a + s.recognized, 0) || 0)} recognized records
          </p>
        </div>
        <Badge tone={paused ? 'amber' : 'mint'}>
          {paused ? 'Monitoring paused' : busy ? 'Reconciling…' : 'Monitoring locally'}
        </Badge>
        <button className="button small" onClick={onPause}>
          {paused ? <Play size={13} /> : <Pause size={13} />} {paused ? 'Resume' : 'Pause'}
        </button>
        <button
          className="button small"
          disabled={busy}
          onClick={() => run('rescan', {}, 'Source scan complete')}
        >
          <RefreshCw size={13} className={busy ? 'spin' : ''} />
          {busy ? 'Scanning…' : 'Rescan'}
        </button>
      </div>
      <div className="source-list">
        {snapshot?.sources.map((s) => (
          <section className={'panel source-card ' + (!s.enabled ? 'disabled' : '')} key={s.id}>
            <div className="source-card-heading">
              <span className={'source-provider ' + s.provider}>
                <ProviderIcon provider={s.provider} />
              </span>
              <div>
                <h3>
                  {s.label}
                  <Badge>{providerName(s.provider)}</Badge>
                </h3>
                <span className="mono source-path">{s.path}</span>
              </div>
              <Badge
                tone={
                  s.status === 'healthy' || s.status === 'ready'
                    ? 'mint'
                    : s.status === 'error' || s.status === 'missing'
                      ? 'amber'
                      : 'neutral'
                }
              >
                {s.enabled ? s.status : 'Disabled'}
              </Badge>
              <button
                className={'toggle ' + (s.enabled ? 'on' : '')}
                role="switch"
                aria-label={'Enable ' + s.label}
                aria-checked={s.enabled}
                onClick={() => run('source_save', { ...s, enabled: !s.enabled })}
              >
                <span />
              </button>
            </div>
            <div className="source-stats">
              {[
                ['Files', s.files],
                ['Recognized', s.recognized],
                ['Ignored', s.ignored],
                ['Warnings', s.warnings],
              ].map(([l, v]) => (
                <div key={l}>
                  <strong>{n(Number(v))}</strong>
                  <span>{l}</span>
                </div>
              ))}
              <div>
                <strong>{ago(s.last_read)}</strong>
                <span>Last successful read</span>
              </div>
              <div>
                <strong>{ago(s.last_activity)}</strong>
                <span>Last activity</span>
              </div>
            </div>
            {s.message && (
              <div className="source-message">
                <CircleAlert size={13} />
                {s.message}
              </div>
            )}
            {s.recovery_backup && (
              <p className="small-note">
                Source reselected; indexed history and notes retained. Preserved index:{' '}
                <code>{s.recovery_backup}</code>
              </p>
            )}
            {!!s.diagnostics?.length && (
              <details className="source-diagnostics">
                <summary>File and line diagnostics</summary>
                <ul>
                  {s.diagnostics.map((d, i) => (
                    <li key={i}>
                      <code>
                        {d.path}
                        {d.line > 0 ? ':' + d.line : ''}
                      </code>
                      <p>{d.message}</p>
                    </li>
                  ))}
                </ul>
              </details>
            )}
            {s.exclusions.length > 0 && (
              <p className="small-note">Excluded: {s.exclusions.join(', ')}</p>
            )}
            <div className="source-card-footer">
              <span>
                <ShieldCheck size={12} />
                Read-only source
              </span>
              <button className="text-button" onClick={() => onEdit(s)}>
                <Pencil size={12} />
                Configure
              </button>
              <button
                className="text-button"
                onClick={() =>
                  run('source_remove', { id: s.id }, 'Source removed. Original logs are unchanged.')
                }
              >
                <Trash2 size={12} />
                Remove
              </button>
            </div>
          </section>
        ))}
      </div>
      {!snapshot?.sources.length && (
        <Empty
          icon={FolderOpen}
          title="Connect your first source"
          description={
            isAppStore
              ? 'Choose a Claude Code or Codex log folder to grant read-only access, or explore the demo workspace below. No coding tools are needed for the demo.'
              : 'Chip Count looks for Claude Code and Codex logs in their standard locations. Add a custom directory or import a JSONL file to get started.'
          }
          action={
            <button className="button primary" onClick={() => onEdit()}>
              <Plus size={14} />
              Add a source
            </button>
          }
        />
      )}
      <div className="two-column-grid">
        <section className="panel source-info-card">
          <ShieldCheck size={21} />
          <h3>Private by design</h3>
          <p>
            Only usage metadata is indexed. Source files are read-only. Your prompts, code, and tool
            arguments are never collected into the index.
          </p>
          <div className="source-info-footer">
            <Badge>Offline ready</Badge>
            <Badge>No API keys</Badge>
          </div>
        </section>
        <section className="panel source-info-card">
          <Sparkles size={21} />
          <h3>A workspace to explore</h3>
          <p>
            Try a separate demo workspace with varied sessions, projects, and model usage. Your real
            source configuration stays independent.
          </p>
          <button className="button small" onClick={onDemo}>
            {demo ? 'Return to my data' : 'Explore demo workspace'}
            <ArrowRight size={13} />
          </button>
        </section>
      </div>
      <section className="panel index-maintenance">
        <div>
          <h3>Rebuild local index</h3>
          <p>
            Re-read configured sources and recalculate derived usage. Your annotations, settings,
            and original logs are preserved.
          </p>
        </div>
        <button className="button" onClick={() => setRebuild(true)}>
          <RefreshCw size={13} />
          Rebuild index
        </button>
      </section>
      {backup && (
        <p className="small-note" role="status">
          Preserved index: {backup}
        </p>
      )}
      <Modal
        open={rebuild}
        onClose={() => setRebuild(false)}
        title="Rebuild the local index?"
        description="Chip Count will re-read all configured roots. Source logs are never modified."
      >
        <div className="dialog-body">
          <p className="small-note">
            Use this to recover a damaged derived index or apply parser changes. Local notes,
            labels, sources, prices, budgets, preferences, and existing observations are kept. A
            SQLite backup is saved before the rebuild; it stops if a source is unavailable. Large
            histories may take a moment.
          </p>
        </div>
        <div className="dialog-footer">
          <button className="button" onClick={() => setRebuild(false)}>
            Cancel
          </button>
          <button
            className="button primary"
            disabled={busy}
            onClick={async () => {
              const ok = await run('rescan', { rebuild: true }, 'Local index rebuilt');
              if (ok) {
                setRebuild(false);
                if (ok.backup) setBackup(ok.backup);
              }
            }}
          >
            Rebuild index
          </button>
        </div>
      </Modal>
    </div>
  );
}
function SourceDialog({
  open,
  source,
  demo,
  onClose,
  run,
  onToast,
}: {
  open: boolean;
  source?: Source;
  demo: boolean;
  onClose: () => void;
  run: Run;
  onToast: (s: string) => void;
}) {
  const [provider, setProvider] = useState<'claude' | 'codex'>('claude');
  const [label, setLabel] = useState('Personal');
  const [path, setPath] = useState('');
  const [selectionId, setSelectionId] = useState<string>();
  const [exclusions, setExclusions] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [saving, setSaving] = useState(false);
  const [selectionError, setSelectionError] = useState<UserFacingError | null>(null);
  useEffect(() => {
    if (open) {
      setProvider(source?.provider || 'claude');
      setLabel(source?.label || 'Personal');
      setPath(source?.path || '');
      setSelectionId(undefined);
      setSelectionError(null);
      setExclusions(source?.exclusions.join('\n') || '');
      setEnabled(source?.enabled ?? true);
    }
  }, [open, source]);
  const browse = async (directory: boolean) => {
    setSelectionError(null);
    const opener = document.activeElement as HTMLElement | null;
    try {
      const p = await pickPath(directory);
      if (p) {
        setPath(p.path);
        setSelectionId(p.selection_id);
      } else if (!isDesktop)
        onToast(
          'Enter an absolute local path below. Native file selection is available in the desktop app.',
        );
    } catch (e) {
      setSelectionError(userError(e));
    } finally {
      opener?.focus();
    }
  };
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={source ? 'Configure source' : 'Add a source'}
      description="Connect a local directory or JSONL usage file."
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setSaving(true);
          const ok = await run(
            'source_save',
            {
              id: source?.id,
              selection_id: selectionId,
              provider,
              label: label.trim(),
              path: path.trim(),
              enabled,
              exclusions: exclusions
                .split('\n')
                .map((s) => s.trim())
                .filter(Boolean),
            },
            'Source saved',
          );
          setSaving(false);
          if (ok) onClose();
        }}
      >
        <div className="dialog-body">
          {selectionError && (
            <ErrorNotice error={selectionError} clear={() => setSelectionError(null)} />
          )}
          <div className="provider-choices">
            {(['claude', 'codex'] as const).map((p) => (
              <button
                type="button"
                key={p}
                className={provider === p ? 'selected' : ''}
                onClick={() => setProvider(p)}
              >
                <ProviderIcon provider={p} />
                {providerName(p)}
                {provider === p && <Check size={13} />}
              </button>
            ))}
          </div>
          <Field
            label="Source / profile label"
            hint="An organizational label, not an account identity or quota."
          >
            <input
              required
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="Personal, Work, or a custom label"
            />
          </Field>
          <Field
            label={isAppStore && !demo ? 'Selected source' : 'Absolute local path'}
            hint={
              isAppStore && !demo
                ? 'Choose folder or Import JSONL to grant access. For hidden folders press ⇧⌘G in the picker and enter ~/.claude/projects, ~/.codex/sessions, or ~/.codex/archived_sessions.'
                : undefined
            }
          >
            <input
              required
              value={path}
              readOnly={isAppStore && !demo}
              onChange={(e) => {
                setPath(e.target.value);
                setSelectionId(undefined);
              }}
              placeholder={
                provider === 'claude' ? '/Users/you/.claude/projects' : '/Users/you/.codex/sessions'
              }
            />
          </Field>
          <div className="form-inline-actions">
            <button type="button" className="button small" onClick={() => browse(true)}>
              <FolderOpen size={13} />
              Choose folder
            </button>
            <button type="button" className="button small" onClick={() => browse(false)}>
              <FileJson size={13} />
              Import JSONL
            </button>
          </div>
          <Field
            label="Exclusion patterns"
            hint="One pattern per line. Applied within this source root."
          >
            <textarea
              rows={3}
              value={exclusions}
              onChange={(e) => setExclusions(e.target.value)}
              placeholder="**/node_modules/**\n**/tmp/**"
            />
          </Field>
          <Toggle
            label="Enable source"
            description="Read complete usage records from this root."
            value={enabled}
            onChange={setEnabled}
          />
        </div>
        <div className="dialog-footer">
          <button className="button" type="button" onClick={onClose}>
            Cancel
          </button>
          <button className="button primary" type="submit" disabled={saving}>
            {saving ? 'Connecting…' : source ? 'Save source' : 'Connect source'}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function SettingsPage({
  snapshot,
  run,
  onSources,
  onDemo,
  demo,
  onExport,
}: {
  snapshot: Snapshot | null;
  run: Run;
  onSources: () => void;
  onDemo: () => void;
  demo: boolean;
  onExport: () => void;
}) {
  const [tab, setTab] = useStored('settings-tab', 'General');
  const [pricing, setPricing] = useState<Price | null>(null);
  const [refreshingPrices, setRefreshingPrices] = useState(false);
  const [rates, setRates] = useState({
    input: '0',
    output: '0',
    cache_read: '0',
    cache_write: '0',
  });
  const [repriceOpen, setRepriceOpen] = useState(false);
  const [timezone, setTimezone] = useState('');
  const [subscription, setSubscription] = useState('');
  const [inactivity, setInactivity] = useState('5');
  const [retention, setRetention] = useState('365');
  useEffect(() => {
    if (snapshot) {
      setTimezone(snapshot.settings.timezone);
      setSubscription(
        snapshot.settings.monthly_subscription === null
          ? ''
          : String(snapshot.settings.monthly_subscription),
      );
      setInactivity(String(snapshot.settings.inactivity_minutes));
      setRetention(String(snapshot.settings.retention_days));
    }
  }, [
    snapshot?.settings.timezone,
    snapshot?.settings.monthly_subscription,
    snapshot?.settings.inactivity_minutes,
    snapshot?.settings.retention_days,
  ]);
  const save = async (settings: Partial<Settings>) => {
    await run('settings_save', { settings }, 'Preference saved');
  };
  const openPrice = (price: Price) => {
    setPricing(price);
    setRates({
      input: String(price.input),
      output: String(price.output),
      cache_read: String(price.cache_read),
      cache_write: String(price.cache_write),
    });
  };
  if (!snapshot)
    return (
      <Empty
        icon={Settings2}
        title="Loading preferences"
        description="Your preferences are stored on this device."
      />
    );
  const settings = snapshot.settings;
  return (
    <div className="standard-page settings-page">
      <div className="page-heading">
        <div>
          <div className="heading-kicker">MAKE YOURSELF AT HOME</div>
          <h1>Settings</h1>
          <p>A workspace that works the way you do.</p>
        </div>
      </div>
      <div className="settings-tabs">
        {['General', 'Appearance', 'Pricing', 'Data & exports'].map((t) => (
          <button key={t} className={t === tab ? 'active' : ''} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </div>
      {tab === 'General' && (
        <>
          <section className="panel">
            <SectionHeading title="Activity & time" />
            <form
              className="settings-form"
              onSubmit={(e) => {
                e.preventDefault();
                void save({ timezone, inactivity_minutes: Number(inactivity) });
              }}
            >
              <Field
                label="Reporting timezone"
                hint="Used for daily boundaries and displayed timestamps."
              >
                <input
                  value={timezone}
                  onChange={(e) => setTimezone(e.target.value)}
                  list="timezones"
                />
                <datalist id="timezones">
                  {[
                    'UTC',
                    'America/Chicago',
                    'America/New_York',
                    'America/Los_Angeles',
                    'Europe/London',
                    'Europe/Paris',
                    'Asia/Tokyo',
                    'Asia/Kolkata',
                    'Australia/Sydney',
                  ].map((t) => (
                    <option key={t} value={t} />
                  ))}
                </datalist>
              </Field>
              <Field
                label="Inactivity threshold (minutes)"
                hint="Sessions with no recent usage become idle. This is inferred activity, not explicit completion."
              >
                <input
                  type="number"
                  required
                  min="1"
                  max="1440"
                  value={inactivity}
                  onChange={(e) => setInactivity(e.target.value)}
                />
              </Field>
              <button className="button small" type="submit">
                Save activity preferences
              </button>
            </form>
          </section>
          <section className="panel">
            <SectionHeading title="Desktop behavior" />
            {!isDesktop && (
              <p className="small-note">
                These system integrations are available in the installed desktop app.
              </p>
            )}
            <Toggle
              label="Launch at login"
              description={
                isAppStore
                  ? 'Unavailable in the App Store 1.0 build.'
                  : 'Keep local usage monitoring ready when you sign in.'
              }
              value={isAppStore ? false : settings.launch_at_login}
              onChange={(v) => save({ launch_at_login: v })}
              disabled={!isDesktop || isAppStore}
            />
            <Toggle
              label="Close to menu bar"
              description="Keep indexing when the main window closes."
              value={settings.close_to_tray}
              onChange={(v) => save({ close_to_tray: v })}
              disabled={!isDesktop}
            />
            <Toggle
              label="Budget notifications"
              description="Show desktop notifications for newly crossed thresholds."
              value={settings.notifications}
              onChange={(v) => save({ notifications: v })}
              disabled={!isDesktop}
            />
          </section>
          <section className="panel settings-about">
            <img src="/chip.svg" alt="" />
            <div>
              <h3>
                Chip Count <Badge>0.1.1</Badge>
              </h3>
              <p>Know where every token went.</p>
              <small>A local-first tool from Rippley Labs.</small>
            </div>
            <ShieldCheck size={23} />
          </section>
        </>
      )}
      {tab === 'Appearance' && (
        <>
          <section className="panel">
            <SectionHeading title="Theme" />
            <p className="small-note">Calm, readable, and comfortable throughout the day.</p>
            <div className="theme-options">
              {[
                { value: 'dark', label: 'Graphite', icon: Moon },
                { value: 'light', label: 'Warm ivory', icon: Sun },
                { value: 'system', label: 'System', icon: Monitor },
              ].map(({ value, label, icon: Icon }) => (
                <button
                  key={value}
                  className={
                    'theme-option ' + value + ' ' + (settings.theme === value ? 'selected' : '')
                  }
                  onClick={() => save({ theme: value as Settings['theme'] })}
                >
                  <div className="theme-preview">
                    <i />
                    <span />
                    <b />
                    <em />
                  </div>
                  <span>
                    <Icon size={14} />
                    {label}
                    {settings.theme === value && <Check size={14} />}
                  </span>
                </button>
              ))}
            </div>
          </section>
          <section className="panel">
            <SectionHeading title="Information density" />
            <div className="density-options">
              <button
                className={settings.density === 'comfortable' ? 'selected' : ''}
                onClick={() => save({ density: 'comfortable' })}
              >
                <LayoutGrid size={20} />
                <div>
                  <strong>Comfortable</strong>
                  <span>A little more room to breathe.</span>
                </div>
                {settings.density === 'comfortable' && <Check size={16} />}
              </button>
              <button
                className={settings.density === 'compact' ? 'selected' : ''}
                onClick={() => save({ density: 'compact' })}
              >
                <ListFilter size={20} />
                <div>
                  <strong>Compact</strong>
                  <span>More of your workspace in view.</span>
                </div>
                {settings.density === 'compact' && <Check size={16} />}
              </button>
            </div>
          </section>
        </>
      )}
      {tab === 'Pricing' && (
        <>
          <div className="info-box">
            <CircleHelp size={16} />
            <span>
              API-equivalent cost is an estimate using versioned model rates. It is separate from
              subscription fees, invoices, and provider quotas. Unknown model prices remain
              unpriced.
            </span>
          </div>
          <section className="panel">
            <SectionHeading title="Automatic price refresh" />
            <Toggle
              label="Refresh API prices daily"
              description="Fetch the public Models.dev catalog while Chip Count is running. Catch up on launch; retry failed refreshes after an hour."
              value={settings.pricing_auto_refresh}
              onChange={(value) => save({ pricing_auto_refresh: value })}
              disabled={demo}
            />
            <p className="small-note">
              {snapshot.pricing_refresh?.last_success
                ? `Last refreshed ${date(snapshot.pricing_refresh.last_success)} · ${snapshot.pricing_refresh.updated_models} models changed.`
                : 'Using saved prices. No successful refresh yet.'}
              {demo && ' Live price refresh is unavailable in demo mode.'}
            </p>
            {snapshot.pricing_refresh?.error && (
              <p className="small-note" role="status">
                Refresh failed. Saved prices remain available. {snapshot.pricing_refresh.error}
              </p>
            )}
            <button
              className="button small"
              disabled={demo || refreshingPrices}
              onClick={async () => {
                setRefreshingPrices(true);
                try {
                  await run('pricing_refresh', {}, 'API prices refreshed');
                } finally {
                  setRefreshingPrices(false);
                }
              }}
            >
              <RefreshCw size={13} />
              {refreshingPrices ? 'Refreshing prices…' : 'Refresh prices now'}
            </button>
            <p className="small-note">
              Only the public price catalog is downloaded. Local overrides and historical estimates
              keep their recorded rates.
            </p>
          </section>
          <section className="panel">
            <SectionHeading title="Model pricing">
              <Badge>USD per million tokens</Badge>
            </SectionHeading>
            <div className="table-overflow">
              <table className="data-table pricing-table">
                <thead>
                  <tr>
                    <th>Model</th>
                    <th>Input</th>
                    <th>Output</th>
                    <th>Cache read</th>
                    <th>Cache write</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {snapshot.prices.map((p) => (
                    <tr key={p.model}>
                      <td>
                        <strong>{p.model}</strong>
                        <small>
                          {p.override
                            ? 'Local override'
                            : p.inferred
                              ? 'Auto-review estimate · model inferred'
                              : p.version.startsWith('models-dev-')
                                ? 'Fetched catalog'
                                : 'Bundled snapshot'}{' '}
                          · {p.version} · {date(p.retrieved_at)}
                        </small>
                        <span className="price-source">{p.source}</span>
                      </td>
                      <td>{money(p.input)}</td>
                      <td>{money(p.output)}</td>
                      <td>{money(p.cache_read)}</td>
                      <td>{money(p.cache_write)}</td>
                      <td>
                        <IconButton
                          label={'Override ' + p.model + ' pricing'}
                          onClick={() => openPrice(p)}
                        >
                          <Pencil size={13} />
                        </IconButton>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="pricing-actions">
              <button
                className="button small"
                onClick={() =>
                  openPrice({
                    model: '',
                    input: 0,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                    version: 'local',
                    source: 'Local override',
                    retrieved_at: new Date().toISOString(),
                    override: true,
                  })
                }
              >
                <Plus size={13} />
                Add model override
              </button>
              <button className="button small" onClick={() => setRepriceOpen(true)}>
                <RefreshCw size={13} />
                Recalculate historical costs
              </button>
            </div>
            <p className="small-note">
              Overrides apply to new observations. Historical estimates keep their original pricing
              version unless you explicitly recalculate.
            </p>
          </section>
          <section className="panel">
            <SectionHeading title="Subscription context" />
            <form
              className="settings-form"
              onSubmit={(e) => {
                e.preventDefault();
                void save({ monthly_subscription: subscription ? Number(subscription) : null });
              }}
            >
              <Field
                label="Monthly subscription amount (USD)"
                hint="An optional point of reference, not a claim of billed savings or available quota."
              >
                <input
                  type="number"
                  min="0"
                  step=".01"
                  placeholder="Optional"
                  value={subscription}
                  onChange={(e) => setSubscription(e.target.value)}
                />
              </Field>
              <button type="submit" className="button small">
                Save amount
              </button>
            </form>
            {settings.monthly_subscription !== null && (
              <p className="small-note">
                Subscription: {money(settings.monthly_subscription)} / month. Current filtered
                API-equivalent value: {usageCost(snapshot.totals)}.
              </p>
            )}
          </section>
        </>
      )}
      {tab === 'Data & exports' && (
        <>
          <section className="panel">
            <SectionHeading title="Your local data" />
            <div className="setting-row">
              <div>
                <strong>Configured sources</strong>
                <p>{snapshot.sources.length} roots · Claude Code and Codex</p>
              </div>
              <button className="button small" onClick={onSources}>
                Manage sources <ArrowRight size={12} />
              </button>
            </div>
            <Toggle
              label="Demo workspace"
              description="A separate local database of deterministic sample sessions."
              value={demo}
              onChange={onDemo}
            />
            <form
              className="settings-form retention-form"
              onSubmit={(e) => {
                e.preventDefault();
                void save({ retention_days: Number(retention) });
              }}
            >
              <Field
                label="Metadata retention (days)"
                hint="Applied to indexed usage metadata; original source logs are preserved."
              >
                <input
                  type="number"
                  min="1"
                  max="36500"
                  required
                  value={retention}
                  onChange={(e) => setRetention(e.target.value)}
                />
              </Field>
              <button type="submit" className="button small">
                Save retention
              </button>
            </form>
          </section>
          <section className="panel">
            <SectionHeading title="Export defaults" />
            <Toggle
              label="Redact local paths"
              description="Hide your local directory structure in CSV and JSON exports."
              value={settings.redact_paths}
              onChange={(v) => save({ redact_paths: v })}
            />
            <Toggle
              label="Redact custom labels"
              description="Hide your personal profile, project, and session aliases."
              value={settings.redact_labels}
              onChange={(v) => save({ redact_labels: v })}
            />
            <button className="button small" onClick={onExport}>
              <Download size={13} />
              Export current view
            </button>
          </section>
          <section className="panel">
            <SectionHeading title="Keyboard shortcuts" />
            <div className="shortcut-list">
              {[
                ['Settings', '⌘ / Ctrl ,'],
                ['Select sessions', '↑ / ↓ · Home / End · Page Up / Down'],
                ['Quick actions', '⌘ / Ctrl K'],
                ['Export current view', '⌘ / Ctrl E'],
                ['Navigate pages', '⌘ / Ctrl 1–8'],
                ['Close a dialog', 'Esc'],
                ['Resize inspector', '← / → when focused'],
              ].map(([l, k]) => (
                <div key={l}>
                  <span>{l}</span>
                  <kbd>{k}</kbd>
                </div>
              ))}
            </div>
          </section>
        </>
      )}
      <Modal
        open={!!pricing}
        onClose={() => setPricing(null)}
        title="Local pricing override"
        description="USD per one million tokens. Source model identifiers are preserved."
      >
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!pricing) return;
            const ok = await run(
              'pricing_save',
              {
                model: pricing.model,
                input: Number(rates.input),
                output: Number(rates.output),
                cache_read: Number(rates.cache_read),
                cache_write: Number(rates.cache_write),
              },
              'Pricing override saved for new observations',
            );
            if (ok) setPricing(null);
          }}
        >
          <div className="dialog-body">
            <Field label="Exact model identifier">
              <input
                required
                value={pricing?.model || ''}
                onChange={(e) => setPricing(pricing ? { ...pricing, model: e.target.value } : null)}
              />
            </Field>
            <div className="form-grid">
              {(['input', 'output', 'cache_read', 'cache_write'] as const).map((key) => (
                <Field key={key} label={key.replace('_', ' ') + ' · USD / 1M'}>
                  <input
                    required
                    type="number"
                    min="0"
                    step="any"
                    value={rates[key]}
                    onChange={(e) => setRates({ ...rates, [key]: e.target.value })}
                  />
                </Field>
              ))}
            </div>
          </div>
          <div className="dialog-footer">
            <button type="button" className="button" onClick={() => setPricing(null)}>
              Cancel
            </button>
            <button type="submit" className="button primary">
              Save override
            </button>
          </div>
        </form>
      </Modal>
      <Modal
        open={repriceOpen}
        onClose={() => setRepriceOpen(false)}
        title="Recalculate historical costs?"
        description="Apply your current pricing snapshot and local overrides to all indexed events."
      >
        <div className="dialog-body">
          <p className="small-note">
            This explicitly changes historical API-equivalent estimates and recorded pricing
            versions. Token counts and source files remain unchanged. Export a report first if you
            need to retain the previous estimates.
          </p>
        </div>
        <div className="dialog-footer">
          <button className="button" onClick={() => setRepriceOpen(false)}>
            Cancel
          </button>
          <button
            className="button primary"
            onClick={async () => {
              const ok = await run('reprice', {}, 'Historical estimates recalculated');
              if (ok) setRepriceOpen(false);
            }}
          >
            Recalculate estimates
          </button>
        </div>
      </Modal>
    </div>
  );
}
