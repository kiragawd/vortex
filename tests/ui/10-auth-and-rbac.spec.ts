import { test, expect } from '@playwright/test';

test.describe('10 - Auth & Login', () => {
  test('Login page has username and password fields', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('input[type="text"], input#username')).toBeVisible();
    await expect(page.locator('input[type="password"]')).toBeVisible();
  });

  test('Login page has Sign in button', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('button:has-text("Sign in")')).toBeVisible();
  });

  test('Login page shows Ryuo branding', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('text=Ryuo')).toBeVisible();
  });

  test('Login page shows default credentials hint', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('text=admin')).toBeVisible();
  });

  test('Auth token persists across navigation', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Logout button redirects to login', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const logoutBtn = page.locator('button[aria-label="Sign out"]');
    await expect(logoutBtn).toBeVisible();
    await logoutBtn.click();
    await page.waitForURL('**/login', { timeout: 10000 });
    await expect(page.locator('text=Sign in')).toBeVisible();
  });
});
