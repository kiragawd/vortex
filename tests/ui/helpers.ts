import { Page, expect } from '@playwright/test';

/**
 * RYUO Test Helpers — React UI
 * Utility functions for the new React/TypeScript/Vite UI at localhost:3000
 */

export class RyuoHelpers {
  private page: Page;
  private apiKey: string = 'ryuo_admin_key';

  constructor(page: Page) {
    this.page = page;
  }

  /**
   * Make API call with authorization header
   */
  async api(
    path: string,
    method: string = 'GET',
    body?: Record<string, unknown>
  ): Promise<unknown> {
    const response = await this.page.request.fetch(`http://localhost:3000${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${this.apiKey}`,
        'Content-Type': 'application/json',
      },
      data: body ? JSON.stringify(body) : undefined,
    });

    if (!response.ok()) {
      throw new Error(`API Error: ${response.status()} ${response.statusText()}`);
    }

    const contentType = response.headers()['content-type'];
    if (contentType && contentType.includes('application/json')) {
      return await response.json();
    }
    return null;
  }

  /**
   * Login as admin user (set auth header)
   */
  async loginAsAdmin(): Promise<void> {
    this.apiKey = 'ryuo_admin_key';
    const response = await this.page.request.fetch('http://localhost:3000/api/dags', {
      headers: {
        Authorization: `Bearer ${this.apiKey}`,
      },
    });
    expect(response.ok()).toBeTruthy();
  }

  /**
   * Navigate to a page via sidebar link
   */
  async navigateTo(href: string): Promise<void> {
    await this.page.locator(`a[href="${href}"]`).click();
    await this.page.waitForURL(`**${href}`, { timeout: 10000 });
  }

  /**
   * Navigate to a page and wait for heading text
   */
  async navigateAndExpectHeading(href: string, heading: string | RegExp): Promise<void> {
    await this.navigateTo(href);
    await expect(this.page.locator('h1')).toContainText(heading, { timeout: 10000 });
  }

  /**
   * Fetch all DAGs from paginated API
   */
  async fetchDAGs(): Promise<Array<Record<string, unknown>>> {
    const res = (await this.api('/api/dags')) as { data: Array<Record<string, unknown>> };
    return res.data ?? [];
  }

  /**
   * Fetch a specific DAG's tasks
   */
  async fetchDAGTasks(dagId: string): Promise<Record<string, unknown>> {
    return (await this.api(`/api/dags/${encodeURIComponent(dagId)}/tasks`)) as Record<string, unknown>;
  }

  /**
   * Fetch all secrets
   */
  async fetchSecrets(): Promise<Record<string, unknown>> {
    return (await this.api('/api/secrets')) as Record<string, unknown>;
  }

  /**
   * Create a test secret
   */
  async createTestSecret(key: string, value: string): Promise<void> {
    await this.api('/api/secrets', 'POST', { key, value });
  }

  /**
   * Delete a test secret
   */
  async deleteTestSecret(key: string): Promise<void> {
    await this.api(`/api/secrets/${encodeURIComponent(key)}`, 'DELETE');
  }

  /**
   * Click the first DAG row in the table and wait for detail page
   */
  async clickFirstDagRow(): Promise<void> {
    const dagRows = this.page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await this.page.waitForURL(/\/dags\/.+/);
  }

  /**
   * Wait for loading to complete (spinners gone, network idle)
   */
  async waitForLoadingComplete(): Promise<void> {
    await this.page.waitForLoadState('networkidle');
    // Wait for any spinners to disappear
    await this.page
      .locator('.animate-spin')
      .first()
      .waitFor({ state: 'hidden', timeout: 10000 })
      .catch(() => {
        // Some pages might not have spinners
      });
  }

  /**
   * Get count of table rows in the main content area
   */
  async getTableRowCount(): Promise<number> {
    return await this.page.locator('table tbody tr').count();
  }

  /**
   * Check if sidebar is visible
   */
  async isSidebarVisible(): Promise<boolean> {
    return await this.page.locator('aside').isVisible();
  }

  /**
   * Toggle the sidebar via header button
   */
  async toggleSidebar(): Promise<void> {
    await this.page.locator('button[aria-label="Toggle sidebar"]').click();
  }

  /**
   * Toggle dark mode via header button
   */
  async toggleTheme(): Promise<void> {
    await this.page.locator('button[aria-label="Toggle theme"]').click();
  }

  /**
   * Check if dark mode is active
   */
  async isDarkMode(): Promise<boolean> {
    return await this.page.locator('html.dark').count() > 0;
  }
}

/**
 * Export helper factory
 */
export function createHelpers(page: Page): RyuoHelpers {
  return new RyuoHelpers(page);
}
