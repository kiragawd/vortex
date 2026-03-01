import { Page, expect } from '@playwright/test';

/**
 * VORTEX Test Helpers
 * Utility functions for API calls, auth, and common operations
 */

export class VortexHelpers {
  private page: Page;
  private apiKey: string = 'vortex_admin_key';

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
    const response = await this.page.request.fetch(path, {
      method,
      headers: {
        'Authorization': this.apiKey,
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
    this.apiKey = 'vortex_admin_key';
    // Verify admin access
    const response = await this.page.request.fetch('/api/dags', {
      headers: {
        'Authorization': this.apiKey,
      },
    });
    expect(response.ok()).toBeTruthy();
  }

  /**
   * Create a test DAG via API
   */
  async createTestDAG(dagId: string = `test_dag_${Date.now()}`): Promise<string> {
    const payload = {
      dag_id: dagId,
      description: `Test DAG - ${new Date().toISOString()}`,
      owner: 'test_user',
      schedule_interval: '@daily',
      is_paused: false,
      tags: ['test'],
    };

    // Note: This assumes a POST /api/dags endpoint exists
    // If not, the test will fail gracefully
    try {
      await this.api('/api/dags', 'POST', payload);
    } catch (e) {
      console.log('Note: POST /api/dags not implemented, using existing DAGs for tests');
    }

    return dagId;
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
    await this.api(`/api/secrets/${key}`, 'DELETE');
  }

  /**
   * Create a test user
   */
  async createTestUser(
    username: string,
    password: string,
    role: string = 'Operator'
  ): Promise<void> {
    await this.api('/api/users', 'POST', {
      username,
      password: password,
      role,
    });
  }

  /**
   * Delete a test user
   */
  async deleteTestUser(username: string): Promise<void> {
    await this.api(`/api/users/${username}`, 'DELETE');
  }

  /**
   * Fetch all DAGs
   */
  async fetchDAGs(): Promise<Array<Record<string, unknown>>> {
    return (await this.api('/api/dags')) as Array<Record<string, unknown>>;
  }

  /**
   * Fetch a specific DAG's tasks
   */
  async fetchDAGTasks(dagId: string): Promise<Record<string, unknown>> {
    return (await this.api(`/api/dags/${dagId}/tasks`)) as Record<string, unknown>;
  }

  /**
   * Fetch all secrets
   */
  async fetchSecrets(): Promise<Record<string, unknown>> {
    return (await this.api('/api/secrets')) as Record<string, unknown>;
  }

  /**
   * Fetch all users
   */
  async fetchUsers(): Promise<Array<Record<string, unknown>>> {
    return (await this.api('/api/users')) as Array<Record<string, unknown>>;
  }

  /**
   * Wait for API call to complete
   */
  async waitForApiCall(path: string, method: string = 'GET'): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(new Error(`API call timeout: ${method} ${path}`)),
        5000
      );

      this.page.on('response', (response) => {
        if (response.url().includes(path) && response.request().method() === method) {
          clearTimeout(timeout);
          response.json().then(resolve).catch(() => resolve(null));
        }
      });
    });
  }

  /**
   * Wait for a specific element with text
   */
  async waitForElementWithText(selector: string, text: string): Promise<void> {
    await this.page.locator(`${selector}:has-text("${text}")`).waitFor({ timeout: 5000 });
  }

  /**
   * Get all visible DAG cards from list
   */
  async getVisibleDAGCards(): Promise<number> {
    return await this.page.locator('#dag-list > div').count();
  }

  /**
   * Click on a DAG card by ID
   */
  async clickDAGCard(dagId: string): Promise<void> {
    await this.page.locator(`#dag-list > div:has-text("${dagId}")`).click();
  }

  /**
   * Check if detail view is visible
   */
  async isDetailViewVisible(): Promise<boolean> {
    const detailDiv = this.page.locator('#view-details');
    return await detailDiv.isVisible();
  }

  /**
   * Check if secrets section is visible
   */
  async isSecretsSectionVisible(): Promise<boolean> {
    return await this.page.locator('#view-secrets').isVisible();
  }

  /**
   * Check if users section is visible
   */
  async isUsersSectionVisible(): Promise<boolean> {
    return await this.page.locator('#view-users').isVisible();
  }

  /**
   * Get current pause button text
   */
  async getPauseButtonText(): Promise<string> {
    return await this.page.locator('#btn-pause').textContent() || '';
  }

  /**
   * Get swarm status text
   */
  async getSwarmStatus(): Promise<string> {
    return await this.page.locator('#swarm-status-badge').textContent() || '';
  }

  /**
   * Toggle swarm panel
   */
  async toggleSwarmPanel(): Promise<void> {
    await this.page.click('[id="swarm-panel"] > div');
  }

  /**
   * Check if swarm details are visible
   */
  async areSwarmDetailsVisible(): Promise<boolean> {
    const details = this.page.locator('#swarm-details');
    return await details.isVisible();
  }

  /**
   * Get visible element count by selector
   */
  async getElementCount(selector: string): Promise<number> {
    return await this.page.locator(selector).count();
  }

  /**
   * Wait for loading to complete (look for spinners/placeholders)
   */
  async waitForLoadingComplete(): Promise<void> {
    await this.page.waitForLoadState('networkidle');
    // Also wait for any animate-pulse to finish
    await this.page.locator('.animate-pulse').first().waitFor({ state: 'hidden', timeout: 5000 }).catch(() => {
      // Some pages might not have pulse elements
    });
  }
}

/**
 * Export helper factory
 */
export function createHelpers(page: Page): VortexHelpers {
  return new VortexHelpers(page);
}
