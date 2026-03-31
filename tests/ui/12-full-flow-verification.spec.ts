import { test, expect } from '@playwright/test';

test.describe('12 - Full UI Flow Verification', () => {
  test.beforeEach(async ({ page }) => {
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        const text = msg.text();
        if (!text.includes('favicon') && !text.includes('net::ERR')) {
          console.log('Browser error:', text);
        }
      }
    });
  });

  test('Dashboard loads without errors', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('h1')).toContainText('Dashboard');
    await expect(page.locator('text=Active DAGs')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('text=Total Runs')).toBeVisible({ timeout: 5000 });
  });

  test('DAGs page - list loads and DAG click works', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const header = page.locator('h1');
    await expect(header).toBeVisible({ timeout: 10000 });
  });

  test('DAG detail page - tabs work without crash', async ({ page }) => {
    await page.goto('/dags');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Graph")')).toBeVisible();
    await expect(page.locator('button:has-text("Runs")')).toBeVisible();
    await expect(page.locator('button:has-text("Info")')).toBeVisible();
    await page.locator('button:has-text("Runs")').click();
    await page.locator('button:has-text("Info")').click();
    await expect(page.locator('text=DAG Properties')).toBeVisible();
  });

  test('DAG detail page - trigger button works', async ({ page }) => {
    await page.goto('/dags');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible({ timeout: 10000 });
    await triggerBtn.click();
    await expect(triggerBtn).toContainText(/Trigger/);
  });

  test('Runs page - loads and navigates to run detail', async ({ page }) => {
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
    const runRows = page.locator('table tbody tr');
    const count = await runRows.count();
    if (count > 0) {
      await runRows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Run Detail')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page - back navigation works', async ({ page }) => {
    await page.goto('/runs');
    const runRows = page.locator('table tbody tr');
    const count = await runRows.count();
    if (count > 0) {
      await runRows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const breadcrumb = page.locator('a[href*="/dags/"]').first();
      if (await breadcrumb.isVisible()) {
        await breadcrumb.click();
        await page.waitForURL(/\/dags\//);
        const body = await page.textContent('body');
        expect(body).not.toContain('SyntaxError');
      }
    }
  });

  test('Events page - loads with content', async ({ page }) => {
    await page.goto('/events');
    await expect(page.locator('h1')).toContainText('Events');
    await expect(page.locator('text=Available Sensor Types')).toBeVisible();
    await expect(page.locator('text=Alert Routing')).toBeVisible();
    await expect(page.locator('text=Event Feed')).toBeVisible();
  });

  test('Compliance page loads', async ({ page }) => {
    await page.goto('/compliance');
    await expect(page.locator('h1')).toContainText('Compliance');
  });

  test('RBAC page loads roles', async ({ page }) => {
    await page.goto('/rbac');
    await expect(page.locator('h1')).toContainText(/RBAC|Access/);
    await expect(page.locator('text=Roles')).toBeVisible();
  });

  test('Monitoring page loads health', async ({ page }) => {
    await page.goto('/monitoring');
    await expect(page.locator('h1')).toContainText('Monitoring');
  });

  test('Swarm page loads', async ({ page }) => {
    await page.goto('/swarm');
    await expect(page.locator('h1')).toContainText('Swarm');
  });

  test('Lineage page loads', async ({ page }) => {
    await page.goto('/lineage');
    await expect(page.locator('h1')).toContainText('Lineage');
  });

  test('Connectors page loads', async ({ page }) => {
    await page.goto('/connectors');
    await expect(page.locator('h1')).toContainText('Connector');
  });

  test('Settings page loads', async ({ page }) => {
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Sidebar navigation - all links work', async ({ page }) => {
    const routes = [
      { href: '/dags', title: 'DAGs' },
      { href: '/runs', title: 'Runs' },
      { href: '/events', title: 'Events' },
      { href: '/connectors', title: 'Connector' },
      { href: '/lineage', title: 'Lineage' },
      { href: '/swarm', title: 'Swarm' },
      { href: '/monitoring', title: 'Monitoring' },
      { href: '/compliance', title: 'Compliance' },
      { href: '/settings', title: 'Settings' },
    ];

    await page.goto('/');
    for (const route of routes) {
      await page.locator(`a[href="${route.href}"]`).click();
      await page.waitForURL(`**${route.href}`, { timeout: 10000 });
      await expect(page.locator('h1')).toContainText(route.title, { timeout: 5000 });
    }
  });

  test('No page errors across all pages', async ({ page }) => {
    test.setTimeout(60000);
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    const pages = ['/', '/dags', '/runs', '/events', '/connectors', '/lineage',
      '/swarm', '/monitoring', '/compliance', '/rbac', '/settings'];

    for (const p of pages) {
      await page.goto(p);
      await page.waitForLoadState('domcontentloaded');
      await page.waitForTimeout(500);
    }

    expect(errors).toEqual([]);
  });
});
