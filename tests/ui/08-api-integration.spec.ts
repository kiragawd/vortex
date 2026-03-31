import { test, expect } from '@playwright/test';

test.describe('08 - API Integration', () => {
  test('GET /api/dags returns paginated data on page load', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/dags') && r.request().method() === 'GET'
    );
    await page.goto('/dags');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('data');
    expect(Array.isArray(body.data)).toBeTruthy();
  });

  test('API requests include Authorization header', async ({ page }) => {
    let authHeader: string | null = null;
    page.on('request', (req) => {
      if (req.url().includes('/api/')) {
        authHeader = req.headers()['authorization'] ?? null;
      }
    });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    expect(authHeader).not.toBeNull();
    expect(authHeader).toContain('Bearer');
  });

  test('GET /api/health returns status', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/health') && r.request().method() === 'GET'
    );
    await page.goto('/');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('status');
  });

  test('GET /api/runs returns paginated data', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/runs') && r.request().method() === 'GET'
    );
    await page.goto('/runs');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('GET /api/dags/:id/tasks returns tasks and dependencies', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/tasks') && r.request().method() === 'GET'
    );
    await rows.first().click();
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('tasks');
    expect(body).toHaveProperty('dependencies');
  });

  test('POST /api/dags/:id/trigger fires correctly', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/trigger') && r.request().method() === 'POST'
    );
    await page.locator('button:has-text("Trigger")').click();
    const res = await apiPromise.catch(() => null);
    expect(res).not.toBeNull();
  });

  test('GET /api/swarm/status returns swarm data', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/swarm/status')
    );
    await page.goto('/swarm');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('GET /api/rbac/roles returns roles', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/rbac/roles')
    );
    await page.goto('/rbac');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('API error handling - invalid endpoint returns error gracefully', async ({ page }) => {
    await page.goto('/');
    const res = await page.request.fetch('http://localhost:3000/api/nonexistent', {
      headers: { Authorization: 'Bearer vortex_admin_key' },
    });
    expect(res.status()).toBeGreaterThanOrEqual(400);
  });
});
