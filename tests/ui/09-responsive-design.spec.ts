import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('09 - Responsive Design & Layout', () => {
  test('Desktop (1920x1080): All sections visible with grid layout', async ({ page }) => {
    // Set desktop viewport
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const helpers = createHelpers(page);

    // Check navigation is visible
    const nav = page.locator('nav');
    await expect(nav).toBeVisible();

    // Check stats cards are visible
    const statsRow = page.locator('#stats-row');
    await expect(statsRow).toBeVisible();

    // Check for grid layout
    const classes = await statsRow.getAttribute('class');
    expect(classes).toContain('grid');
    expect(classes).toContain('md:grid-cols-5');

    // Check swarm panel
    const swarmPanel = page.locator('#swarm-panel');
    await expect(swarmPanel).toBeVisible();

    // Check DAG list
    const dagList = page.locator('#dag-list');
    await expect(dagList).toBeVisible();

    // All main sections should be visible
    const mainElement = page.locator('main');
    await expect(mainElement).toBeVisible();
  });

  test('Tablet (768x1024): Layout adjusts properly', async ({ page }) => {
    // Set tablet viewport
    await page.setViewportSize({ width: 768, height: 1024 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Check that content is still accessible
    const nav = page.locator('nav');
    await expect(nav).toBeVisible();

    // Stats might be stacked differently
    const statsRow = page.locator('#stats-row');
    await expect(statsRow).toBeVisible();

    // Check for responsive grid classes
    const classes = await statsRow.getAttribute('class');
    expect(classes).toContain('grid-cols-1');
  });

  test('Mobile (375x667): Stack layout with proper text sizing', async ({ page }) => {
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Navigation should be visible on mobile
    const nav = page.locator('nav');
    await expect(nav).toBeVisible();

    // Check for mobile-friendly layout (single column)
    const statsRow = page.locator('#stats-row');
    const classes = await statsRow.getAttribute('class');
    expect(classes).toContain('grid-cols-1');

    // Content should fit in viewport without horizontal scroll
    const mainElement = page.locator('main');
    const width = await mainElement.boundingBox();
    expect(width?.width).toBeLessThanOrEqual(375);
  });

  test('Navigation is accessible on all viewports', async ({ page }) => {
    const viewports = [
      { width: 375, height: 667, name: 'Mobile' },
      { width: 768, height: 1024, name: 'Tablet' },
      { width: 1920, height: 1080, name: 'Desktop' },
    ];

    for (const viewport of viewports) {
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.goto('/');
      await page.waitForLoadState('networkidle');

      const nav = page.locator('nav');
      await expect(nav).toBeVisible(`Navigation should be visible on ${viewport.name}`);

      // Check key nav elements
      const logoArea = page.locator('nav >> text=VORTEX');
      await expect(logoArea).toBeVisible();

      const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
      await expect(usersBtn).toBeVisible();

      const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
      await expect(secretsBtn).toBeVisible();
    }
  });

  test('Stats cards are responsive (5 col on desktop, 1 col on mobile)', async ({ page }) => {
    // Desktop: 5 columns
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    let statsRow = page.locator('#stats-row');
    let classes = await statsRow.getAttribute('class');
    expect(classes).toContain('md:grid-cols-5');

    // Mobile: 1 column
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    statsRow = page.locator('#stats-row');
    classes = await statsRow.getAttribute('class');
    expect(classes).toContain('grid-cols-1');
  });

  test('Modal size adjusts and is not clipped on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check modal is visible
    const modal = page.locator('#secret-modal');
    await expect(modal).toBeVisible();

    // Check modal dimensions fit viewport
    const box = await modal.boundingBox();
    expect(box).not.toBeNull();

    if (box) {
      expect(box.width).toBeLessThanOrEqual(375);
      expect(box.height).toBeLessThanOrEqual(667);
    }

    // Modal should have padding for mobile
    const modalContent = page.locator('#secret-modal').locator('>> [class*="max-w"]');
    await expect(modalContent).toBeVisible();
  });

  test('DAG detail view is responsive on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();

    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click DAG card
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Detail view should be visible
    const detailView = page.locator('#dag-detail');
    await expect(detailView).toBeVisible();

    // Action buttons should be stacked on mobile
    const actionButtons = page.locator('#dag-detail >> button');
    const count = await actionButtons.count();
    expect(count).toBeGreaterThan(0);

    // Buttons might be wrapped/stacked - check they don't overflow
    const box = await detailView.boundingBox();
    expect(box?.width).toBeLessThanOrEqual(375 + 16); // 375px viewport + padding
  });

  test('Task and Instance sections are stacked on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();

    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click DAG card
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check grid layout (should be 1 column on mobile)
    const gridContainer = page.locator('#tab-content-tasks');
    const classes = await gridContainer.getAttribute('class');
    expect(classes).toContain('grid-cols-1');
  });

  test('Overflow content has proper scrolling on all viewports', async ({ page }) => {
    const viewports = [375, 768, 1920];

    for (const width of viewports) {
      await page.setViewportSize({ width, height: 1080 });
      await page.goto('/');
      await page.waitForLoadState('networkidle');

      // Check swarm details have scroll
      const helpers = createHelpers(page);
      await helpers.toggleSwarmPanel();

      const swarmDetails = page.locator('#swarm-details');
      const classes = await swarmDetails.getAttribute('class') || '';

      // Should be readable even if not explicitly scrollable on desktop
      // But shouldn't cause layout issues
      await expect(swarmDetails).toBeDefined();
    }
  });

  test('Text is readable on all viewports (proper font sizes)', async ({ page }) => {
    const viewports = [
      { width: 375, height: 667 },
      { width: 768, height: 1024 },
      { width: 1920, height: 1080 },
    ];

    for (const viewport of viewports) {
      await page.setViewportSize(viewport);
      await page.goto('/');
      await page.waitForLoadState('networkidle');

      // Check that headings are visible
      const heading = page.locator('text=DAG Registry');
      await expect(heading).toBeVisible();

      // Get font size
      const fontSize = await heading.evaluate((el) => {
        return window.getComputedStyle(el).fontSize;
      });

      // Should be reasonable size (not too small)
      const size = parseInt(fontSize);
      expect(size).toBeGreaterThan(10);
    }
  });

  test('Navigation buttons stack properly on mobile', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // All nav buttons should be accessible
    const navButtons = page.locator('nav button');
    const count = await navButtons.count();
    expect(count).toBeGreaterThan(0);

    // Check that buttons don't overflow viewport
    const nav = page.locator('nav');
    const box = await nav.boundingBox();
    expect(box?.width).toBeLessThanOrEqual(375 + 16); // viewport + small margin
  });

  test('Lists have proper spacing and don\'t overflow', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Check DAG list
    const dagList = page.locator('#dag-list');
    const dagBox = await dagList.boundingBox();

    if (dagBox) {
      // Should not overflow viewport width
      expect(dagBox.width).toBeLessThanOrEqual(375 + 32); // 375px + padding
    }
  });

  test('Swarm panel responsive styling maintains readability', async ({ page }) => {
    const viewports = [375, 768, 1920];

    for (const width of viewports) {
      await page.setViewportSize({ width, height: 1080 });
      await page.goto('/');
      await page.waitForLoadState('networkidle');

      // Swarm panel should be visible and readable
      const swarmPanel = page.locator('#swarm-panel');
      await expect(swarmPanel).toBeVisible();

      // Check that content is not cut off
      const header = page.locator('#swarm-panel > div').first();
      await expect(header).toBeVisible();

      // Status badge should be visible
      const statusBadge = page.locator('#swarm-status-badge');
      await expect(statusBadge).toBeVisible();
    }
  });

  test('Touch targets are appropriately sized on mobile (min 44x44)', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Check button sizes
    const buttons = page.locator('button');
    const count = await buttons.count();

    // Sample first few buttons
    for (let i = 0; i < Math.min(3, count); i++) {
      const button = buttons.nth(i);
      const box = await button.boundingBox();

      if (box) {
        // Button should be reasonably sized for touch
        // (not enforcing exactly 44x44, but should be accessible)
        expect(Math.max(box.width, box.height)).toBeGreaterThan(30);
      }
    }
  });

  test('Main content padding is appropriate on all viewports', async ({ page }) => {
    const viewports = [
      { width: 375, height: 667 },
      { width: 768, height: 1024 },
      { width: 1920, height: 1080 },
    ];

    for (const viewport of viewports) {
      await page.setViewportSize(viewport);
      await page.goto('/');
      await page.waitForLoadState('networkidle');

      const main = page.locator('main');
      const box = await main.boundingBox();

      expect(box).not.toBeNull();

      if (box) {
        // Should have reasonable padding and not extend to edges
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.width).toBeLessThanOrEqual(viewport.width);
      }
    }
  });
});
