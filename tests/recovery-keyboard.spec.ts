import { test, expect, type Locator, type Page } from '@playwright/test';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

async function tabTo(page: Page, target: Locator) {
  for (let i = 0; i < 100; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press('Tab');
  }
  throw new Error('Could not reach control by Tab: ' + (await target.getAttribute('aria-label')));
}
async function typeInto(page: Page, target: Locator, value: string) {
  await tabTo(page, target);
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.insertText(value);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem('chip-count:demo', 'true');
    localStorage.setItem('chip-count:page', '"Live"');
    localStorage.setItem('chip-count:filter', '{}');
    localStorage.setItem('chip-count:selected', 'null');
    localStorage.setItem('chip-count:inspector-tab', '"Overview"');
    localStorage.setItem('chip-count:settings-tab', '"General"');
  });
});

test('keyboard traverses virtual session history, filters, settings, Help and export', async ({
  page,
  request,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const response = await request.post('/api/dispatch', {
    data: { command: 'snapshot', args: {}, demo: true },
  });
  const before = await response.json();
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await tabTo(page, page.locator('.session-row-main').first());
  await page.keyboard.press('Enter');
  await expect(page.locator('.inspector-stats')).toBeVisible();
  await page.keyboard.press('End');
  const last = page.locator('.session-row-main').last();
  await expect(last).toBeFocused();
  await expect(last).toHaveAttribute('data-index', String(before.sessions.length - 1));
  await page.keyboard.press('Home');
  await expect(page.locator('.session-row-main').first()).toBeFocused();
  await page.keyboard.press('PageDown');
  await expect(page.locator('.session-row-main[data-index="8"]')).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.session-row-main[data-index="9"]')).toBeFocused();
  await page.keyboard.press('Control+2');
  const filters = page.getByRole('button', { name: /^Filters/ });
  await tabTo(page, filters);
  await page.keyboard.press('Enter');
  const from = page.getByLabel('From', { exact: true });
  await tabTo(page, from);
  await expect(from).toBeFocused();
  await page.keyboard.press('Control+,');
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  const timezone = page.getByLabel(/^Reporting timezone/);
  await typeInto(page, timezone, 'Mars/Invalid');
  const save = page.getByRole('button', { name: 'Save activity preferences' });
  await tabTo(page, save);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toContainText('IANA timezone');
  await expect(timezone).toHaveValue('Mars/Invalid');
  await typeInto(page, timezone, before.settings.timezone);
  await tabTo(page, save);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.keyboard.press('Control+e');
  const dialog = page.getByRole('dialog', { name: 'Export usage' });
  await expect(dialog.getByLabel('File format')).toBeFocused();
  await page.keyboard.press('j');
  await page.keyboard.press('Enter');
  const exportButton = dialog.getByRole('button', { name: 'Export JSON', exact: true });
  await tabTo(page, exportButton);
  const download = page.waitForEvent('download');
  await page.keyboard.press('Enter');
  expect((await download).suggestedFilename()).toMatch(/\.json$/);
  await expect(dialog).not.toBeVisible();
  await expect(save).toBeFocused();
  const help = page.locator('.sidebar').getByRole('button', { name: 'Help', exact: true });
  await tabTo(page, help);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Chip Count Help' })).toContainText('reselect');
  await page.keyboard.press('Escape');
  await expect(help).toBeFocused();
  expect(errors).toEqual([]);
});

test('keyboard can configure and retry an inaccessible source without discarding history', async ({
  page,
  request,
}) => {
  const folder = await mkdtemp(join(tmpdir(), 'chip-keyboard-source-'));
  const path = join(folder, 'session.jsonl');
  await writeFile(
    path,
    JSON.stringify({
      type: 'assistant',
      timestamp: new Date().toISOString(),
      sessionId: folder,
      cwd: folder,
      requestId: folder,
      message: {
        id: folder,
        model: 'claude-sonnet-4-5',
        usage: { input_tokens: 100, output_tokens: 10 },
      },
    }) + '\nBROKEN PRIVATE LOG\n',
  );
  await page.addInitScript(() => localStorage.setItem('chip-count:demo', 'false'));
  const real = async (command: string, args = {}) => {
    const r = await request.post('/api/dispatch', { data: { command, args, demo: false } });
    expect(r.ok(), await r.text()).toBe(true);
    return r.json();
  };
  let id: string | undefined;
  try {
    await page.goto('/');
    await page.keyboard.press('Control+7');
    const add = page.getByRole('button', { name: 'Add source', exact: true });
    await tabTo(page, add);
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('dialog', { name: 'Add a source' });
    await typeInto(page, dialog.getByLabel('Source / profile label'), 'Keyboard source');
    await typeInto(page, dialog.getByLabel('Absolute local path'), path);
    await tabTo(page, dialog.getByRole('button', { name: 'Connect source', exact: true }));
    await page.keyboard.press('Enter');
    await expect(dialog).not.toBeVisible();
    const data = await real('snapshot');
    id = data.sources.find(
      (s: any) => s.path.includes(folder) || s.label === 'Keyboard source',
    )?.id;
    expect(id).toBeTruthy();
    const card = page.locator('.source-card').filter({ hasText: 'Keyboard source' });
    await expect(card).toContainText('Some records could not be indexed');
    await tabTo(page, card.locator('summary'));
    await page.keyboard.press('Enter');
    await expect(card).toContainText(':2');
    await expect(card).not.toContainText('BROKEN PRIVATE');
    const before = await real('snapshot', { filter: { project: folder } });
    await rm(path);
    await real('rescan');
    await page.reload();
    await page.keyboard.press('Control+7');
    await expect(card).toContainText('Reconnect it or choose another path');
    await tabTo(page, card.getByRole('button', { name: 'Configure', exact: true }));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('dialog', { name: 'Configure source' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(card.getByRole('button', { name: 'Configure', exact: true })).toBeFocused();
    const after = await real('snapshot', { filter: { project: folder } });
    expect(after.totals).toEqual(before.totals);
  } finally {
    const data = await real('snapshot');
    const source = data.sources.find((s: any) => s.id === id || s.label === 'Keyboard source');
    if (source) await real('source_remove', { id: source.id });
    await rm(folder, { recursive: true, force: true });
  }
});

test('chart date selector offers the same drilldown using keyboard', async ({ page, request }) => {
  const r = await request.post('/api/dispatch', {
    data: { command: 'snapshot', args: {}, demo: true },
  });
  const data = await r.json();
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page.keyboard.press('Control+3');
  const date = page.getByLabel('Usage date', { exact: true });
  await tabTo(page, date);
  await page.keyboard.type(data.daily[1].label);
  await tabTo(page, page.getByRole('button', { name: 'Inspect day' }));
  await page.keyboard.press('Enter');
  await expect(page.locator('.breadcrumbs strong')).toHaveText('Sessions');
  await expect(
    page.locator('.filter-chip').filter({ hasText: 'from: ' + data.daily[1].key }),
  ).toBeVisible();
  await expect(
    page.locator('.filter-chip').filter({ hasText: 'to: ' + data.daily[1].key }),
  ).toBeVisible();
});

test('failed rate validation stays in the dialog and Escape returns focus', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  await page.keyboard.press('Control+,');
  await tabTo(page, page.getByRole('button', { name: 'Pricing', exact: true }));
  await page.keyboard.press('Enter');
  const add = page.getByRole('button', { name: 'Add model override' });
  await tabTo(page, add);
  await page.keyboard.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'Local pricing override' });
  await typeInto(page, dialog.getByLabel('Exact model identifier'), 'invalid-test');
  await typeInto(page, dialog.getByLabel('input · USD / 1M', { exact: true }), '1000001');
  await tabTo(page, dialog.getByRole('button', { name: 'Save override', exact: true }));
  await page.keyboard.press('Enter');
  await expect(dialog.getByRole('alert')).toContainText('finite nonnegative price');
  await expect(dialog.getByLabel('input · USD / 1M', { exact: true })).toHaveValue('1000001');
  await page.keyboard.press('Escape');
  await expect(add).toBeFocused();
});

test('session paging is keyboard reachable and respects a configured page size', async ({
  page,
  request,
}) => {
  await page.addInitScript(() =>
    localStorage.setItem('chip-count:filter', JSON.stringify({ limit: 5 })),
  );
  const r = await request.post('/api/dispatch', {
    data: { command: 'snapshot', args: { filter: { limit: 5, offset: 5 } }, demo: true },
  });
  const expected = await r.json();
  await page.goto('/');
  await expect(page.locator('.session-row-main').first()).toBeVisible();
  const next = page.getByRole('button', { name: 'Next session page' });
  await tabTo(page, next);
  await page.keyboard.press('Enter');
  await expect(page.locator('.session-row-main').first()).toHaveAttribute(
    'data-session-id',
    expected.sessions[0].id,
  );
  await expect(page.getByRole('status').filter({ hasText: 'Sessions 6–10' })).toBeVisible();
  await expect(next).toBeFocused();
  const previous = page.getByRole('button', { name: 'Previous session page' });
  await page.keyboard.press('Shift+Tab');
  await expect(previous).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('status').filter({ hasText: 'Sessions 1–5' })).toBeVisible();
});
