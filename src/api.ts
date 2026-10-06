import type { Commands, ExportResult, Settings } from './types';
export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const isAppStore = isDesktop && import.meta.env.VITE_APP_STORE === '1';
export interface UserErrorData {
  category: string;
  message: string;
  next_steps: string;
  diagnostics: string;
}
export class UserFacingError extends Error {
  constructor(public data: UserErrorData) {
    super(data.message);
  }
  override toString() {
    return this.data.message + ' ' + this.data.next_steps;
  }
}
export function userError(error: unknown): UserFacingError {
  if (error instanceof UserFacingError) return error;
  if (typeof error === 'object' && error && 'category' in error && 'message' in error)
    return new UserFacingError(error as UserErrorData);
  return new UserFacingError({
    category: 'service_unavailable',
    message: error instanceof Error ? error.message : String(error),
    next_steps: 'Retry after checking local access. Your draft and saved metadata are retained.',
    diagnostics: '',
  });
}
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: nativeInvoke } = await import('@tauri-apps/api/core');
  try {
    return await nativeInvoke<T>(command, args);
  } catch (e) {
    throw userError(e);
  }
}
export async function restoreIndex(): Promise<{ restored: boolean; backup?: string }> {
  return invoke('restore_index');
}
export async function api<K extends keyof Commands>(
  command: K,
  args: Commands[K]['args'],
  demo = false,
): Promise<Commands[K]['result']> {
  const request = { command, args, demo };
  if (isDesktop) {
    if (command === 'settings_save') {
      const settings = (args as Commands['settings_save']['args']).settings;
      if (
        !demo &&
        ['close_to_tray', 'launch_at_login', 'notifications'].some((key) => key in settings)
      )
        return invoke<Commands[K]['result']>('desktop_settings', { settings });
    }
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
    throw userError(body.error || 'The local index could not complete this request.');
  return body;
}
export async function pickPath(
  directory = true,
): Promise<{ path: string; selection_id?: string } | null> {
  if (!isDesktop)
    throw new Error(
      'Native folder and file selection is available in the desktop app. Enter an absolute path here to use the browser preview.',
    );
  if (isAppStore) {
    return invoke('select_source', { directory });
  }
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({
    directory,
    multiple: false,
    ...(!directory ? { filters: [{ name: 'Session logs', extensions: ['jsonl'] }] } : {}),
  });
  return typeof result === 'string' ? { path: result } : null;
}
export async function saveExport(result: ExportResult): Promise<boolean> {
  if (isDesktop) {
    const saved = await invoke<{ saved: boolean }>('save_export', {
      content: result.content,
      filename: result.filename,
    });
    return saved.saved;
  }
  const url = URL.createObjectURL(new Blob([result.content], { type: result.mime }));
  const link = document.createElement('a');
  link.href = url;
  link.download = result.filename;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
  return true;
}
export async function revealPath(path: string): Promise<void> {
  if (!isDesktop) throw new Error('Reveal in Finder is available in the desktop app.');
  await invoke('reveal_path', { path });
}
export async function setMonitoring(paused: boolean): Promise<void> {
  if (isDesktop) {
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
    await invoke('compact');
  } else window.open('/?compact=1', 'chip-count-monitor', 'width=430,height=520');
}
export async function saveDesktopSettings(settings: Partial<Settings>): Promise<void> {
  if (isDesktop) {
    await invoke('desktop_settings', { settings });
  }
}
export async function getMonitoring(): Promise<{ paused: boolean }> {
  if (isDesktop) {
    return invoke('monitoring');
  }
  const response = await fetch('/api/health');
  if (!response.ok) throw new Error('Monitoring status is unavailable.');
  return response.json();
}
