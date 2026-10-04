import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem('chip-count:demo', 'true');
    localStorage.setItem('chip-count:page', '"Live"');
    localStorage.setItem('chip-count:filter', '{}');
    localStorage.setItem('chip-count:selected', 'null');
    localStorage.setItem('chip-count:inspector-tab', '"Overview"');
  });
});
test('live workspace opens a persistent inspector and filters real Rust demo data', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Live sessions' })).toBeVisible();
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page.locator('.session-row-main').first().click();
  await expect(page.getByRole('separator', { name: 'Resize session inspector' })).toBeVisible();
  await expect(page.locator('.inspector-stats')).toBeVisible();
  await page.screenshot({ path: 'artifacts/live-desktop.png', fullPage: true });
  await page.getByRole('textbox', { name: 'Search sessions' }).fill('definitely-no-such-project');
  await expect(page.getByRole('heading', { name: 'No matching sessions' })).toBeVisible();
  await page.getByRole('button', { name: 'Clear search', exact: true }).click();
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page.getByRole('combobox', { name: 'Filter by provider' }).selectOption('claude');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await expect(page.locator('.session-row-main').filter({ hasText: 'Codex' })).toHaveCount(0);
  expect(errors).toEqual([]);
});
test('all eight pages render, keyboard palette navigates, laptop layout fits', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  for (const name of [
    'Sessions',
    'Analytics',
    'Projects',
    'Compare',
    'Budgets & Limits',
    'Sources',
    'Settings',
  ]) {
    await page
      .locator('.sidebar')
      .getByRole('button', { name: new RegExp('^' + name) })
      .click();
    await expect(page.locator('.breadcrumbs strong')).toHaveText(name);
    await expect(page.locator('.page-content')).not.toBeEmpty();
  }
  await page.keyboard.press('Control+k');
  await expect(page.getByRole('dialog', { name: 'Quick actions' })).toBeVisible();
  await page.getByRole('button', { name: 'Go to Live' }).click();
  await page.locator('.session-row-main').first().click();
  await expect(page.locator('.inspector-stats')).toBeVisible();
  await page.screenshot({ path: 'artifacts/live-laptop.png', fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  expect(errors).toEqual([]);
});
test('exports download actual indexed metadata and demo can be exited', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page.getByRole('button', { name: 'Export', exact: true }).click();
  await page.getByLabel('File format').selectOption('json');
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export JSON', exact: true }).click();
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/\.json$/);
  await page.getByRole('button', { name: 'Return to my data' }).click();
  await expect(page.locator('.demo-banner')).toHaveCount(0);
  await expect(page.locator('.workspace-switch')).toContainText('Local workspace');
});
test('compact monitor uses the same Rust pipeline', async ({ page }) => {
  await page.goto('/?compact=1');
  await expect(page.locator('.compact-monitor')).toBeVisible();
  await expect(page.getByText('TOKENS TODAY', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Pause monitoring' }).click();
  await expect(page.getByRole('button', { name: 'Resume monitoring' })).toBeVisible();
  await page.getByRole('button', { name: 'Resume monitoring' }).click();
});

test('calendar presets share Rust boundaries across pages and cost legend discloses estimates', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page
    .locator('.sidebar')
    .getByRole('button', { name: /^Analytics/ })
    .click();
  await expect(page.getByRole('heading', { name: 'Usage analytics' })).toBeVisible();
  await expect(page.getByText('Weeks start Monday', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Activity calendar' })).toHaveCount(0);
  for (const [label, period] of [
    ['Today', 'today'],
    ['This Week', 'week'],
    ['This Month', 'month'],
  ]) {
    const responsePromise = page.waitForResponse((r) => {
      if (!r.url().endsWith('/api/dispatch')) return false;
      const body = r.request().postDataJSON();
      return body.command === 'snapshot' && body.args.filter.period === period;
    });
    await page.getByRole('button', { name: label, exact: true }).click();
    const snapshot = await (await responsePromise).json();
    expect(snapshot.reporting.period).toBe(period);
    expect(snapshot.reporting.week_start).toBe('Monday');
    await expect(page.getByLabel('Reporting range')).toContainText(snapshot.reporting.from_local);
    await expect(page.getByText(/Complete prior calendar period:/)).toBeVisible();
  }
  await page.getByRole('button', { name: 'Est. cost', exact: true }).click();
  const trend = page.locator('.trend-panel');
  await expect(trend.locator('.chart-legend')).toContainText('Estimated cost (USD)');
  await expect(trend.locator('.chart-legend')).not.toContainText('Cache read');
  await expect(trend).toContainText('API-equivalent estimate in USD');
  await page.screenshot({ path: 'artifacts/calendar-reporting.png', fullPage: true });
  await page
    .locator('.sidebar')
    .getByRole('button', { name: /^Sessions/ })
    .click();
  const filter = await page.evaluate(() =>
    JSON.parse(localStorage.getItem('chip-count:filter') || '{}'),
  );
  expect(filter.period).toBe('month');
  await expect(page.getByLabel('Reporting range')).toContainText('(end exclusive)');
  expect(errors).toEqual([]);
});
