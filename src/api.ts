import type { Commands, ExportResult, Settings } from './types';
export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export async function api<K extends keyof Commands>(
  command: K,
  args: Commands[K]['args'],
  demo = false,
): Promise<Commands[K]['result']> {
  const request = { command, args, demo };
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<Commands[K]['result']>('dispatch', { request });
  }
  let response: Response;
  try {
    response = await fetch('/api/dispatch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
  } catch {
    throw new Error(
      'The local Rust service is unavailable. Start Chip Count with npm run dev, or open the desktop app.',
    );
  }
  const body = await response.json().catch(() => {
    throw new Error(
      'The local Rust service is unavailable. Run npm run dev to start both services.',
    );
  });
  if (!response.ok)
    throw new Error(body.error || 'The local index could not complete this request.');
  return body;
}
export async function pickPath(directory = true): Promise<string | null> {
  if (!isDesktop)
    throw new Error(
      'Native folder and file selection is available in the desktop app. Enter an absolute path here to use the browser preview.',
    );
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({
    directory,
    multiple: false,
    ...(!directory ? { filters: [{ name: 'Session logs', extensions: ['jsonl'] }] } : {}),
  });
  return typeof result === 'string' ? result : null;
}
export async function saveExport(result: ExportResult): Promise<void> {
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    const saved = await invoke<{ saved: boolean }>('save_export', {
      content: result.content,
      filename: result.filename,
    });
    if (!saved.saved) throw new Error('Export cancelled.');
    return;
  }
  const url = URL.createObjectURL(new Blob([result.content], { type: result.mime }));
  const link = document.createElement('a');
  link.href = url;
  link.download = result.filename;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}
export async function revealPath(path: string): Promise<void> {
  if (!isDesktop) throw new Error('Reveal in Finder is available in the desktop app.');
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('reveal_path', { path });
}
export async function setMonitoring(paused: boolean): Promise<void> {
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('monitoring', { paused });
  } else {
    const res = await fetch('/api/monitoring', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ paused }),
    });
    if (!res.ok) throw new Error('Could not change monitoring.');
  }
}
export async function openCompact(): Promise<void> {
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('compact');
  } else window.open('/?compact=1', 'chip-count-monitor', 'width=430,height=520');
}
export async function saveDesktopSettings(settings: Partial<Settings>): Promise<void> {
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('desktop_settings', { settings });
  }
}
export async function getMonitoring(): Promise<{ paused: boolean }> {
  if (isDesktop) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke('monitoring');
  }
  const response = await fetch('/api/health');
  if (!response.ok) throw new Error('Monitoring status is unavailable.');
  return response.json();
}
