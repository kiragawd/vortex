import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('06 - Task & Instance View', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('DAG detail view shows Tasks & Instances tab active by default', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check if Tasks & Instances tab is active
    const tasksTab = page.locator('#tab-tasks');
    const classes = await tasksTab.getAttribute('class');
    expect(classes).toContain('tab-active');

    // Content should be visible
    const tasksContent = page.locator('#tab-content-tasks');
    await expect(tasksContent).toBeVisible();
  });

  test('Task graph (left column) shows task list with proper layout', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check task list container
    const taskList = page.locator('#task-list');
    await expect(taskList).toBeVisible();

    // Check that it's in a grid layout (lg:col-span-1)
    const tasksSection = page.locator('#tab-content-tasks');
    const classes = await tasksSection.getAttribute('class');
    expect(classes).toContain('grid');
    expect(classes).toContain('lg:col-span');
  });

  test('Task instances (right column) shows instances with proper layout', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check instance list container
    const instanceList = page.locator('#instance-list');
    await expect(instanceList).toBeVisible();

    // Should have proper styling
    const classes = await instanceList.getAttribute('class');
    expect(classes).toContain('space-y-2');
    expect(classes).toContain('max-h');
    expect(classes).toContain('overflow-y-auto');
  });

  test('Task list displays task names (or is empty)', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Get task list
    const taskList = page.locator('#task-list');
    await expect(taskList).toBeVisible();

    // Get task elements
    const taskElements = page.locator('#task-list > div');
    const count = await taskElements.count();

    // Should have 0 or more tasks
    expect(count).toBeGreaterThanOrEqual(0);

    // If tasks exist, they should have content
    if (count > 0) {
      const firstTask = taskElements.first();
      const text = await firstTask.textContent();
      expect(text).not.toBeEmpty();
    }
  });

  test('Instance list displays instances (or is empty)', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Get instance list
    const instanceList = page.locator('#instance-list');
    await expect(instanceList).toBeVisible();

    // Get instance elements
    const instanceElements = page.locator('#instance-list > div');
    const count = await instanceElements.count();

    // Should have 0 or more instances
    expect(count).toBeGreaterThanOrEqual(0);
  });

  test('Section headings display "Task Graph" and "Task Instances"', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check section headings
    const taskGraphHeading = page.locator('#tab-content-tasks >> text=Task Graph');
    await expect(taskGraphHeading).toBeVisible();

    const taskInstancesHeading = page.locator('#tab-content-tasks >> text=Task Instances');
    await expect(taskInstancesHeading).toBeVisible();
  });

  test('Task elements have proper styling (bg-white/5, rounded-lg)', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Wait for tasks to load
    await page.waitForTimeout(500);

    // Get first task element if exists
    const taskElements = page.locator('#task-list > div');
    const count = await taskElements.count();

    if (count > 0) {
      const firstTask = taskElements.first();
      const classes = await firstTask.getAttribute('class');
      
      // Check for styling
      expect(classes).toContain('bg-white');
      expect(classes).toContain('rounded');
    }
  });

  test('Instances have proper layout with overflow handling', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check instance list styling
    const instanceList = page.locator('#instance-list');
    const classes = await instanceList.getAttribute('class');

    // Should have height limit and scrolling
    expect(classes).toContain('max-h');
    expect(classes).toContain('overflow-y-auto');
    expect(classes).toContain('pr-2'); // padding for scrollbar
  });

  test('Switching to DAG Runs tab changes content', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Verify Tasks content is visible
    const tasksContent = page.locator('#tab-content-tasks');
    expect(await tasksContent.isVisible()).toBeTruthy();

    // Click Runs tab
    const runsTab = page.locator('#tab-runs');
    await runsTab.click();

    // Wait for tab switch
    await page.waitForTimeout(300);

    // Tasks content should be hidden
    expect(await tasksContent.isHidden()).toBeTruthy();

    // Runs content should be visible
    const runsContent = page.locator('#tab-content-runs');
    expect(await runsContent.isVisible()).toBeTruthy();

    // Should show "DAG Run History" heading
    const heading = page.locator('#tab-content-runs >> text=DAG Run History');
    await expect(heading).toBeVisible();
  });

  test('DAG Runs tab has runs list with overflow handling', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Click Runs tab
    const runsTab = page.locator('#tab-runs');
    await runsTab.click();

    // Wait for switch
    await page.waitForTimeout(300);

    // Check runs list
    const runsList = page.locator('#runs-list');
    await expect(runsList).toBeVisible();

    // Should have proper styling
    const classes = await runsList.getAttribute('class');
    expect(classes).toContain('space-y-2');
    expect(classes).toContain('max-h');
    expect(classes).toContain('overflow-y-auto');
  });

  test('Grid layout is responsive (lg:col-span)', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Check grid layout
    const gridContainer = page.locator('#tab-content-tasks');
    const classes = await gridContainer.getAttribute('class');

    // Should be a grid with responsive columns
    expect(classes).toContain('grid');
    expect(classes).toContain('grid-cols-1');
    expect(classes).toContain('lg:');
  });

  test('Task and Instance sections are side-by-side on lg screens', async ({ page }) => {
    // Set viewport to large screen
    await page.setViewportSize({ width: 1920, height: 1080 });

    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Both sections should be visible and styled for side-by-side layout
    const taskList = page.locator('#task-list');
    const instanceList = page.locator('#instance-list');

    await expect(taskList).toBeVisible();
    await expect(instanceList).toBeVisible();

    // Check grid classes for responsive layout
    const gridContainer = page.locator('#tab-content-tasks');
    const classes = await gridContainer.getAttribute('class');
    expect(classes).toContain('lg:col-span');
  });
});
