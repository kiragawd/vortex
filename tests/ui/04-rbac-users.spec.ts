import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('04 - RBAC User Management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('"👥 Users" button opens users section', async ({ page }) => {
    // Click users button
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Wait for users section to appear
    const usersSection = page.locator('#view-users');
    await expect(usersSection).toBeVisible();

    // DAG container should be hidden
    const dagContainer = page.locator('#view-registry');
    expect(await dagContainer.isHidden()).toBeTruthy();
  });

  test('User list displays with Username, API Key, Role columns', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Wait for list to load
    const usersList = page.locator('#users-list');
    await expect(usersList).toBeVisible();

    // Verify users can be fetched
    const users = await helpers.fetchUsers();
    expect(Array.isArray(users)).toBeTruthy();

    // If users exist, check structure
    if (users.length > 0) {
      const user = users[0] as Record<string, unknown>;
      expect(user.username).toBeDefined();
      expect(user.api_key).toBeDefined();
      expect(user.role).toBeDefined();
    }
  });

  test('"ADD USER" button opens modal', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Click ADD USER button
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // User modal should be visible
    const modal = page.locator('#add-user-modal');
    await expect(modal).toBeVisible();
  });

  test('User modal has Username, Password, Role dropdown', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Check username input
    const usernameInput = page.locator('#new-user-name');
    await expect(usernameInput).toBeVisible();
    expect(await usernameInput.getAttribute('placeholder')).toMatch(/username/i);

    // Check password input
    const passwordInput = page.locator('#new-user-pass');
    await expect(passwordInput).toBeVisible();
    expect(await passwordInput.getAttribute('placeholder')).toMatch(/password/i);
    expect(await passwordInput.getAttribute('type')).toBe('password');

    // Check role dropdown
    const roleSelect = page.locator('#new-user-role');
    await expect(roleSelect).toBeVisible();
  });

  test('Role dropdown shows all 3 options: Admin, Operator, Viewer', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Check dropdown options
    const roleSelect = page.locator('#new-user-role');
    const options = page.locator('#new-user-role option');

    const optionCount = await options.count();
    expect(optionCount).toBe(3);

    // Check specific roles
    const optionTexts = [];
    for (let i = 0; i < optionCount; i++) {
      const text = await options.nth(i).textContent();
      optionTexts.push(text || '');
    }

    expect(optionTexts.join('|')).toContain('Admin');
    expect(optionTexts.join('|')).toContain('Operator');
    expect(optionTexts.join('|')).toContain('Viewer');
  });

  test('Form submission creates user (POST /api/users)', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill form
    const testUsername = `testuser_${Date.now()}`;
    const testPassword = 'test_password_123';
    const testRole = 'Operator';

    await page.locator('#new-user-name').fill(testUsername);
    await page.locator('#new-user-pass').fill(testPassword);
    await page.locator('#new-user-role').selectOption(testRole);

    // Submit form
    const submitBtn = page.locator('#add-user-modal button:has-text("CREATE USER")');

    // Wait for API call
    const apiPromise = page.waitForResponse(response =>
      response.url().includes('/api/users') && response.request().method() === 'POST'
    );

    await submitBtn.click();

    // Wait for API response
    const apiResponse = await apiPromise;
    expect(apiResponse.ok()).toBeTruthy();

    // Modal should close
    const modal = page.locator('#add-user-modal');
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('User appears in list with correct role badge', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Add new user
    const testUsername = `displaytest_${Date.now()}`;
    await helpers.createTestUser(testUsername, 'password123', 'Admin');

    // Refresh page
    await page.reload();
    await page.waitForLoadState('networkidle');

    // Navigate to users again
    await usersBtn.click();
    await page.waitForTimeout(500);

    // Check if user appears in list
    const usersList = page.locator('#users-list');
    const content = await usersList.innerHTML();
    expect(content).toContain(testUsername);
    expect(content).toContain('Admin');
  });

  test.skip('Delete user button removes them from list', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Create a test user
    const testUsername = `deletetest_${Date.now()}`;
    await helpers.createTestUser(testUsername, 'password123', 'Viewer');

    // Refresh list
    await page.reload();
    await page.waitForLoadState('networkidle');
    await usersBtn.click();
    await page.waitForTimeout(500);

    // Find and click delete button
    const usersList = page.locator('#users-list');
    const userItem = usersList.locator(`text=${testUsername}`).first();
    const deleteBtn = userItem.locator('.. >> button:has-text("Delete")');

    // Confirm delete
    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    // Wait for deletion
    await page.waitForTimeout(500);

    // Verify user is gone
    const listContent = await usersList.innerHTML();
    expect(listContent).not.toContain(testUsername);
  });

  test('Modal close button (X) closes without saving', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill form
    await page.locator('#new-user-name').fill('tempuser');
    await page.locator('#new-user-pass').fill('temppass');

    // Click close button
    const closeBtn = page.locator('#add-user-modal button[onclick="closeAddUserModal()"]');
    await closeBtn.click();

    // Modal should be hidden
    const modal = page.locator('#add-user-modal');
    expect(await modal.isHidden()).toBeTruthy();

    // Verify user wasn't saved
    const usersSection = page.locator('#view-users');
    const content = await usersSection.innerHTML();
    expect(content).not.toContain('tempuser');
  });

  test.skip('Form fields are required (validation)', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Try to submit empty form
    const submitBtn = page.locator('#add-user-modal button:has-text("CREATE USER")');

    // Check if button is disabled or form validates
    const isDisabled = await submitBtn.isDisabled();
    const hasRequired = await page.locator('#new-user-name').getAttribute('required');

    // At minimum, fields should show required attribute or button disabled
    expect(isDisabled || hasRequired !== null).toBeTruthy();
  });

  test('Back button closes users section and returns to DAG list', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Verify users section is visible
    const usersSection = page.locator('#view-users');
    await expect(usersSection).toBeVisible();

    // Click back button
    const backBtn = page.locator('#view-users button').first();
    await backBtn.click();

    // Users section should be hidden
    expect(await usersSection.isHidden()).toBeTruthy();

    // DAG container should be visible again
    const dagContainer = page.locator('#view-registry');
    await expect(dagContainer).toBeVisible();
  });

  test.skip('User list shows API key in font-mono', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Get users
    const users = await helpers.fetchUsers();

    if (users.length > 0) {
      // Check that API key is displayed in list
      const usersList = page.locator('#users-list');
      const apiKeyElement = usersList.locator('.font-mono').first();
      await expect(apiKeyElement).toBeVisible();
    }
  });
});
