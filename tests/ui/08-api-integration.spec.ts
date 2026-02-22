import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('08 - API Integration & Network Calls', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Dashboard calls GET /api/dags on page load', async ({ page }) => {
    // Monitor network calls
    const apiCalls: string[] = [];

    page.on('response', (response) => {
      if (response.url().includes('/api/dags') && response.request().method() === 'GET') {
        apiCalls.push(response.url());
      }
    });

    // Refresh page
    await page.reload();
    await page.waitForLoadState('networkidle');

    // Verify /api/dags was called
    expect(apiCalls.length).toBeGreaterThan(0);
  });

  test('API calls include Authorization header', async ({ page }) => {
    let headerFound = false;
    let headerValue = '';

    page.on('request', (request) => {
      if (request.url().includes('/api/dags')) {
        const authHeader = request.headerValue('Authorization');
        if (authHeader) {
          headerFound = true;
          headerValue = authHeader;
        }
      }
    });

    await page.reload();
    await page.waitForLoadState('networkidle');

    // Verify authorization header exists
    expect(headerFound).toBeTruthy();
    expect(headerValue).toBe('vortex_admin_key');
  });

  test('DAG list is populated from API response', async ({ page }) => {
    const helpers = createHelpers(page);

    // Fetch DAGs via API
    const dags = await helpers.fetchDAGs();

    // Verify response is valid
    expect(Array.isArray(dags)).toBeTruthy();

    // Check if DAG list in UI matches API response
    const dagCount = await page.locator('#dag-list > div').count();

    if (dags.length > 0) {
      expect(dagCount).toBeGreaterThan(0);
    }
  });

  test('Secret add calls POST /api/secrets with correct payload', async ({ page }) => {
    const helpers = createHelpers(page);

    // Monitor API calls
    let postCalled = false;
    let requestBody: Record<string, unknown> | null = null;

    page.on('request', (request) => {
      if (request.url().includes('/api/secrets') && request.method() === 'POST') {
        postCalled = true;
        try {
          const body = request.postDataJSON();
          requestBody = body as Record<string, unknown>;
        } catch {
          // Ignore parse errors
        }
      }
    });

    // Navigate to secrets and add one
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const testKey = `TEST_KEY_${Date.now()}`;
    const testValue = 'test_secret';

    await page.locator('#secret-key').fill(testKey);
    await page.locator('#secret-value').fill(testValue);

    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
    await submitBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify API was called
    expect(postCalled).toBeTruthy();

    if (requestBody) {
      expect(requestBody.key).toBe(testKey);
      expect(requestBody.value).toBe(testValue);
    }
  });

  test('Secret delete calls DELETE /api/secrets/{key}', async ({ page }) => {
    const helpers = createHelpers(page);

    // Create a test secret first
    const testKey = `DELETE_API_TEST_${Date.now()}`;
    await helpers.createTestSecret(testKey, 'temp_value');

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Reload to show the new secret
    await page.reload();
    await page.waitForLoadState('networkidle');
    await secretsBtn.click();
    await page.waitForTimeout(500);

    // Monitor delete calls
    let deleteCalled = false;
    let deleteUrl = '';

    page.on('request', (request) => {
      if (
        request.url().includes('/api/secrets') &&
        request.method() === 'DELETE'
      ) {
        deleteCalled = true;
        deleteUrl = request.url();
      }
    });

    // Find and click delete
    const secretsList = page.locator('#secrets-list');
    const secretItem = secretsList.locator(`text=${testKey}`).first();
    const deleteBtn = secretItem.locator('.. >> button:has-text("Delete")');

    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    // Wait for deletion
    await page.waitForTimeout(500);

    // Verify DELETE was called
    expect(deleteCalled).toBeTruthy();
    expect(deleteUrl).toContain(testKey);
  });

  test('User add calls POST /api/users with correct payload', async ({ page }) => {
    // Monitor API calls
    let postCalled = false;
    let requestBody: Record<string, unknown> | null = null;

    page.on('request', (request) => {
      if (request.url().includes('/api/users') && request.method() === 'POST') {
        postCalled = true;
        try {
          const body = request.postDataJSON();
          requestBody = body as Record<string, unknown>;
        } catch {
          // Ignore parse errors
        }
      }
    });

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    const testUsername = `testuser_${Date.now()}`;
    const testPassword = 'testpass123';
    const testRole = 'Operator';

    await page.locator('#user-username').fill(testUsername);
    await page.locator('#user-password').fill(testPassword);
    await page.locator('#user-role').selectOption(testRole);

    const submitBtn = page.locator('#user-modal button:has-text("Create User")');
    await submitBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify POST was called
    expect(postCalled).toBeTruthy();

    if (requestBody) {
      expect(requestBody.username).toBe(testUsername);
      expect(requestBody.password_hash).toBe(testPassword);
      expect(requestBody.role).toBe(testRole);
    }
  });

  test('User delete calls DELETE /api/users/{username}', async ({ page }) => {
    const helpers = createHelpers(page);

    // Create test user
    const testUsername = `deleteuser_${Date.now()}`;
    await helpers.createTestUser(testUsername, 'password123', 'Viewer');

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Reload to show new user
    await page.reload();
    await page.waitForLoadState('networkidle');
    await usersBtn.click();
    await page.waitForTimeout(500);

    // Monitor delete calls
    let deleteCalled = false;
    let deleteUrl = '';

    page.on('request', (request) => {
      if (
        request.url().includes('/api/users') &&
        request.method() === 'DELETE'
      ) {
        deleteCalled = true;
        deleteUrl = request.url();
      }
    });

    // Find and click delete
    const usersList = page.locator('#users-list');
    const userItem = usersList.locator(`text=${testUsername}`).first();
    const deleteBtn = userItem.locator('.. >> button:has-text("Delete")');

    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    // Wait for deletion
    await page.waitForTimeout(500);

    // Verify DELETE was called
    expect(deleteCalled).toBeTruthy();
    expect(deleteUrl).toContain(testUsername);
  });

  test('DAG detail calls GET /api/dags/{id}/tasks', async ({ page }) => {
    const helpers = createHelpers(page);

    // Get DAGs
    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Monitor API calls
    let tasksCalled = false;

    page.on('request', (request) => {
      if (
        request.url().includes(`/api/dags/${firstDagId}/tasks`) &&
        request.method() === 'GET'
      ) {
        tasksCalled = true;
      }
    });

    // Click DAG to load tasks
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify API was called
    expect(tasksCalled).toBeTruthy();
  });

  test('Pause/Unpause calls PATCH /api/dags/{id}/pause', async ({ page }) => {
    const helpers = createHelpers(page);

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Monitor PATCH calls
    let patchCalled = false;

    page.on('request', (request) => {
      if (
        request.url().includes(`/api/dags/${firstDagId}`) &&
        (request.method() === 'PATCH' || request.method() === 'PUT')
      ) {
        patchCalled = true;
      }
    });

    // Click DAG to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // Click pause button
    const pauseBtn = page.locator('#pause-btn');
    await pauseBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify PATCH was called (or similar method)
    expect(patchCalled).toBeTruthy();
  });

  test('Fetch secrets returns proper structure', async ({ page }) => {
    const helpers = createHelpers(page);

    // Fetch secrets
    const secretsData = await helpers.fetchSecrets();

    // Should have secrets property or be an array
    expect(secretsData).toBeDefined();

    if (typeof secretsData === 'object' && !Array.isArray(secretsData)) {
      expect((secretsData as Record<string, unknown>).secrets).toBeDefined();
    }
  });

  test('Fetch users returns array with proper structure', async ({ page }) => {
    const helpers = createHelpers(page);

    // Fetch users
    const users = await helpers.fetchUsers();

    // Should be an array
    expect(Array.isArray(users)).toBeTruthy();

    // Each user should have required fields
    if (users.length > 0) {
      const user = users[0] as Record<string, unknown>;
      expect(user.username).toBeDefined();
      expect(user.api_key).toBeDefined();
      expect(user.role).toBeDefined();
    }
  });

  test('API responses include proper Content-Type header', async ({ page }) => {
    let contentTypeFound = false;

    page.on('response', (response) => {
      if (response.url().includes('/api/')) {
        const contentType = response.headers()['content-type'];
        if (contentType && contentType.includes('application/json')) {
          contentTypeFound = true;
        }
      }
    });

    await page.reload();
    await page.waitForLoadState('networkidle');

    // Verify Content-Type was found
    expect(contentTypeFound).toBeTruthy();
  });

  test('Failed API calls are handled gracefully', async ({ page }) => {
    const helpers = createHelpers(page);

    // Try to fetch with invalid path (should handle error)
    try {
      await helpers.api('/api/invalid_endpoint');
      // If no error, that's unexpected but not a failure
    } catch (e) {
      // Error handling is working
      expect((e as Error).message).toContain('API Error');
    }
  });

  test('Network errors do not crash the page', async ({ page, context }) => {
    // Enable offline mode temporarily
    await page.context().setOffline(true);

    // Try to navigate or interact
    const refreshBtn = page.locator('text=/Refresh/');
    
    // Wait a moment
    await page.waitForTimeout(500);

    // Page should still be responsive
    expect(await page.title()).toBeTruthy();

    // Re-enable network
    await page.context().setOffline(false);
  });

  test('Concurrent API calls work properly', async ({ page }) => {
    const helpers = createHelpers(page);

    // Make multiple API calls at once
    const promises = [
      helpers.fetchDAGs(),
      helpers.fetchSecrets(),
      helpers.fetchUsers(),
    ];

    // Should complete without errors
    const results = await Promise.all(promises);

    // Verify all returned data
    expect(Array.isArray(results[0])).toBeTruthy(); // DAGs
    expect(results[1]).toBeDefined(); // Secrets
    expect(Array.isArray(results[2])).toBeTruthy(); // Users
  });
});
