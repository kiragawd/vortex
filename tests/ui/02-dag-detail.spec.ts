import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('02 - DAG List & Detail View', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Click DAG card opens detail view', async ({ page }) => {
    const helpers = createHelpers(page);

    // Get available DAGs
    const dags = await helpers.fetchDAGs();
    expect(dags.length).toBeGreaterThan(0);

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click on first DAG card
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Wait for detail view to appear
    const detailView = page.locator('#view-details');
    await expect(detailView).toBeVisible();
  });

  test('Detail view shows DAG title and ID', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check title is displayed
    const detailTitle = page.locator('#detail-title');
    await expect(detailTitle).toBeVisible();
    expect(await detailTitle.textContent()).not.toBe('');
  });

  test('Detail view shows action buttons: Edit, Retry, Pause, Backfill, Trigger', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check Edit Code button
    const editBtn = page.locator('#btn-edit');
    await expect(editBtn).toBeVisible();

    // Check Retry Failures button
    const retryBtn = page.locator('#btn-retry');
    await expect(retryBtn).toBeVisible();

    // Check Pause button
    const pauseBtn = page.locator('#btn-pause');
    await expect(pauseBtn).toBeVisible();

    // Check Backfill button
    const backfillBtn = page.locator('#btn-backfill');
    await expect(backfillBtn).toBeVisible();

    // Check Trigger button
    const triggerBtn = page.locator('#btn-trigger');
    await expect(triggerBtn).toBeVisible();
    const triggerText = await triggerBtn.textContent();
    expect(triggerText).toMatch(/TRIGGER\s+RUN/i);
  });

  test('Tabs exist: "Visual Graph", "Run History", and "Timeline"', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check Visual Graph tab
    const graphTab = page.locator('#tab-graph');
    await expect(graphTab).toBeVisible();
    expect(await graphTab.textContent()).toContain('Visual Graph');

    // Check Run History tab
    const historyTab = page.locator('#tab-history');
    await expect(historyTab).toBeVisible();
    expect(await historyTab.textContent()).toContain('Run History');

    // Check Timeline tab
    const timelineTab = page.locator('#tab-timeline');
    await expect(timelineTab).toBeVisible();
    expect(await timelineTab.textContent()).toContain('Timeline');
  });

  test('Back button closes detail view and returns to list', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Verify detail view is visible
    await expect(page.locator('#view-details')).toBeVisible();

    // Click back button
    const backBtn = page.locator('#view-details button[onclick="showView(\'registry\')"]').first();
    await backBtn.click();

    // Detail should be hidden
    const detailView = page.locator('#view-details');
    expect(await detailView.isHidden()).toBeTruthy();

    // DAG list should be visible again
    const dagContainer = page.locator('#view-registry');
    await expect(dagContainer).toBeVisible();
  });

  test('Pause/Unpause toggle changes button text', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    const pauseBtn = page.locator('#btn-pause');
    const initialText = await pauseBtn.textContent();

    // Click pause button
    await pauseBtn.click();

    // Wait for state change
    await page.waitForTimeout(500);

    // Text should change (Pause -> Resume or vice versa)
    const newText = await pauseBtn.textContent();
    expect(newText).not.toBe(initialText);
  });

  test('Backfill modal opens on "BACKFILL" button click', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Click backfill button
    const backfillBtn = page.locator('#btn-backfill');
    await backfillBtn.click();

    // Check if modal appears
    const backfillModal = page.locator('#backfill-modal');
    await expect(backfillModal).toBeVisible();
  });

  test('Trigger Run button is prominently visible', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check trigger button
    const triggerBtn = page.locator('#btn-trigger');
    await expect(triggerBtn).toBeVisible();

    // Should be styled with vortex-gradient (indicates prominence)
    const classes = await triggerBtn.getAttribute('class');
    expect(classes).toContain('vortex-gradient');
  });

  test('Switching tabs shows different content', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Tasks tab should be active by default
    const graphContent = page.locator('#detail-tab-graph');
    const historyContent = page.locator('#detail-tab-history');

    // Graph content should be visible
    const graphVisible = await graphContent.isVisible();
    expect(graphVisible).toBeTruthy();

    // Click History tab
    const historyTab = page.locator('#tab-history');
    await historyTab.click();

    // Wait for content switch
    await page.waitForTimeout(300);

    // History content should now be visible
    const historyVisible = await historyContent.isVisible();
    expect(historyVisible).toBeTruthy();
  });

  test('Task list is populated with tasks', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check SVG container graph
    const svgList = page.locator('#dag-svg');
    await expect(svgList).toBeVisible();
  });
});
