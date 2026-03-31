import { test, expect } from '@playwright/test';

test.describe('09 - Responsive Design', () => {
  test('Desktop layout shows sidebar and main content', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('aside')).toBeVisible();
    await expect(page.locator('h1')).toContainText('Dashboard');
  });

  test('Stat cards use grid layout on desktop', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const grid = page.locator('.grid.grid-cols-1').first();
    await expect(grid).toBeVisible();
  });

  test('Mobile viewport renders correctly', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('h1')).toContainText('Dashboard');
  });

  test('Sidebar toggle works to collapse/expand', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('aside')).toBeVisible();
    await page.locator('button[aria-label="Toggle sidebar"]').click();
    await expect(page.locator('aside')).not.toBeVisible();
    await page.locator('button[aria-label="Toggle sidebar"]').click();
    await expect(page.locator('aside')).toBeVisible();
  });

  test('Tables are scrollable on small viewports', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    const table = page.locator('table');
    if (await table.isVisible().catch(() => false)) {
      const wrapper = page.locator('.overflow-x-auto');
      const count = await wrapper.count();
      expect(count).toBeGreaterThan(0);
    }
  });

  test('Tablet viewport renders DAGs page', async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('h1')).toContainText('DAGs');
  });

  test('Navigation works at all viewports', async ({ page }) => {
    for (const size of [
      { width: 1920, height: 1080 },
      { width: 768, height: 1024 },
      { width: 375, height: 667 },
    ]) {
      await page.setViewportSize(size);
      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await expect(page.locator('h1')).toBeVisible();
    }
  });
});
