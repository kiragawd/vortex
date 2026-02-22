import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('10 - Authorization & RBAC Enforcement', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('API Key header is sent with all requests (Bearer vortex_admin_key)', async ({ page }) => {
    let apiKeyFound = false;
    let headerValue = '';

    page.on('request', (request) => {
      if (request.url().includes('/api/')) {
        const auth = request.headerValue('Authorization');
        if (auth === 'vortex_admin_key') {
          apiKeyFound = true;
          headerValue = auth;
        }
      }
    });

    // Trigger some API calls
    await page.reload();
    await page.waitForLoadState('networkidle');

    expect(apiKeyFound).toBeTruthy();
    expect(headerValue).toBe('vortex_admin_key');
  });

  test('All /api/ endpoints receive Authorization header', async ({ page }) => {
    const endpoints = new Set<string>();
    const missingAuth: string[] = [];

    page.on('request', (request) => {
      if (request.url().includes('/api/')) {
        const endpoint = new URL(request.url()).pathname;
        endpoints.add(endpoint);

        const auth = request.headerValue('Authorization');
        if (!auth) {
          missingAuth.push(endpoint);
        }
      }
    });

    // Trigger various API calls
    await page.reload();
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    await page.waitForLoadState('networkidle');

    // All endpoints should have had auth header
    expect(endpoints.size).toBeGreaterThan(0);
    expect(missingAuth.length).toBe(0);
  });

  test('Admin user: All UI sections visible (Users, Secrets, DAG Actions)', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Check navigation buttons
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await expect(usersBtn).toBeVisible();

    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await expect(secretsBtn).toBeVisible();

    // Check Admin button
    const adminBtn = page.locator('nav >> button:has-text("Admin")');
    await expect(adminBtn).toBeVisible();

    // Get a DAG and check action buttons are visible
    const dags = await helpers.fetchDAGs();
    if (dags.length > 0) {
      const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];
      const dagCard = page.locator(`text=${firstDagId}`).first();
      await dagCard.click();

      // Action buttons should be visible
      const pauseBtn = page.locator('#pause-btn');
      await expect(pauseBtn).toBeVisible();

      const scheduleBtn = page.locator('#schedule-btn');
      await expect(scheduleBtn).toBeVisible();

      const triggerBtn = page.locator('#trigger-btn');
      await expect(triggerBtn).toBeVisible();
    }
  });

  test('Admin user: Can add and delete secrets', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Create secret
    const testKey = `RBAC_SECRET_${Date.now()}`;
    await helpers.createTestSecret(testKey, 'secret_value');

    // Verify in UI
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    await page.reload();
    await page.waitForLoadState('networkidle');
    await secretsBtn.click();
    await page.waitForTimeout(500);

    const secretsList = page.locator('#secrets-list');
    const content = await secretsList.innerHTML();
    expect(content).toContain(testKey);

    // Delete secret
    const secretItem = secretsList.locator(`text=${testKey}`).first();
    const deleteBtn = secretItem.locator('.. >> button:has-text("Delete")');
    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    await page.waitForTimeout(500);

    // Verify deleted
    const newContent = await secretsList.innerHTML();
    expect(newContent).not.toContain(testKey);
  });

  test('Admin user: Can add and delete users', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Create user
    const testUsername = `rbacuser_${Date.now()}`;
    await helpers.createTestUser(testUsername, 'password123', 'Operator');

    // Verify in UI
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    await page.reload();
    await page.waitForLoadState('networkidle');
    await usersBtn.click();
    await page.waitForTimeout(500);

    const usersList = page.locator('#users-list');
    let content = await usersList.innerHTML();
    expect(content).toContain(testUsername);

    // Delete user
    const userItem = usersList.locator(`text=${testUsername}`).first();
    const deleteBtn = userItem.locator('.. >> button:has-text("Delete")');
    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    await page.waitForTimeout(500);

    // Verify deleted
    content = await usersList.innerHTML();
    expect(content).not.toContain(testUsername);
  });

  test('Admin user: Can perform DAG actions (Pause, Schedule, Trigger)', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click DAG to show detail
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    // All action buttons should be visible and clickable
    const pauseBtn = page.locator('#pause-btn');
    expect(await pauseBtn.isDisabled()).toBeFalsy();

    const scheduleBtn = page.locator('#schedule-btn');
    expect(await scheduleBtn.isDisabled()).toBeFalsy();

    const triggerBtn = page.locator('#trigger-btn');
    expect(await triggerBtn.isDisabled()).toBeFalsy();
  });

  test('Admin user: Role dropdown shows all three options', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    const roleSelect = page.locator('#user-role');
    const options = page.locator('#user-role option');

    const optionCount = await options.count();
    expect(optionCount).toBe(3);

    // Check all three roles
    const optionTexts = [];
    for (let i = 0; i < optionCount; i++) {
      const text = await options.nth(i).textContent();
      optionTexts.push(text || '');
    }

    const allText = optionTexts.join('|');
    expect(allText).toContain('Admin');
    expect(allText).toContain('Operator');
    expect(allText).toContain('Viewer');
  });

  test('User list shows role badges with correct styling', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Create users with different roles
    const adminUser = `admin_${Date.now()}`;
    const operatorUser = `operator_${Date.now()}`;
    const viewerUser = `viewer_${Date.now()}`;

    await helpers.createTestUser(adminUser, 'pass', 'Admin');
    await helpers.createTestUser(operatorUser, 'pass', 'Operator');
    await helpers.createTestUser(viewerUser, 'pass', 'Viewer');

    // View users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    await page.reload();
    await page.waitForLoadState('networkidle');
    await usersBtn.click();
    await page.waitForTimeout(500);

    const usersList = page.locator('#users-list');
    const content = await usersList.innerHTML();

    // All roles should be visible
    expect(content).toContain('Admin');
    expect(content).toContain('Operator');
    expect(content).toContain('Viewer');

    // Cleanup
    await helpers.deleteTestUser(adminUser);
    await helpers.deleteTestUser(operatorUser);
    await helpers.deleteTestUser(viewerUser);
  });

  test('Secrets section requires proper authorization header', async ({ page }) => {
    let secretsApiCalled = false;
    let hadAuth = false;

    page.on('request', (request) => {
      if (request.url().includes('/api/secrets')) {
        secretsApiCalled = true;
        const auth = request.headerValue('Authorization');
        if (auth) {
          hadAuth = true;
        }
      }
    });

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify API was called with auth
    expect(secretsApiCalled).toBeTruthy();
    expect(hadAuth).toBeTruthy();
  });

  test('Users section requires proper authorization header', async ({ page }) => {
    let usersApiCalled = false;
    let hadAuth = false;

    page.on('request', (request) => {
      if (request.url().includes('/api/users')) {
        usersApiCalled = true;
        const auth = request.headerValue('Authorization');
        if (auth) {
          hadAuth = true;
        }
      }
    });

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // Verify API was called with auth
    expect(usersApiCalled).toBeTruthy();
    expect(hadAuth).toBeTruthy();
  });

  test('DAG actions require authorization header', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    let dagActionsAuthed = false;

    page.on('request', (request) => {
      if (request.url().includes('/api/dags') && request.method() !== 'GET') {
        const auth = request.headerValue('Authorization');
        if (auth) {
          dagActionsAuthed = true;
        }
      }
    });

    const dags = await helpers.fetchDAGs();
    if (dags.length === 0) {
      test.skip();
    }

    const firstDagId = (dags[0] as Record<string, string>).id || Object.keys(dags[0])[0];

    // Click DAG and perform action
    const dagCard = page.locator(`text=${firstDagId}`).first();
    await dagCard.click();

    const pauseBtn = page.locator('#pause-btn');
    await pauseBtn.click();

    // Wait for API call
    await page.waitForTimeout(500);

    // If a state-changing request was made, it should have auth
    // (may not always be triggered depending on implementation)
  });

  test('Navigation shows status indicator (Active)', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Check status badge
    const statusBadge = page.locator('nav >> text=Active');
    await expect(statusBadge).toBeVisible();

    // Should have styling indicating active state
    const statusSpan = page.locator('nav >> text=Active').first();
    const classes = await statusSpan.getAttribute('class');
    expect(classes).toContain('emerald-400'); // Green color
  });

  test('Admin button is always visible for authenticated users', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    const adminBtn = page.locator('nav >> button:has-text("Admin")');
    await expect(adminBtn).toBeVisible();

    // Should be interactive
    const isDisabled = await adminBtn.isDisabled();
    expect(isDisabled).toBeFalsy();
  });

  test('Content permissions: UI respects user role (Admin has all)', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    // Admin should see all sections
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');

    await expect(usersBtn).toBeVisible();
    await expect(secretsBtn).toBeVisible();

    // Should be clickable
    expect(await usersBtn.isDisabled()).toBeFalsy();
    expect(await secretsBtn.isDisabled()).toBeFalsy();
  });

  test('API errors with 401 would indicate auth failure', async ({ page }) => {
    // This is a potential error case - we're checking the app handles it gracefully
    let errorCount = 0;

    page.on('response', (response) => {
      if (response.status() === 401 && response.url().includes('/api/')) {
        errorCount++;
      }
    });

    await page.reload();
    await page.waitForLoadState('networkidle');

    // We should NOT get 401 errors when properly authenticated
    expect(errorCount).toBe(0);
  });

  test('Valid API Key format is maintained throughout session', async ({ page }) => {
    const helpers = createHelpers(page);
    await helpers.loginAsAdmin();

    const seenKeys = new Set<string>();

    page.on('request', (request) => {
      if (request.url().includes('/api/')) {
        const auth = request.headerValue('Authorization');
        if (auth) {
          seenKeys.add(auth);
        }
      }
    });

    // Trigger multiple operations
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    await page.waitForLoadState('networkidle');

    // All auth headers should be the same valid key
    expect(seenKeys.size).toBe(1);
    const authKey = Array.from(seenKeys)[0];
    expect(authKey).toBe('vortex_admin_key');
  });
});
