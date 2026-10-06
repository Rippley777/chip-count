import { test, expect } from '@playwright/test';

// The accounting requests still run in Rust. Only native panels/events are injected,
// allowing cancellation and OS write failures to be exercised deterministically.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem('chip-count:demo', 'true');
    localStorage.setItem('chip-count:page', '"Sources"');
    localStorage.setItem('chip-count:filter', '{}');
    localStorage.setItem('chip-count:selected', 'null');
    const callbacks = new Map<number, (value: unknown) => void>();
    let callbackId = 0;
    (window as any).nativeTest = {
      failSave: false,
      corrupt: false,
      sourceCancel: true,
      navigate: null,
    };
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    (window as any).__TAURI_INTERNALS__ = {
      transformCallback: (fn: (value: unknown) => void) => {
        callbacks.set(++callbackId, fn);
        return callbackId;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any) => {
        const state = (window as any).nativeTest;
        if (command === 'plugin:event|listen') {
          if (args.event === 'navigate') state.navigate = callbacks.get(args.handler);
          return 1;
        }
        if (command === 'plugin:event|unlisten') return;
        if (command === 'monitoring') return { paused: false };
        if (command === 'plugin:dialog|open') return null;
        if (command === 'save_export') {
          if (state.failSave)
            throw {
              category: 'save_failed',
              message: 'Could not save export.',
              next_steps: 'Choose a writable destination and retry. The report remains available.',
              diagnostics: 'Permission denied at destination',
            };
          return { saved: false };
        }
        if (command === 'restore_index') return { restored: false };
        if (command === 'dispatch') {
          if (state.corrupt && args.request.command === 'snapshot')
            throw {
              category: 'index_corrupt',
              message: 'The local index could not be read.',
              next_steps:
                'Preserve a recovery copy, then restore a known-good backup. Your original index and notes are kept.',
              diagnostics: 'Index: /test/chip-count.sqlite\nNot a database',
            };
          if (state.corrupt && args.request.command === 'index_preserve')
            return { ok: true, backup: '/test/recovery-copy' };
          const r = await fetch('/api/dispatch', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(args.request),
          });
          const data = await r.json();
          if (!r.ok) throw data.error;
          return data;
        }
        throw new Error('Unexpected native command: ' + command);
      },
    };
  });
});

test('native source cancellation keeps the draft and returns focus to the chooser', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Add source', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Add a source' });
  await dialog.getByLabel('Source / profile label').fill('Keep draft');
  const chooser = dialog.getByRole('button', { name: 'Choose folder' });
  await chooser.focus();
  await page.keyboard.press('Enter');
  await expect(chooser).toBeFocused();
  await expect(dialog.getByLabel('Source / profile label')).toHaveValue('Keep draft');
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('native export failure is persistent and a cancelled retry is neutral', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.source-card').first()).toBeVisible();
  await page.keyboard.press('Control+e');
  const dialog = page.getByRole('dialog', { name: 'Export usage' });
  const format = dialog.getByLabel('File format');
  await expect(format).toBeFocused();
  await page.keyboard.press('j');
  await page.keyboard.press('Enter');
  await page.evaluate(() => {
    (window as any).nativeTest.failSave = true;
  });
  const exportButton = dialog.getByRole('button', { name: 'Export JSON', exact: true });
  await exportButton.focus();
  await page.keyboard.press('Enter');
  await expect(dialog.getByRole('alert')).toContainText('writable destination');
  await expect(format).toHaveValue('json');
  await page.evaluate(() => {
    (window as any).nativeTest.failSave = false;
  });
  await exportButton.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await expect(page.getByRole('status')).toContainText('Export cancelled');
  await expect(exportButton).toBeFocused();
});

test('structured corrupt startup shows recovery, preserves copies and treats restore cancellation neutrally', async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as any).nativeTest.corrupt = true;
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Open your local index' })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('original index and notes are kept');
  await page.getByRole('button', { name: 'Preserve recovery copy', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('/test/recovery-copy');
  await page.getByRole('button', { name: 'Select backup to restore…' }).click();
  await expect(page.getByRole('alert')).toHaveCount(1);
  await expect(page.getByRole('heading', { name: 'Open your local index' })).toBeVisible();
});

test('native Settings and Help events reach the main window', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.source-card').first()).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => typeof (window as any).nativeTest.navigate))
    .toBe('function');
  await page.evaluate(() => {
    (window as any).nativeTest.navigate({ payload: 'settings' });
  });
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible();
  await page.evaluate(() => {
    (window as any).nativeTest.navigate({ payload: 'help' });
  });
  await expect(page.getByRole('dialog', { name: 'Chip Count Help' })).toBeVisible();
});
