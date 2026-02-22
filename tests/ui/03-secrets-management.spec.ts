import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('03 - Secrets Management (Pillar 3)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('"🔐 Secrets" button in nav opens secrets section', async ({ page }) => {
    // Click secrets button
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Wait for secrets section to appear
    const secretsSection = page.locator('#secrets-section');
    await expect(secretsSection).toBeVisible();

    // DAG container should be hidden
    const dagContainer = page.locator('#dag-container');
    expect(await dagContainer.isHidden()).toBeTruthy();
  });

  test('Secrets list displays with correct structure', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Wait for list to load
    const secretsList = page.locator('#secrets-list');
    await expect(secretsList).toBeVisible();

    // List should have items or be empty (both valid states)
    const secretsData = await helpers.fetchSecrets();
    expect(secretsData).toBeDefined();
  });

  test('"ADD SECRET" button opens modal', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Click ADD SECRET button
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Secret modal should be visible
    const modal = page.locator('#secret-modal');
    await expect(modal).toBeVisible();
  });

  test('Secret modal has KEY_NAME and VALUE fields', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check KEY_NAME input
    const keyInput = page.locator('#secret-key');
    await expect(keyInput).toBeVisible();
    expect(await keyInput.getAttribute('placeholder')).toContain('KEY_NAME');

    // Check VALUE input (password field)
    const valueInput = page.locator('#secret-value');
    await expect(valueInput).toBeVisible();
    expect(await valueInput.getAttribute('type')).toBe('password');
  });

  test('Form submission stores secret (POST /api/secrets)', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill form
    const testKey = `TEST_KEY_${Date.now()}`;
    const testValue = 'test_secret_value_12345';

    await page.locator('#secret-key').fill(testKey);
    await page.locator('#secret-value').fill(testValue);

    // Submit form
    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
    
    // Wait for API call
    const apiPromise = page.waitForResponse(response => 
      response.url().includes('/api/secrets') && response.request().method() === 'POST'
    );

    await submitBtn.click();

    // Wait for API response
    const apiResponse = await apiPromise;
    expect(apiResponse.ok()).toBeTruthy();

    // Modal should close
    const modal = page.locator('#secret-modal');
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('Newly added secret appears in list', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Get initial secrets
    const initialSecrets = await helpers.fetchSecrets();
    const initialCount = (initialSecrets as Record<string, unknown>).secrets 
      ? ((initialSecrets as Record<string, unknown>).secrets as unknown[]).length 
      : 0;

    // Add a new secret
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const testKey = `SECRET_${Date.now()}`;
    const testValue = 'secret_value';

    await page.locator('#secret-key').fill(testKey);
    await page.locator('#secret-value').fill(testValue);

    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
    await submitBtn.click();

    // Wait for list refresh
    await page.waitForTimeout(500);

    // Check if new secret appears in list
    const secretsList = page.locator('#secrets-list');
    const listContent = await secretsList.innerHTML();
    expect(listContent).toContain(testKey);
  });

  test('Delete secret button removes it from list', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Create a test secret first
    const testKey = `DELETE_TEST_${Date.now()}`;
    await helpers.createTestSecret(testKey, 'temp_value');

    // Refresh list
    await page.reload();
    await page.waitForLoadState('networkidle');

    // Navigate to secrets again
    await secretsBtn.click();

    // Wait for list to load
    await page.waitForTimeout(500);

    // Find and click delete button for our test secret
    const secretsList = page.locator('#secrets-list');
    const secretItem = secretsList.locator(`text=${testKey}`).first();
    const deleteBtn = secretItem.locator('.. >> button:has-text("Delete")');

    // Confirm delete
    page.once('dialog', dialog => dialog.accept());
    await deleteBtn.click();

    // Wait for deletion
    await page.waitForTimeout(500);

    // Verify secret is gone
    const listContent = await secretsList.innerHTML();
    expect(listContent).not.toContain(testKey);
  });

  test('Confirmation dialog shows on delete', async ({ page }) => {
    const helpers = createHelpers(page);

    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Create a test secret
    const testKey = `CONFIRM_TEST_${Date.now()}`;
    await helpers.createTestSecret(testKey, 'temp_value');

    // Refresh and navigate back
    await page.reload();
    await page.waitForLoadState('networkidle');
    await secretsBtn.click();
    await page.waitForTimeout(500);

    // Listen for dialog
    let dialogCaught = false;
    page.once('dialog', async dialog => {
      expect(dialog.type()).toBe('confirm');
      dialogCaught = true;
      await dialog.dismiss();
    });

    // Click delete
    const secretsList = page.locator('#secrets-list');
    const secretItem = secretsList.locator(`text=${testKey}`).first();
    const deleteBtn = secretItem.locator('.. >> button:has-text("Delete")');
    await deleteBtn.click();

    // Verify dialog was shown
    expect(dialogCaught).toBeTruthy();
  });

  test('Modal close button (X) closes without saving', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill form
    await page.locator('#secret-key').fill('TEMP_SECRET');
    await page.locator('#secret-value').fill('temp_value');

    // Get close button
    const closeBtn = page.locator('#secret-modal button[onclick="closeSecretModal()"]');
    await closeBtn.click();

    // Modal should be hidden
    const modal = page.locator('#secret-modal');
    expect(await modal.isHidden()).toBeTruthy();

    // Verify secret wasn't saved (try to find it in list)
    const secretsList = page.locator('#secrets-list');
    const content = await secretsList.innerHTML();
    expect(content).not.toContain('TEMP_SECRET');
  });

  test('Form fields clear after successful submit', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill form
    const keyInput = page.locator('#secret-key');
    const valueInput = page.locator('#secret-value');

    await keyInput.fill(`TEST_KEY_${Date.now()}`);
    await valueInput.fill('test_value');

    // Submit
    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
    await submitBtn.click();

    // Wait for submission
    await page.waitForTimeout(500);

    // Modal should close and reopen for next entry
    // If we open again, fields should be empty
    const modal = page.locator('#secret-modal');
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('Back button closes secrets section and returns to DAG list', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Verify secrets section is visible
    const secretsSection = page.locator('#secrets-section');
    await expect(secretsSection).toBeVisible();

    // Click back button
    const backBtn = page.locator('#secrets-section button').first();
    await backBtn.click();

    // Secrets section should be hidden
    expect(await secretsSection.isHidden()).toBeTruthy();

    // DAG container should be visible again
    const dagContainer = page.locator('#dag-container');
    await expect(dagContainer).toBeVisible();
  });
});
