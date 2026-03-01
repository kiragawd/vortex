import { defineConfig, devices } from '@playwright/test';

/**
 * VORTEX Dashboard Playwright Configuration
 * 
 * Base URL: http://localhost:3000
 * Browsers: Chromium (headless)
 * Timeout: 30 seconds
 * Reporters: HTML + JSON
 */
export default defineConfig({
  testDir: './tests/ui',
  testMatch: '**/*.spec.ts',

  /* Run tests in files in parallel */
  fullyParallel: true,

  /* Fail the build on CI if you accidentally left test.only in the source code */
  forbidOnly: !!process.env.CI,

  /* Retry on CI only */
  retries: process.env.CI ? 2 : 0,

  /* Opt out of parallel tests on CI */
  workers: process.env.CI ? 1 : undefined,

  /* Reporter to use. See https://playwright.dev/docs/test-reporters */
  reporter: [
    ['html'],
    ['json', { outputFile: 'test-results/results.json' }],
  ],

  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    /* Base URL to use in actions like `await page.goto('/')` */
    baseURL: 'http://localhost:3000',

    /* Injects login credentials globally */
    storageState: 'auth.json',

    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: 'on-first-retry',

    /* Screenshot on failure */
    screenshot: 'only-on-failure',
  },

  /* Configure projects for major browsers */
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],

  /* Run your local dev server before starting the tests */
  webServer: {
    command: 'echo "NOTE: Server must be running manually. Start: cargo run --release --bin vortex"',
    port: 3000,
    reuseExistingServer: true,
  },

  /* Global timeout for all tests */
  timeout: 5 * 1000,

  /* Expect timeout */
  expect: {
    timeout: 1 * 1000,
  },
});
