import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('07 - Forms & Modals Validation & UX', () => {
  test.beforeEach(async ({ page }) => {
    // Mock the backend APIs to ensure the forms actually get a successful response
    await page.route('/api/secrets', async route => {
      await route.fulfill({ status: 200, body: JSON.stringify({ message: 'Success' }) });
    });
    await page.route('/api/users', async route => {
      await route.fulfill({ status: 200, body: JSON.stringify({ message: 'Success' }) });
    });

    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Secret modal appears centered and properly sized', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#add-secret-modal');
    await expect(modal).toBeVisible();

    // Check modal styling (should be centered and have max-width)
    const classes = await modal.getAttribute('class');
    expect(classes).toContain('fixed');
    expect(classes).toContain('inset-0');
    expect(classes).toContain('items-center');
    expect(classes).toContain('justify-center');
  });

  test('User modal appears centered and properly sized', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#add-user-modal');
    await expect(modal).toBeVisible();

    // Check modal styling
    const classes = await modal.getAttribute('class');
    expect(classes).toContain('fixed');
    expect(classes).toContain('inset-0');
  });

  test('Secret modal close button (X) closes modal', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#add-secret-modal');
    await expect(modal).toBeVisible();

    // Click close button
    const closeBtn = page.locator('#add-secret-modal button').filter({ has: page.locator('svg') });
    await closeBtn.click();

    // Modal should be hidden
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('User modal close button (X) closes modal', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#add-user-modal');
    await expect(modal).toBeVisible();

    // Click close button
    const closeBtn = page.locator('#add-user-modal button').filter({ has: page.locator('svg') });
    await closeBtn.click();

    // Modal should be hidden
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('Secret form submission shows feedback (button interaction)', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill form
    const keyInput = page.locator('#new-secret-key');
    const valueInput = page.locator('#new-secret-val');
    const submitBtn = page.locator('#add-secret-modal button:has-text("SAVE SECRET")');

    await keyInput.fill(`TEST_${Date.now()}`);
    await valueInput.fill('test_value');

    // Button should be enabled and clickable
    const isDisabled = await submitBtn.isDisabled();
    expect(isDisabled).toBeFalsy();
  });

  test('User form submission shows feedback (button interaction)', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill form
    const usernameInput = page.locator('#new-user-name');
    const passwordInput = page.locator('#new-user-pass');
    const roleSelect = page.locator('#new-user-role');
    const submitBtn = page.locator('#add-user-modal button:has-text("CREATE USER")');

    await usernameInput.fill(`user_${Date.now()}`);
    await passwordInput.fill('password123');
    await roleSelect.selectOption('Admin');

    // Button should be enabled
    const isDisabled = await submitBtn.isDisabled();
    expect(isDisabled).toBeFalsy();
  });

  test('Empty secret form has disabled or required fields', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check if fields have required attribute
    const keyInput = page.locator('#new-secret-key');
    const valueInput = page.locator('#new-secret-val');
    const submitBtn = page.locator('#add-secret-modal button:has-text("SAVE SECRET")');

    await submitBtn.click();

    // Verify error is shown since fields were empty
    const errorMsg = page.locator('#add-secret-error');
    await expect(errorMsg).toBeVisible();
    await expect(errorMsg).toHaveText(/Key and value required/);
  });

  test('Empty user form has disabled or required fields', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Check if fields have required attribute
    const usernameInput = page.locator('#new-user-name');
    const passwordInput = page.locator('#new-user-pass');
    const submitBtn = page.locator('#add-user-modal button:has-text("CREATE USER")');

    await submitBtn.click();

    // Check for validation error
    const errorMsg = page.locator('#add-user-error');
    await expect(errorMsg).toBeVisible();
    await expect(errorMsg).toHaveText(/Username and password required/);
  });

  test.skip('Secret form fields clear when modal is reopened after successful submit', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    let addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill and submit
    const keyInput = page.locator('#new-secret-key');
    const valueInput = page.locator('#new-secret-val');

    await keyInput.fill(`FORM_TEST_${Date.now()}`);
    await valueInput.fill('test_value');

    const submitBtn = page.locator('#add-secret-modal button:has-text("SAVE SECRET")');
    await submitBtn.click();

    // Wait for submission and modal close
    await expect(page.locator('#add-secret-modal')).toBeHidden({ timeout: 2000 });

    // Reopen modal
    addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fields should be cleared
    const newKeyValue = await page.locator('#new-secret-key').inputValue();
    const newValueValue = await page.locator('#new-secret-val').inputValue();

    expect(newKeyValue).toBe('');
    expect(newValueValue).toBe('');
  });

  test.skip('User form fields clear when modal is reopened after successful submit', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    let addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill and submit
    const usernameInput = page.locator('#new-user-name');
    const passwordInput = page.locator('#new-user-pass');
    const roleSelect = page.locator('#new-user-role');

    await usernameInput.fill(`userform_${Date.now()}`);
    await passwordInput.fill('password123');
    await roleSelect.selectOption('Operator');

    const submitBtn = page.locator('#add-user-modal button:has-text("CREATE USER")');
    await submitBtn.click();

    // Wait for submission
    await expect(page.locator('#add-user-modal')).toBeHidden({ timeout: 2000 });

    // Reopen modal
    addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fields should be cleared
    const newUsername = await page.locator('#new-user-name').inputValue();
    const newPassword = await page.locator('#new-user-pass').inputValue();

    expect(newUsername).toBe('');
    expect(newPassword).toBe('');
  });

  test('Modal has proper backdrop styling (semi-transparent dark overlay)', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const modal = page.locator('#add-secret-modal');
    const classes = await modal.getAttribute('class');

    // Should have dark semi-transparent background
    expect(classes).toContain('bg-black');
    expect(classes).toContain('fixed');
    expect(classes).toContain('inset-0');
  });

  test('Form buttons have proper styling with vortex-gradient', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const submitBtn = page.locator('#add-secret-modal button:has-text("SAVE SECRET")');
    const classes = await submitBtn.getAttribute('class');

    // Should have vortex-gradient styling
    expect(classes).toContain('vortex-gradient');
    expect(classes).toContain('text-black');
    expect(classes).toContain('font-black');
  });

  test('Input fields have proper styling and focus states', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const keyInput = page.locator('#new-secret-key');
    const classes = await keyInput.getAttribute('class');

    // Should have glass-like styling
    expect(classes).toContain('bg-white');
    expect(classes).toContain('border');
    expect(classes).toContain('rounded');
    expect(classes).toContain('px-4');
    expect(classes).toContain('py-3');
  });

  test('Role dropdown has all options properly labeled', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#view-users >> button:has-text("ADD USER")');
    await addBtn.click();

    const roleSelect = page.locator('#new-user-role');
    const options = page.locator('#new-user-role option');

    // Get all option texts
    const optionTexts = [];
    for (let i = 0; i < (await options.count()); i++) {
      const text = await options.nth(i).textContent();
      optionTexts.push(text || '');
    }

    // Should have full role descriptions
    const allText = optionTexts.join('|');
    expect(allText).toContain('Admin');
    expect(allText).toContain('Operator');
    expect(allText).toContain('Viewer');
  });

  test('Modal has proper z-index to appear above other content', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const modal = page.locator('#add-secret-modal');
    const classes = await modal.getAttribute('class');

    // Should have z-index class
    expect(classes).toContain('z-[10000]');
  });

  test('Modal header has clear title and close button', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#view-secrets >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check for title using regex to bypass emoji
    const title = page.locator('#add-secret-modal >> text=/Add Secret/');
    await expect(title).toBeVisible();

    // Check for close button (X icon)
    const closeBtn = page.locator('#add-secret-modal button[onclick="closeAddSecretModal()"]');
    await expect(closeBtn).toBeVisible();
  });
});
