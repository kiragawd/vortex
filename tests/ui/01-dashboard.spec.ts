import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('01 - Dashboard Rendering', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Page loads without errors', async ({ page }) => {
    // Check for any console errors
    const errors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        errors.push(msg.text());
      }
    });

    // Wait a moment for any errors to be logged
    await page.waitForTimeout(500);

    expect(errors.length).toBe(0);
  });

  test('Navigation bar displays VORTEX logo, Username, and LOGOUT button', async ({ page }) => {
    // Check VORTEX logo
    const logo = page.locator('nav >> text=/VORTEX/');
    await expect(logo).toBeVisible();

    // Check Username indicator
    const username = page.locator('#nav-username');
    await expect(username).toBeVisible();
    await expect(username).toHaveText('admin');

    // Check Logout button
    const logoutBtn = page.locator('nav >> button:has-text("LOGOUT")');
    await expect(logoutBtn).toBeVisible();
  });

  test('Navigation buttons: Users and Secrets are visible', async ({ page }) => {
    // Check Users button
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await expect(usersBtn).toBeVisible();

    // Check Secrets button
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await expect(secretsBtn).toBeVisible();
  });

  test('Stats cards render with correct labels and values', async ({ page }) => {
    // Total DAGs stat
    const totalDagsValue = page.locator('#stat-total');
    await expect(totalDagsValue).toBeVisible();

    // Active stat
    const activeValue = page.locator('#stat-active');
    await expect(activeValue).toBeVisible();
    expect(await activeValue.textContent()).toMatch(/^\d+$/);

    // Paused stat
    const pausedValue = page.locator('#stat-paused');
    await expect(pausedValue).toBeVisible();
    expect(await pausedValue.textContent()).toMatch(/^\d+$/);

    // Workers stat
    const workersValue = page.locator('#stat-workers');
    await expect(workersValue).toBeVisible();
    expect(await workersValue.textContent()).toMatch(/^\d+$/);

    // Queue stat
    const queueValue = page.locator('#stat-queue');
    await expect(queueValue).toBeVisible();
    expect(await queueValue.textContent()).toMatch(/^\d+$/);
  });

  test('Refresh button exists and is clickable', async ({ page }) => {
    const refreshBtn = page.locator('button:has-text("Refresh")').first();
    await expect(refreshBtn).toBeVisible();

    // Click refresh
    await refreshBtn.click();

    // Wait for network to settle
    await page.waitForLoadState('networkidle');

    // Page should still be responsive
    const dagContainer = page.locator('#view-registry');
    await expect(dagContainer).toBeVisible();
  });

  test('DAG list is visible and populated', async ({ page, request }) => {
    const helpers = createHelpers(page);

    // Fetch DAGs to verify API works
    const dags = await helpers.fetchDAGs();
    expect(Array.isArray(dags)).toBeTruthy();

    // Check if DAG list container exists
    const dagList = page.locator('#dag-list');
    await expect(dagList).toBeVisible();

    if (dags.length > 0) {
      // If DAGs exist, verify at least one is rendered
      const dagCards = page.locator('#dag-list > div');
      const count = await dagCards.count();
      expect(count).toBeGreaterThan(0);
    } else {
      // Empty state is acceptable
      console.log('No DAGs found - empty state is valid');
    }
  });

  test('Swarm panel displays with title, status badge, and controls', async ({ page }) => {
    // Check swarm panel title
    const swarmTitle = page.locator('#swarm-panel >> text=VORTEX Swarm');
    await expect(swarmTitle).toBeVisible();

    // Check status badge
    const statusBadge = page.locator('#swarm-status-badge');
    await expect(statusBadge).toBeVisible();

    // Check chevron for expand/collapse
    const chevron = page.locator('#swarm-chevron');
    await expect(chevron).toBeVisible();
  });

  test('DAG Registry heading is visible', async ({ page }) => {
    const heading = page.locator('text=DAG Registry');
    await expect(heading).toBeVisible();
  });

  test('Page layout uses glass-morphism and vortex styling', async ({ page }) => {
    // Check for glass class on stat cards
    const glassElements = page.locator('.glass');
    const count = await glassElements.count();
    expect(count).toBeGreaterThan(0);

    // Check for vortex-border on key elements
    const borderElements = page.locator('.vortex-border');
    const borderCount = await borderElements.count();
    expect(borderCount).toBeGreaterThan(0);
  });
});
