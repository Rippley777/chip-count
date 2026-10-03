import { test, expect, type APIRequestContext, type Page } from '@playwright/test';
import type { Commands, Detail } from '../src/types';

// These are end-to-end checks against the Rust demo database, never response mocks.
async function dispatch<K extends keyof Commands>(
  request: APIRequestContext,
  command: K,
  args: Commands[K]['args'],
): Promise<Commands[K]['result']> {
  const response = await request.post('/api/dispatch', { data: { command, args, demo: true } });
  expect(response.ok(), await response.text()).toBe(true);
  return response.json();
}

function isCommand(
  response: { url(): string; request(): { postDataJSON(): any } },
  command: string,
) {
  if (!response.url().endsWith('/api/dispatch')) return false;
  try {
    return response.request().postDataJSON()?.command === command;
  } catch {
    return false;
  }
}

async function openSession(page: Page, request: APIRequestContext): Promise<Detail> {
  const snapshot = await dispatch(request, 'snapshot', {});
  // Pick rich, stable history instead of depending on the currently active top row.
  const session = [...snapshot.sessions].sort((a, b) => b.usage.events - a.usage.events)[0];
  expect(session?.usage.events).toBeGreaterThan(2);
  const detail = await dispatch(request, 'session', { id: session.id });
  await page.addInitScript(
    (id) => localStorage.setItem('chip-count:selected', JSON.stringify(id)),
    session.id,
  );
  await page.goto('/');
  await expect(page.locator('.inspector-stats')).toBeVisible();
  return detail;
}

async function expectPaintedCurve(page: Page, selector: string) {
  const curve = page.locator(selector).first();
  await expect(curve).toBeVisible();
  await expect(curve).toHaveAttribute('d', /^M[-\d.]+,[-\d.]+[CL]/);
  const geometry = await curve.evaluate((element: SVGGraphicsElement) => {
    const box = element.getBBox();
    return {
      width: box.width,
      height: box.height,
      stroke: getComputedStyle(element).stroke,
      d: element.getAttribute('d'),
    };
  });
  expect(geometry.width).toBeGreaterThan(100);
  expect(geometry.height).toBeGreaterThan(1);
  expect(geometry.stroke).not.toBe('none');
  expect(geometry.d).not.toMatch(/NaN|Infinity/);
}

async function navigate(page: Page, label: string) {
  await page.locator('.sidebar').getByRole('button', { name: label, exact: true }).click();
  await expect(page.locator('.breadcrumbs strong')).toHaveText(label);
}

test.beforeEach(async ({ page, request }) => {
  page.setDefaultTimeout(10000);
  await request.post('/api/monitoring', { data: { paused: false } });
  await page.addInitScript(() => {
    localStorage.setItem('chip-count:demo', 'true');
    localStorage.setItem('chip-count:page', '"Live"');
    localStorage.setItem('chip-count:filter', '{}');
    localStorage.setItem('chip-count:selected', 'null');
    localStorage.setItem('chip-count:inspector-tab', '"Overview"');
    localStorage.setItem('chip-count:settings-tab', '"General"');
  });
});

test('usage events are searchable and expose canonical source provenance', async ({
  page,
  request,
}) => {
  const detail = await openSession(page, request);
  const event = detail.events[0];
  await page.getByRole('tab', { name: 'Events', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Usage events', exact: true })).toBeVisible();
  await page.getByRole('textbox', { name: 'Search usage events' }).fill(event.id);
  await expect(page.locator('.event-table tbody tr')).toHaveCount(1);
  await page.locator('.event-table tbody tr').click();
  const provenance = page.locator('.event-detail');
  await expect(provenance.getByRole('heading', { name: 'Event provenance' })).toBeVisible();
  await expect(provenance).toContainText(event.id);
  await expect(provenance).toContainText(event.model);
  await expect(provenance).toContainText(event.parser_version);
  await expect(provenance).toContainText(event.source_path + ':' + event.source_line);
  await expect(provenance).toContainText(
    'contributes ' + event.usage.total.toLocaleString('en-US') + ' tokens once',
  );
  await expect(page.locator('.inspector')).toContainText(
    'Prompt bodies and tool arguments are not indexed',
  );
  await page.getByRole('textbox', { name: 'Search usage events' }).fill('no-such-usage-event-abc');
  await expect(page.locator('.event-table tbody tr')).toHaveCount(0);
  await expect(page.getByText('No matching usage events.')).toBeVisible();
});

test('timeline interval selection and reset agree with authoritative Rust event totals', async ({
  page,
  request,
}) => {
  const detail = await openSession(page, request);
  const sorted = [...detail.events].sort((a, b) => a.timestamp.localeCompare(b.timestamp));
  const from = new Date(sorted[0].timestamp);
  from.setUTCSeconds(0, 0);
  const to = new Date(sorted[Math.floor(sorted.length / 2)].timestamp);
  to.setUTCSeconds(0, 0);
  const localFields = await page.evaluate(
    ({ start, end }) => {
      const asInput = (stamp: string) => {
        const value = new Date(stamp);
        const pad = (part: number) => String(part).padStart(2, '0');
        return (
          value.getFullYear() +
          '-' +
          pad(value.getMonth() + 1) +
          '-' +
          pad(value.getDate()) +
          'T' +
          pad(value.getHours()) +
          ':' +
          pad(value.getMinutes())
        );
      };
      return { from: asInput(start), to: asInput(end) };
    },
    { start: from.toISOString(), end: to.toISOString() },
  );
  const expected = await dispatch(request, 'session', {
    id: detail.session.id,
    filter: { from: from.toISOString(), to: to.toISOString() },
  });
  expect(expected.event_count).toBeLessThan(detail.event_count);
  expect(expected.event_count).toBeGreaterThan(0);
  await page.getByRole('tab', { name: 'Timeline', exact: true }).click();
  await expect(page.locator('.inspector .recharts-brush')).toBeVisible();
  await page.getByLabel('From (device local time)', { exact: true }).fill(localFields.from);
  await page.getByLabel('Through (device local time)', { exact: true }).fill(localFields.to);
  await page.getByRole('button', { name: 'Apply interval' }).click();
  await expect(page.locator('.interval-banner')).toContainText('Selected interval');
  await expect(page.locator('.interval-summary')).toContainText(
    expected.event_count.toLocaleString('en-US') + ' events',
  );
  await page.getByRole('tab', { name: 'Events', exact: true }).click();
  await expect(page.locator('.event-table tbody tr')).toHaveCount(expected.events.length);
  await page.getByRole('tab', { name: 'Timeline', exact: true }).click();
  await page.getByRole('button', { name: 'Reset interval', exact: true }).click();
  await expect(page.locator('.interval-banner')).toHaveCount(0);
  await expect(page.locator('.interval-summary')).toContainText(
    detail.event_count.toLocaleString('en-US') + ' events',
  );
  await expect(page.locator('.inspector .recharts-brush')).toBeVisible();
});

test('session aliases, notes and tags persist without changing observed usage', async ({
  page,
  request,
}) => {
  const original = await openSession(page, request);
  const label = 'Interaction test ' + Date.now();
  try {
    await page.locator('.inspector').getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByLabel('Session alias', { exact: true }).fill(label);
    await page
      .getByLabel('Notes', { exact: true })
      .fill('A local annotation exercised by the browser integration test.');
    await page.getByLabel('Tags', { exact: false }).fill('interaction-test, verified');
    await page.getByRole('button', { name: 'Save locally', exact: true }).click();
    await expect(page.locator('.inspector-header h2')).toHaveText(label);
    await expect(page.locator('.notes-text')).toContainText('A local annotation exercised');
    await expect(page.locator('.session-tags')).toContainText('interaction-test');
    const saved = await dispatch(request, 'session', { id: original.session.id });
    expect(saved.session.name).toBe(label);
    expect(saved.session.tags).toEqual(['interaction-test', 'verified']);
    expect(saved.session.usage).toEqual(original.session.usage);
    expect(saved.session.source_path).toBe(original.session.source_path);
    await page.reload();
    await expect(page.locator('.inspector-header h2')).toHaveText(label);
    await expect(page.locator('.notes-text')).toContainText('A local annotation exercised');
  } finally {
    await dispatch(request, 'annotate', {
      id: original.session.id,
      alias: original.session.name,
      notes: original.session.notes,
      tags: original.session.tags,
      pinned: original.session.pinned,
    });
  }
});

test('in-progress settings remain intact across background snapshot polling', async ({
  page,
  request,
}) => {
  const before = await dispatch(request, 'snapshot', {});
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await navigate(page, 'Settings');
  const timezone = page.getByLabel(/^Reporting timezone/);
  const inactivity = page.getByLabel(/^Inactivity threshold/);
  const draftZone = before.settings.timezone === 'Asia/Tokyo' ? 'Europe/Paris' : 'Asia/Tokyo';
  await timezone.fill(draftZone);
  await inactivity.fill('47');
  // Wait on actual successful polling responses, not a fixed delay or mocked timer.
  for (let i = 0; i < 2; i++) await page.waitForResponse((r) => isCommand(r, 'snapshot') && r.ok());
  await expect(timezone).toHaveValue(draftZone);
  await expect(inactivity).toHaveValue('47');
  const after = await dispatch(request, 'snapshot', {});
  expect(after.settings.timezone).toBe(before.settings.timezone);
  expect(after.settings.inactivity_minutes).toBe(before.settings.inactivity_minutes);
});

test('light appearance updates the actual app theme and persists in Rust settings', async ({
  page,
  request,
}) => {
  const before = await dispatch(request, 'snapshot', {});
  try {
    await page.goto('/');
    await expect(page.locator('.session-row-main').first()).toBeVisible();
    await navigate(page, 'Settings');
    await page.getByRole('button', { name: 'Appearance', exact: true }).click();
    await page.getByRole('button', { name: 'Warm ivory', exact: true }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect
      .poll(async () => (await dispatch(request, 'snapshot', {})).settings.theme)
      .toBe('light');
    const background = await page
      .locator('.app-shell')
      .evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(background).toBe('rgb(246, 245, 241)');
    await expect(page.locator('.toast')).toHaveCount(0);
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.locator('.sidebar').getByRole('button', { name: /^Live/ }).click();
    await page.locator('.session-row-main').first().click();
    await expect(page.locator('.inspector-stats')).toBeVisible();
    await expectPaintedCurve(page, '.inspector .recharts-area-curve');
    await page.screenshot({
      path: 'artifacts/live-light.png',
      fullPage: true,
      animations: 'disabled',
    });
    await page.setViewportSize({ width: 1440, height: 960 });
    await navigate(page, 'Analytics');
    await expect(
      page.getByRole('heading', { name: 'A little perspective', exact: true }),
    ).toBeVisible();
    await expect(page.locator('.trend-panel .recharts-surface')).toBeVisible();
    await expectPaintedCurve(page, '.trend-panel .recharts-area-curve');
    await page.screenshot({
      path: 'artifacts/analytics-light.png',
      fullPage: true,
      animations: 'disabled',
    });
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  } finally {
    await dispatch(request, 'settings_save', { settings: { theme: before.settings.theme } });
  }
});

test('a custom rolling token budget saves its window and can be removed', async ({
  page,
  request,
}) => {
  const name = 'Interaction window ' + Date.now();
  let id: string | undefined;
  try {
    await page.goto('/');
    await expect(page.locator('.session-row-main').first()).toBeVisible();
    await navigate(page, 'Budgets & Limits');
    await page.getByRole('button', { name: 'Create budget', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Create a local budget', exact: true });
    await dialog.getByLabel('Budget name', { exact: true }).fill(name);
    await dialog.getByLabel('Budget amount', { exact: true }).fill('5000000');
    await dialog.getByRole('combobox', { name: /^Unit/ }).selectOption('tokens');
    await dialog.getByRole('combobox', { name: /^Period/ }).selectOption('window');
    await dialog.getByLabel('Window length (minutes)', { exact: true }).fill('120');
    await dialog.getByLabel('Alert threshold (%)', { exact: true }).fill('73');
    expect(
      await dialog
        .getByLabel('Budget amount', { exact: true })
        .evaluate((input: HTMLInputElement) => input.validationMessage),
    ).toBe('');
    await dialog.getByRole('button', { name: 'Save budget', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    const card = page
      .locator('.budget-card')
      .filter({ has: page.getByRole('heading', { name, exact: true }) });
    await expect(card).toContainText('Rolling 120-minute window');
    await expect(card).toContainText('Alert at 73%');
    const snapshot = await dispatch(request, 'snapshot', {});
    const saved = snapshot.budgets.find((b) => b.name === name);
    expect(saved).toMatchObject({
      name,
      amount: 5000000,
      unit: 'tokens',
      period: 'window',
      window_minutes: 120,
      threshold: 73,
    });
    id = saved?.id;
    await card.getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(card).toHaveCount(0);
    expect((await dispatch(request, 'snapshot', {})).budgets.some((b) => b.id === id)).toBe(false);
    id = undefined;
  } finally {
    // Also clean up if a UI assertion fails after the backend accepted creation.
    const snapshot = await dispatch(request, 'snapshot', {});
    const leftover = snapshot.budgets.find((b) => b.id === id || b.name === name);
    if (leftover) await dispatch(request, 'budget_remove', { id: leftover.id });
  }
});

test('analytics paints real stacked usage and a chart point drills into its exact day', async ({
  page,
  request,
}) => {
  const snapshot = await dispatch(request, 'snapshot', {});
  expect(snapshot.daily.length).toBeGreaterThan(3);
  const index = Math.floor(snapshot.daily.length / 2);
  const day = snapshot.daily[index];
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await navigate(page, 'Analytics');
  await expectPaintedCurve(page, '.trend-panel .recharts-area-curve');
  await expect(page.locator('.trend-panel .recharts-area-curve')).toHaveCount(4);
  await expect(page.locator('.trend-panel .recharts-area-area')).toHaveCount(4);
  const extent = await page.locator('.trend-panel .recharts-area-curve').first().boundingBox();
  expect(extent).not.toBeNull();
  const x = extent!.x + (extent!.width * index) / (snapshot.daily.length - 1);
  const y = extent!.y + extent!.height / 2;
  await page.mouse.move(x, y);
  await expect(page.locator('.chart-tooltip strong')).toHaveText(day.label);
  await page.mouse.click(x, y);
  await expect(page.locator('.breadcrumbs strong')).toHaveText('Sessions');
  await expect(page.locator('.filter-chip').filter({ hasText: 'from: ' + day.key })).toBeVisible();
  await expect(page.locator('.filter-chip').filter({ hasText: 'to: ' + day.key })).toBeVisible();
  const filter = await page.evaluate(() =>
    JSON.parse(localStorage.getItem('chip-count:filter') || '{}'),
  );
  expect(filter.from).toBe(day.key);
  expect(filter.to).toBe(day.key);
  const expected = await dispatch(request, 'snapshot', { filter: { from: day.key, to: day.key } });
  await expect(page.locator('.heading-count')).toHaveText(String(expected.total_sessions));
  expect(expected.totals.total).toBe(day.total);
});
