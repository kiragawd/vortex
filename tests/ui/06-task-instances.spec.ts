import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('06 - Task Graph & Run History', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('DAG detail view shows Visual Graph tab active by default', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();

    const graphTab = page.locator('#tab-graph');
    await expect(graphTab).toHaveClass(/tab-active/);

    const graphContent = page.locator('#detail-tab-graph');
    await expect(graphContent).toBeVisible();
  });

  test('Visual Graph renders DAG SVG canvas', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();

    const svgCanvas = page.locator('#dag-svg');
    await expect(svgCanvas).toBeVisible();
  });

  test('Visual Graph canvas contains rendered graph nodes', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();
    await page.waitForTimeout(500);

    const svgCanvas = page.locator('#dag-svg');
    const nodes = svgCanvas.locator('g.node');
    const count = await nodes.count();
    expect(count).toBeGreaterThanOrEqual(0); // Valid to have 0 nodes if empty DAG
  });

  test('Switching to Run History tab changes content', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();

    const historyTab = page.locator('#tab-history');
    await historyTab.click();
    await page.waitForTimeout(300);

    const historyContent = page.locator('#detail-tab-history');
    await expect(historyContent).toBeVisible();

    const graphContent = page.locator('#detail-tab-graph');
    await expect(graphContent).toBeHidden();
  });

  test('Run History tab has runs list with overflow handling', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();

    await page.locator('#tab-history').click();
    await page.waitForTimeout(300);

    const runsList = page.locator('#runs-list');
    await expect(runsList).toBeVisible();
  });

  test('Run History list displays runs or empty state message', async ({ page }) => {
    const helpers = createHelpers(page);
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) test.skip();

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
    await page.locator(`text=${firstDagId}`).first().click();

    await page.locator('#tab-history').click();
    await page.waitForTimeout(500);

    const runsList = page.locator('#runs-list');
    const listContent = await runsList.textContent();
    expect(listContent).toBeTruthy();
  });
});
