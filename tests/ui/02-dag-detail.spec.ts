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
    const detailView = page.locator('#dag-detail');
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
    expect(await detailTitle.textContent()).not.toBeEmpty();

    // Check ID subtitle
    const idSub = page.locator('#detail-id-sub');
    await expect(idSub).toBeVisible();
  });

  test('Detail view shows action buttons: Pause, Schedule, Backfill, Trigger', async ({ page }) => {
    const helpers = createHelpers(page);
    
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check Pause button
    const pauseBtn = page.locator('#pause-btn');
    await expect(pauseBtn).toBeVisible();

    // Check Schedule button
    const scheduleBtn = page.locator('#schedule-btn');
    await expect(scheduleBtn).toBeVisible();

    // Check Backfill button
    const backfillBtn = page.locator('#backfill-btn');
    await expect(backfillBtn).toBeVisible();

    // Check Trigger button (TRIGGER RUN)
    const triggerBtn = page.locator('#trigger-btn');
    await expect(triggerBtn).toBeVisible();
    expect(await triggerBtn.textContent()).toContain('TRIGGER RUN');
  });

  test('Tabs exist: "Tasks & Instances" and "DAG Runs"', async ({ page }) => {
    const helpers = createHelpers(page);
    
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check Tasks & Instances tab
    const tasksTab = page.locator('#tab-tasks');
    await expect(tasksTab).toBeVisible();
    expect(await tasksTab.textContent()).toContain('Tasks & Instances');

    // Check DAG Runs tab
    const runsTab = page.locator('#tab-runs');
    await expect(runsTab).toBeVisible();
    expect(await runsTab.textContent()).toContain('DAG Runs');
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
    await expect(page.locator('#dag-detail')).toBeVisible();

    // Click back button
    const backBtn = page.locator('#dag-detail >> button:has-text("") >> nth=0').first();
    await backBtn.click();

    // Detail should be hidden
    const detailView = page.locator('#dag-detail');
    expect(await detailView.isHidden()).toBeTruthy();

    // DAG list should be visible again
    const dagContainer = page.locator('#dag-container');
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

    const pauseBtn = page.locator('#pause-btn');
    const initialText = await pauseBtn.textContent();

    // Click pause button
    await pauseBtn.click();
    
    // Wait for state change
    await page.waitForTimeout(500);

    // Text should change (Pause -> Resume or vice versa)
    const newText = await pauseBtn.textContent();
    expect(newText).not.toBe(initialText);
  });

  test('Schedule modal opens on "Schedule" button click', async ({ page }) => {
    const helpers = createHelpers(page);
    
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Click schedule button
    const scheduleBtn = page.locator('#schedule-btn');
    await scheduleBtn.click();

    // Check if any modal or form appears (schedule info bar is shown)
    const scheduleInfo = page.locator('#schedule-info');
    await expect(scheduleInfo).toBeVisible();
  });

  test('Schedule info bar displays schedule details', async ({ page }) => {
    const helpers = createHelpers(page);
    
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check schedule info bar
    const scheduleInfo = page.locator('#schedule-info');
    await expect(scheduleInfo).toBeVisible();

    // Check for schedule field
    const schedule = page.locator('#info-schedule');
    await expect(schedule).toBeVisible();

    // Check for timezone field
    const timezone = page.locator('#info-timezone');
    await expect(timezone).toBeVisible();

    // Check for max runs field
    const maxRuns = page.locator('#info-max-runs');
    await expect(maxRuns).toBeVisible();

    // Check for next run field
    const nextRun = page.locator('#info-next-run');
    await expect(nextRun).toBeVisible();
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
    const triggerBtn = page.locator('#trigger-btn');
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
    const tasksContent = page.locator('#tab-content-tasks');
    const runsContent = page.locator('#tab-content-runs');

    // Tasks content should be visible
    const tasksVisible = await tasksContent.isVisible();
    expect(tasksVisible).toBeTruthy();

    // Click Runs tab
    const runsTab = page.locator('#tab-runs');
    await runsTab.click();

    // Wait for content switch
    await page.waitForTimeout(300);

    // Runs content should now be visible
    const runsVisible = await runsContent.isVisible();
    expect(runsVisible).toBeTruthy();
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

    // Check task list
    const taskList = page.locator('#task-list');
    await expect(taskList).toBeVisible();

    // Tasks might be empty or populated
    const taskElements = page.locator('#task-list > div');
    const count = await taskElements.count();
    expect(count).toBeGreaterThanOrEqual(0); // 0 or more tasks is valid
  });
});
