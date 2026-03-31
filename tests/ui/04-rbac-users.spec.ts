import { test, expect } from '@playwright/test';

test.describe('04 - RBAC Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/rbac');
    await page.waitForLoadState('networkidle');
  });

  test('RBAC page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText(/RBAC|Access/);
  });

  test('Roles & Permissions tab content is visible', async ({ page }) => {
    await expect(page.locator('text=Roles')).toBeVisible();
  });

  test('API Tokens tab is clickable and shows token UI', async ({ page }) => {
    const tokensTab = page.locator('button:has-text("API Tokens")');
    await expect(tokensTab).toBeVisible();
    await tokensTab.click();
    await expect(page.locator('text=Token Name')).toBeVisible();
  });

  test('IP Allowlist tab is clickable', async ({ page }) => {
    const ipTab = page.locator('button:has-text("IP Allowlist")');
    await expect(ipTab).toBeVisible();
    await ipTab.click();
    await expect(page.locator('text=CIDR')).toBeVisible();
  });

  test('Role cards are displayed', async ({ page }) => {
    const roleCards = page.locator('.rounded-xl').filter({ hasText: /Admin|Operator|Viewer/ });
    const count = await roleCards.count();
    expect(count).toBeGreaterThan(0);
  });
});
