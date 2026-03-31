import { test, expect } from '@playwright/test';

test.describe('01 - Dashboard Rendering', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Dashboard heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Dashboard');
  });

  test('Stat cards render: System Status, Active DAGs, Total Runs, Failed Runs', async ({ page }) => {
    await expect(page.locator('text=System Status')).toBeVisible();
    await expect(page.locator('text=Active DAGs')).toBeVisible();
    await expect(page.locator('text=Total Runs')).toBeVisible();
    await expect(page.locator('text=Failed Runs')).toBeVisible();
  });

  test('Stat card values render as numbers or status text', async ({ page }) => {
    // Each stat card has a large value text (text-3xl font-bold)
    const values = page.locator('.text-3xl.font-bold');
    const count = await values.count();
    expect(count).toBeGreaterThanOrEqual(4);
  });

  test('Recent Runs section is visible', async ({ page }) => {
    await expect(page.locator('text=Recent Runs')).toBeVisible();
  });

  test('Recent Runs table shows runs or empty message', async ({ page }) => {
    // Either a table with rows or "No runs yet." message
    const table = page.locator('table');
    const emptyMsg = page.locator('text=No runs yet');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Recent Runs table has correct column headers', async ({ page }) => {
    const table = page.locator('table');
    if (await table.isVisible().catch(() => false)) {
      await expect(page.locator('th:has-text("DAG")')).toBeVisible();
      await expect(page.locator('th:has-text("Run ID")')).toBeVisible();
      await expect(page.locator('th:has-text("Status")')).toBeVisible();
      await expect(page.locator('th:has-text("Started")')).toBeVisible();
    }
  });

  test('Sidebar is visible with navigation links', async ({ page }) => {
    const sidebar = page.locator('aside');
    await expect(sidebar).toBeVisible();

    // Key navigation items
    await expect(page.locator('a[href="/dags"]')).toBeVisible();
    await expect(page.locator('a[href="/runs"]')).toBeVisible();
    await expect(page.locator('a[href="/settings"]')).toBeVisible();
  });

  test('Header shows theme toggle, user profile, and logout button', async ({ page }) => {
    await expect(page.locator('button[aria-label="Toggle theme"]')).toBeVisible();
    await expect(page.locator('button[aria-label="Sign out"]')).toBeVisible();
    // User avatar/name area
    const userArea = page.locator('text=admin');
    await expect(userArea).toBeVisible();
  });

  test('Vortex branding in sidebar', async ({ page }) => {
    await expect(page.locator('aside >> text=Vortex')).toBeVisible();
    await expect(page.locator('aside >> text=Enterprise')).toBeVisible();
  });

  test('No page errors on dashboard load', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);
    expect(errors).toEqual([]);
  });
});
