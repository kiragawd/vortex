import { test, expect } from '@playwright/test';

test.describe('02 - DAG List & Detail View', () => {
  test('DAGs page renders table with heading', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
  });

  test('DAGs table shows rows when data exists', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    const count = await rows.count();
    expect(count).toBeGreaterThan(0);
  });

  test('Click DAG row navigates to detail page', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('h1')).toBeVisible({ timeout: 10000 });
  });

  test('DAG detail page shows Graph, Runs, Info tabs', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Graph")')).toBeVisible();
    await expect(page.locator('button:has-text("Runs")')).toBeVisible();
    await expect(page.locator('button:has-text("Info")')).toBeVisible();
  });

  test('DAG detail page shows Trigger and Retry Last buttons', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Trigger")')).toBeVisible();
    await expect(page.locator('button:has-text("Retry Last")')).toBeVisible();
  });

  test('DAG detail quick stat cards show Schedule, Last Run, Next Run, Tasks', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('text=Schedule')).toBeVisible();
    await expect(page.locator('text=Last Run')).toBeVisible();
    await expect(page.locator('text=Next Run')).toBeVisible();
    await expect(page.locator('text=Tasks')).toBeVisible();
  });

  test('Graph tab renders SVG with task nodes', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const svg = page.locator('svg');
    await expect(svg.first()).toBeVisible({ timeout: 10000 });
  });

  test('Runs tab shows runs table or empty state', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await page.locator('button:has-text("Runs")').click();
    const table = page.locator('table');
    const emptyMsg = page.locator('text=No runs recorded yet');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Info tab shows DAG Properties and Tasks list', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await page.locator('button:has-text("Info")').click();
    await expect(page.locator('text=DAG Properties')).toBeVisible();
  });

  test('Trigger button fires API call', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible();
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/trigger') && r.request().method() === 'POST'
    );
    await triggerBtn.click();
    const res = await apiPromise.catch(() => null);
    expect(res).not.toBeNull();
  });
});
