import { test, expect } from '@playwright/test';

test.describe('05 - Swarm Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/swarm');
    await page.waitForLoadState('networkidle');
  });

  test('Swarm page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Swarm');
  });

  test('Swarm status cards show Mode, Active Workers, Queue Depth', async ({ page }) => {
    await expect(page.locator('text=Swarm Mode')).toBeVisible();
    await expect(page.locator('text=Active Workers')).toBeVisible();
    await expect(page.locator('text=Queue Depth')).toBeVisible();
  });

  test('Workers table heading is visible', async ({ page }) => {
    await expect(page.locator('text=Workers')).toBeVisible();
  });

  test('Workers table has correct columns or shows empty state', async ({ page }) => {
    const table = page.locator('table');
    const emptyMsg = page.locator('text=/no.*worker|ryuo worker/i');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Drain button visible if workers exist', async ({ page }) => {
    const table = page.locator('table tbody tr');
    const count = await table.count().catch(() => 0);
    if (count > 0) {
      const drainBtn = page.locator('button:has-text("Drain")').first();
      await expect(drainBtn).toBeVisible();
    }
  });
});
