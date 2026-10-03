import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  timeout: 45000,
  use: {
    baseURL: 'http://127.0.0.1:1431',
    viewport: { width: 1440, height: 960 },
    trace: 'retain-on-failure',
  },
  reporter: 'list',
  webServer: {
    command: 'npm run dev',
    url: 'http://127.0.0.1:1431',
    reuseExistingServer: true,
    timeout: 180000,
    env: { CHIP_COUNT_DATA_DIR: './artifacts/browser-index' },
  },
});
