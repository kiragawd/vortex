import { test, expect } from '@playwright/test';

test.describe('11 - Dark Mode & Routing', () => {
  test('Light mode is default', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass ?? '').not.toContain('dark');
  });

  test('Theme toggle button is visible', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('button[aria-label="Toggle theme"]')).toBeVisible();
  });

  test('Clicking theme toggle enables dark mode', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass).toContain('dark');
  });

  test('Dark mode applies dark background', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    await expect(page.locator('html.dark')).toBeVisible();
  });

  test('Double toggle returns to light mode', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    await page.locator('button[aria-label="Toggle theme"]').click();
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass ?? '').not.toContain('dark');
  });

  test('SPA routing works for all sidebar pages', async ({ page }) => {
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
    await page.waitForLoadState('networkidle');

    for (const route of routes) {
      await page.locator(`a[href="${route.href}"]`).click();
      await page.waitForURL(`**${route.href}`, { timeout: 10000 });
      await expect(page.locator('h1')).toContainText(route.title, { timeout: 5000 });
    }
  });

  test('Direct URL navigation works without 404', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Sidebar navigation highlights active route', async ({ page }) => {
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    const activeLink = page.locator('a[href="/dags"]');
    const classes = await activeLink.getAttribute('class');
    expect(classes).toContain('vortex');
  });
});
