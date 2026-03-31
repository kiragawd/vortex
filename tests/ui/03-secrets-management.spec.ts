import { test, expect } from '@playwright/test';

test.describe('03 - Settings Page (Secrets/Auth Providers)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/settings');
    await page.waitForLoadState('networkidle');
  });

  test('Settings page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('General settings card is visible', async ({ page }) => {
    await expect(page.locator('text=General')).toBeVisible();
  });

  test('Authentication Providers section is visible', async ({ page }) => {
    await expect(page.locator('text=Authentication Providers')).toBeVisible();
  });

  test('Instance name and timezone fields are present', async ({ page }) => {
    await expect(page.locator('text=Instance Name')).toBeVisible();
    await expect(page.locator('text=Timezone')).toBeVisible();
  });

  test('Provider list shows at least one provider', async ({ page }) => {
    const providers = page.locator('text=/OIDC|SAML|LDAP|Local/');
    const count = await providers.count();
    expect(count).toBeGreaterThan(0);
  });
});
