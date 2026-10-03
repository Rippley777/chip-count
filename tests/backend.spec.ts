import { test, expect, type APIRequestContext } from '@playwright/test';

async function command(request: APIRequestContext, command: string, args: unknown = {}) {
  const response = await request.post('/api/dispatch', { data: { command, args, demo: true } });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}
test('Rust demo data stays separate and every summary uses canonical own usage', async ({
  request,
}) => {
  const data = await command(request, 'snapshot');
  expect(data.demo).toBe(true);
  expect(data.total_sessions).toBeGreaterThan(8);
  expect(data.sessions.length).toBeGreaterThan(8);
  expect(data.sources.length).toBeGreaterThan(0);
  const sum = data.sessions.reduce((n: number, s: any) => n + s.usage.total, 0);
  expect(data.totals.total).toBe(sum);
  for (const session of data.sessions) {
    expect(session.usage.total).toBe(
      session.usage.input +
        session.usage.output +
        session.usage.cache_read +
        session.usage.cache_write,
    );
    expect(session.combined_usage.total).toBeGreaterThanOrEqual(session.usage.total);
  }
  const real = await request.post('/api/dispatch', {
    data: { command: 'snapshot', args: { filter: { limit: 5 } }, demo: false },
  });
  expect(real.ok()).toBe(true);
  expect((await real.json()).demo).toBe(false);
});
test('combined filters, detail and export retain one accounting scope', async ({ request }) => {
  const full = await command(request, 'snapshot');
  const session = full.sessions.find((s: any) => s.usage.total > 0);
  const filter = { provider: session.provider, project: session.project_path };
  const scoped = await command(request, 'snapshot', { filter });
  expect(scoped.sessions.length).toBeGreaterThan(0);
  expect(
    scoped.sessions.every(
      (s: any) => s.provider === session.provider && s.project_path === session.project_path,
    ),
  ).toBe(true);
  const detail = await command(request, 'session', { id: session.id });
  expect(detail.session.id).toBe(session.id);
  expect(detail.events.length).toBeGreaterThan(0);
  expect(detail.events.every((e: any) => !('content' in e) && !('prompt' in e))).toBe(true);
  const result = await command(request, 'export', {
    format: 'json',
    filter,
    redact_paths: true,
    redact_labels: true,
  });
  const parsed = JSON.parse(result.content);
  expect(parsed).toBeTruthy();
  expect(result.content).not.toContain(session.project_path);
  expect(result.mime).toContain('json');
});
test('annotations, budgets, and prices validate and persist', async ({ request }) => {
  const before = await command(request, 'snapshot');
  const id = before.sessions[0].id;
  await command(request, 'annotate', {
    id,
    alias: 'Verified session',
    notes: 'Metadata only',
    tags: ['verified'],
    pinned: true,
  });
  const detail = await command(request, 'session', { id });
  expect(detail.session.name).toBe('Verified session');
  expect(detail.session.notes).toBe('Metadata only');
  expect(detail.session.tags).toContain('verified');
  expect(detail.session.pinned).toBe(true);
  await command(request, 'budget_save', {
    name: 'Verification budget',
    amount: 100,
    unit: 'usd',
    period: 'month',
    threshold: 80,
  });
  const after = await command(request, 'snapshot');
  const budget = after.budgets.find((b: any) => b.name === 'Verification budget');
  expect(budget.amount).toBe(100);
  await command(request, 'budget_remove', { id: budget.id });
  const invalid = await request.post('/api/dispatch', {
    data: {
      command: 'pricing_save',
      args: { model: 'bad', input: -1, output: 2, cache_read: 0, cache_write: 0 },
      demo: true,
    },
  });
  expect(invalid.ok()).toBe(false);
});

test('filesystem updates reconcile complete lines, and monitoring pause/resume works', async ({
  request,
}) => {
  const { mkdtemp, writeFile, appendFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const folder = await mkdtemp(join(tmpdir(), 'chip-count-live-test-'));
  const path = join(folder, 'session.jsonl');
  const record = (id: string) =>
    JSON.stringify({
      type: 'assistant',
      timestamp: new Date().toISOString(),
      sessionId: 'watch-' + folder,
      cwd: folder,
      requestId: id,
      message: {
        id,
        model: 'claude-sonnet-4-5',
        usage: {
          input_tokens: 100,
          output_tokens: 10,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
        },
      },
    });
  const real = async (command: string, args: unknown = {}) => {
    const r = await request.post('/api/dispatch', { data: { command, args, demo: false } });
    expect(r.ok(), await r.text()).toBe(true);
    return r.json();
  };
  let sourceId: string | undefined;
  try {
    await writeFile(path, record('first') + '\n');
    await real('source_save', {
      provider: 'claude',
      label: 'Temporary integration fixture',
      path,
      enabled: true,
    });
    const first = await real('snapshot', { filter: { project: folder } });
    sourceId = first.sources.find((s: any) => s.label === 'Temporary integration fixture')?.id;
    expect(first.totals.total).toBe(110);
    await appendFile(path, record('second'));
    await new Promise((resolve) => setTimeout(resolve, 3500));
    expect((await real('snapshot', { filter: { project: folder } })).totals.total).toBe(110);
    await appendFile(path, '\n');
    await expect
      .poll(async () => (await real('snapshot', { filter: { project: folder } })).totals.total, {
        timeout: 12000,
      })
      .toBe(220);
    await request.post('/api/monitoring', { data: { paused: true } });
    await appendFile(path, record('third') + '\n');
    await new Promise((resolve) => setTimeout(resolve, 3500));
    expect((await real('snapshot', { filter: { project: folder } })).totals.total).toBe(220);
    await request.post('/api/monitoring', { data: { paused: false } });
    await expect
      .poll(async () => (await real('snapshot', { filter: { project: folder } })).totals.total, {
        timeout: 12000,
      })
      .toBe(330);
  } finally {
    await request.post('/api/monitoring', { data: { paused: false } });
    if (sourceId) await real('source_remove', { id: sourceId });
    await rm(folder, { recursive: true, force: true });
  }
});

test('auto-review costs display inferred model pricing in the dashboard and inspector', async ({
  page,
  request,
}) => {
  const { mkdtemp, writeFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const folder = await mkdtemp(join(tmpdir(), 'chip-count-review-test-'));
  const path = join(folder, 'review.jsonl');
  const timestamp = new Date().toISOString();
  const real = async (command: string, args: unknown = {}) => {
    const response = await request.post('/api/dispatch', { data: { command, args, demo: false } });
    expect(response.ok(), await response.text()).toBe(true);
    return response.json();
  };
  let sourceId: string | undefined;
  try {
    await writeFile(
      path,
      [
        { type: 'session_meta', timestamp, payload: { id: folder, cwd: folder } },
        { type: 'turn_context', timestamp, payload: { model: 'codex-auto-review' } },
        {
          type: 'event_msg',
          timestamp,
          payload: {
            type: 'token_count',
            info: {
              total_token_usage: {
                input_tokens: 100000,
                cached_input_tokens: 50000,
                output_tokens: 1000,
                reasoning_output_tokens: 500,
                total_tokens: 101000,
              },
            },
          },
        },
      ]
        .map((record) => JSON.stringify(record) + '\n')
        .join(''),
    );
    await real('source_save', {
      provider: 'codex',
      label: 'Temporary auto-review fixture',
      path,
      enabled: true,
    });
    const snapshot = await real('snapshot', { filter: { project: folder } });
    sourceId = snapshot.sources.find((s: any) => s.label === 'Temporary auto-review fixture')?.id;
    expect(snapshot.totals.inferred_price_tokens).toBe(101000);
    expect(snapshot.totals.unpriced_tokens).toBe(0);
    await page.addInitScript(
      ({ project, selected }) => {
        localStorage.setItem('chip-count:demo', 'false');
        localStorage.setItem('chip-count:page', '"Live"');
        localStorage.setItem('chip-count:filter', JSON.stringify({ project }));
        localStorage.setItem('chip-count:selected', JSON.stringify(selected));
        localStorage.setItem('chip-count:inspector-tab', '"Overview"');
      },
      { project: folder, selected: snapshot.sessions[0].id },
    );
    await page.goto('/');
    await expect(page.locator('.metrics-strip').first()).toContainText('≈$0.01');
    await expect(page.locator('.metrics-strip').first()).toContainText('auto-review estimates');
    await expect(page.locator('.inspector-stats')).toContainText('≈$0.01');
    await expect(page.locator('.inspector-stats')).toContainText('model inferred');
    await expect(page.locator('.session-row-main').first()).toContainText('≈$0.01');
  } finally {
    if (sourceId) await real('source_remove', { id: sourceId });
    await rm(folder, { recursive: true, force: true });
  }
});
