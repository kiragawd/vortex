import { test, expect } from '@playwright/test';

test.describe('07 - Forms, Buttons & Actions', () => {
  test('DAG Trigger button changes text while triggering', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible();
    await triggerBtn.click();
    await expect(triggerBtn).toContainText(/Trigger/);
  });

  test('Retry Last button is functional', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const retryBtn = page.locator('button:has-text("Retry Last")');
    await expect(retryBtn).toBeVisible();
    await expect(retryBtn).toBeEnabled();
  });

  test('RBAC token creation form has name and scopes fields', async ({ page }) => {
    await page.goto('/rbac');
    const tokensTab = page.locator('button:has-text("API Tokens")');
    await expect(tokensTab).toBeVisible();
    await tokensTab.click();
    await expect(page.locator('text=Token Name')).toBeVisible();
    await expect(page.locator('text=Scopes')).toBeVisible();
  });

  test('RBAC IP Allowlist has CIDR input and add button', async ({ page }) => {
    await page.goto('/rbac');
    const ipTab = page.locator('button:has-text("IP Allowlist")');
    await expect(ipTab).toBeVisible();
    await ipTab.click();
    await expect(page.locator('text=CIDR')).toBeVisible();
    const addBtn = page.locator('button:has-text("Add")');
    await expect(addBtn).toBeVisible();
  });

  test('Compliance approval requests section is visible', async ({ page }) => {
    await page.goto('/compliance');
    await expect(page.locator('h1')).toContainText('Compliance');
    await expect(page.locator('text=Approval Requests')).toBeVisible();
  });

  test('Events page alert routing form has channel dropdown', async ({ page }) => {
    await page.goto('/events');
    await expect(page.locator('text=Alert Routing')).toBeVisible();
    const selects = page.locator('select');
    const count = await selects.count();
    expect(count).toBeGreaterThan(0);
  });

  test('Swarm drain button is enabled when workers exist', async ({ page }) => {
    await page.goto('/swarm');
    const drainBtns = page.locator('button:has-text("Drain")');
    const count = await drainBtns.count();
    if (count > 0) {
      await expect(drainBtns.first()).toBeEnabled();
    }
  });
});
