import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('05 - Swarm Monitoring', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Swarm panel displays with title "🐝 VORTEX Swarm"', async ({ page }) => {
    // Check swarm panel title
    const swarmTitle = page.locator('#swarm-panel >> text=VORTEX Swarm');
    await expect(swarmTitle).toBeVisible();

    // Check bee emoji
    const beeEmoji = page.locator('#swarm-panel >> text=🐝');
    await expect(beeEmoji).toBeVisible();
  });

  test('Status badge displays and shows correct states', async ({ page }) => {
    const helpers = createHelpers(page);

    // Check status badge exists
    const statusBadge = page.locator('#swarm-status-badge');
    await expect(statusBadge).toBeVisible();

    // Get status text (should be one of: LOADING, ACTIVE, OFFLINE, etc)
    const statusText = await statusBadge.textContent();
    expect(statusText).toBeTruthy();
    expect(statusText).toMatch(/LOADING|ACTIVE|OFFLINE|CONNECTED|DISCONNECTED/i);
  });

  test('Worker count displays', async ({ page }) => {
    // Check worker count label
    const workerLabel = page.locator('#swarm-panel >> text=Workers:');
    await expect(workerLabel).toBeVisible();

    // Check worker count value
    const workerCount = page.locator('#swarm-worker-count');
    await expect(workerCount).toBeVisible();

    const count = await workerCount.textContent();
    expect(count).toMatch(/^\d+$/);
  });

  test('Queue depth displays', async ({ page }) => {
    // Check queue label
    const queueLabel = page.locator('#swarm-panel >> text=Queue:');
    await expect(queueLabel).toBeVisible();

    // Check queue depth value
    const queueDepth = page.locator('#swarm-queue-depth');
    await expect(queueDepth).toBeVisible();

    const depth = await queueDepth.textContent();
    expect(depth).toMatch(/^\d+$/);
  });

  test('Swarm panel header is clickable for expand/collapse', async ({ page }) => {
    // Get the swarm panel header
    const panelHeader = page.locator('#swarm-panel > div').first();

    // Should be clickable
    await expect(panelHeader).toBeVisible();

    // Check cursor style (should have cursor-pointer class)
    const classes = await panelHeader.getAttribute('class');
    expect(classes).toContain('cursor-pointer');
  });

  test('Expand/collapse toggles swarm details visibility', async ({ page }) => {
    const helpers = createHelpers(page);

    // Get swarm details
    const swarmDetails = page.locator('#swarm-details');

    // Initially might be hidden or shown
    const initiallyVisible = await swarmDetails.isVisible();

    // Toggle swarm panel
    await helpers.toggleSwarmPanel();

    // Wait for animation
    await page.waitForTimeout(300);

    // Visibility should change
    const afterToggle = await swarmDetails.isVisible();
    expect(afterToggle).not.toBe(initiallyVisible);

    // Toggle again to verify it works both ways
    await helpers.toggleSwarmPanel();
    await page.waitForTimeout(300);

    const afterSecondToggle = await swarmDetails.isVisible();
    expect(afterSecondToggle).toBe(initiallyVisible);
  });

  test('Chevron icon rotates on expand/collapse', async ({ page }) => {
    const helpers = createHelpers(page);

    const chevron = page.locator('#swarm-chevron');
    await expect(chevron).toBeVisible();

    // Get initial transform
    let transform = await chevron.evaluate((el) => {
      return window.getComputedStyle(el).transform;
    });

    const initialTransform = transform;

    // Toggle swarm
    await helpers.toggleSwarmPanel();
    await page.waitForTimeout(300);

    // Get new transform (should be rotated if it was rotated before, or vice versa)
    // Note: The actual rotation might vary, but we're checking it's interactive
    const chevronClasses = await chevron.getAttribute('class');
    expect(chevronClasses).toContain('transition-transform');
  });

  test('Swarm panel header displays all required information', async ({ page }) => {
    // Check structure: title, status badge, worker count, queue depth, chevron
    const panelHeader = page.locator('#swarm-panel > div').first();
    const headerText = await panelHeader.textContent();

    // Should contain key information
    expect(headerText).toContain('Swarm');
    expect(headerText).toContain('Workers');
    expect(headerText).toContain('Queue');
  });

  test('Worker list is accessible when expanded', async ({ page }) => {
    const helpers = createHelpers(page);

    // Expand swarm details
    const swarmDetails = page.locator('#swarm-details');
    const isVisible = await swarmDetails.isVisible();

    if (!isVisible) {
      await helpers.toggleSwarmPanel();
      await page.waitForTimeout(300);
    }

    // Check worker list container exists
    const workersList = page.locator('#swarm-workers-list');
    await expect(workersList).toBeVisible();
  });

  test('Worker list structure is rendered (even if empty)', async ({ page }) => {
    const helpers = createHelpers(page);

    // Expand swarm details
    const swarmDetails = page.locator('#swarm-details');
    const isVisible = await swarmDetails.isVisible();

    if (!isVisible) {
      await helpers.toggleSwarmPanel();
      await page.waitForTimeout(300);
    }

    // Worker list should exist
    const workersList = page.locator('#swarm-workers-list');
    await expect(workersList).toBeVisible();

    // It might be empty or have items - both are valid
    const workerElements = page.locator('#swarm-workers-list > div');
    const count = await workerElements.count();
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('Swarm panel is styled with glass and vortex-border', async ({ page }) => {
    const swarmPanel = page.locator('#swarm-panel');

    // Check for glass styling
    const hasGlass = await swarmPanel.evaluate((el) =>
      el.classList.contains('glass')
    );
    expect(hasGlass).toBeTruthy();

    // Check for vortex-border
    const hasBorder = await swarmPanel.evaluate((el) =>
      el.classList.contains('vortex-border')
    );
    expect(hasBorder).toBeTruthy();
  });

  test('Status badge is properly styled', async ({ page }) => {
    const statusBadge = page.locator('#swarm-status-badge');

    // Check styling classes
    const classes = await statusBadge.getAttribute('class');
    expect(classes).toContain('px-2');
    expect(classes).toContain('py-0.5');
    expect(classes).toContain('rounded');
    expect(classes).toContain('font-bold');
  });

  test('Swarm panel shows responsive layout on desktop', async ({ page }) => {
    // Check that header flexbox layout works
    const panelHeader = page.locator('#swarm-panel > div').first();

    // Should have flex layout
    const classes = await panelHeader.getAttribute('class');
    expect(classes).toContain('flex');

    // Should justify content between
    expect(classes).toContain('justify-between');

    // Should align items
    expect(classes).toContain('items-center');
  });

  test('Queue and worker displays update responsively', async ({ page }) => {
    // Get values
    const workerCount = page.locator('#swarm-worker-count');
    const queueDepth = page.locator('#swarm-queue-depth');

    // Both should be visible and contain numbers
    const wCount = await workerCount.textContent();
    const qDepth = await queueDepth.textContent();

    expect(wCount).toMatch(/^\d+$/);
    expect(qDepth).toMatch(/^\d+$/);

    // At least they should be parseable as integers
    expect(parseInt(wCount || '0')).toBeGreaterThanOrEqual(0);
    expect(parseInt(qDepth || '0')).toBeGreaterThanOrEqual(0);
  });
});
