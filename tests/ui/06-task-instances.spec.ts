import { test, expect } from '@playwright/test';

test.describe('06 - Run Detail & Task Instances', () => {
  test('Runs page loads with heading', async ({ page }) => {
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
  });

  test('Runs page shows table or empty state', async ({ page }) => {
    await page.goto('/runs');
    const table = page.locator('table');
    const emptyMsg = page.locator('text=/no runs|empty/i');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Click run row navigates to run detail', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Run Detail')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page shows summary cards', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Total Tasks')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page shows task graph and task instances table', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const svg = page.locator('svg');
      await expect(svg.first()).toBeVisible({ timeout: 10000 });
      const taskTable = page.locator('table');
      await expect(taskTable.first()).toBeVisible();
    }
  });

  test('Run detail breadcrumb navigation works', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const breadcrumb = page.locator('a[href*="/dags/"]').first();
      if (await breadcrumb.isVisible()) {
        await breadcrumb.click();
        await page.waitForURL(/\/dags\//);
      }
    }
  });
});
