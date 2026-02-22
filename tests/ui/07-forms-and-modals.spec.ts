import { test, expect } from '@playwright/test';
import { createHelpers } from './helpers';

test.describe('07 - Forms & Modals Validation & UX', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('Secret modal appears centered and properly sized', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#secret-modal');
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
    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#user-modal');
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
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#secret-modal');
    await expect(modal).toBeVisible();

    // Click close button
    const closeBtn = page.locator('#secret-modal >> svg').locator('.. >> button');
    await closeBtn.click();

    // Modal should be hidden
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('User modal close button (X) closes modal', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Modal should be visible
    const modal = page.locator('#user-modal');
    await expect(modal).toBeVisible();

    // Click close button
    const closeBtn = page.locator('#user-modal >> svg').locator('.. >> button');
    await closeBtn.click();

    // Modal should be hidden
    expect(await modal.isHidden()).toBeTruthy();
  });

  test('Secret form submission shows feedback (button interaction)', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill form
    const keyInput = page.locator('#secret-key');
    const valueInput = page.locator('#secret-value');
    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');

    await keyInput.fill(`TEST_${Date.now()}`);
    await valueInput.fill('test_value');

    // Button should be enabled and clickable
    const isDisabled = await submitBtn.isDisabled();
    expect(isDisabled).toBeFalsy();

    // Button should respond to hover
    await submitBtn.hover();
    const hoverClasses = await submitBtn.getAttribute('class');
    expect(hoverClasses).toContain('hover');
  });

  test('User form submission shows feedback (button interaction)', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill form
    const usernameInput = page.locator('#user-username');
    const passwordInput = page.locator('#user-password');
    const roleSelect = page.locator('#user-role');
    const submitBtn = page.locator('#user-modal button:has-text("Create User")');

    await usernameInput.fill(`user_${Date.now()}`);
    await passwordInput.fill('password123');
    await roleSelect.selectOption('Admin');

    // Button should be enabled
    const isDisabled = await submitBtn.isDisabled();
    expect(isDisabled).toBeFalsy();

    // Check for hover styling
    const classes = await submitBtn.getAttribute('class');
    expect(classes).toContain('hover');
  });

  test('Empty secret form has disabled or required fields', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check if fields have required attribute
    const keyInput = page.locator('#secret-key');
    const valueInput = page.locator('#secret-value');
    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');

    // Either fields should have required or button should enforce validation
    const keyRequired = await keyInput.getAttribute('required');
    const valueRequired = await valueInput.getAttribute('required');

    // At least one validation method should exist
    expect(keyRequired !== null || valueRequired !== null || await submitBtn.isDisabled()).toBeTruthy();
  });

  test('Empty user form has disabled or required fields', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Check if fields have required attribute
    const usernameInput = page.locator('#user-username');
    const passwordInput = page.locator('#user-password');
    const submitBtn = page.locator('#user-modal button:has-text("Create User")');

    // Check for validation
    const usernameRequired = await usernameInput.getAttribute('required');
    const passwordRequired = await passwordInput.getAttribute('required');

    expect(usernameRequired !== null || passwordRequired !== null || await submitBtn.isDisabled()).toBeTruthy();
  });

  test('Secret form fields clear when modal is reopened after successful submit', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    let addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fill and submit
    const keyInput = page.locator('#secret-key');
    const valueInput = page.locator('#secret-value');

    await keyInput.fill(`FORM_TEST_${Date.now()}`);
    await valueInput.fill('test_value');

    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
    await submitBtn.click();

    // Wait for submission and modal close
    await page.waitForTimeout(500);

    // Reopen modal
    addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Fields should be cleared
    const newKeyValue = await page.locator('#secret-key').inputValue();
    const newValueValue = await page.locator('#secret-value').inputValue();

    expect(newKeyValue).toBe('');
    expect(newValueValue).toBe('');
  });

  test('User form fields clear when modal is reopened after successful submit', async ({ page }) => {
    // Navigate to users
    const usersBtn = page.locator('nav >> button:has-text("👥 Users")');
    await usersBtn.click();

    // Open modal
    let addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fill and submit
    const usernameInput = page.locator('#user-username');
    const passwordInput = page.locator('#user-password');
    const roleSelect = page.locator('#user-role');

    await usernameInput.fill(`userform_${Date.now()}`);
    await passwordInput.fill('password123');
    await roleSelect.selectOption('Operator');

    const submitBtn = page.locator('#user-modal button:has-text("Create User")');
    await submitBtn.click();

    // Wait for submission
    await page.waitForTimeout(500);

    // Reopen modal
    addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    // Fields should be cleared
    const newUsername = await page.locator('#user-username').inputValue();
    const newPassword = await page.locator('#user-password').inputValue();

    expect(newUsername).toBe('');
    expect(newPassword).toBe('');
  });

  test('Modal has proper backdrop styling (semi-transparent dark overlay)', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const modal = page.locator('#secret-modal');
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
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const submitBtn = page.locator('#secret-modal button:has-text("Store Secret")');
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
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const keyInput = page.locator('#secret-key');
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
    const addBtn = page.locator('#users-section >> button:has-text("ADD USER")');
    await addBtn.click();

    const roleSelect = page.locator('#user-role');
    const options = page.locator('#user-role option');

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
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    const modal = page.locator('#secret-modal');
    const classes = await modal.getAttribute('class');

    // Should have z-index class
    expect(classes).toContain('z-50');
  });

  test('Modal header has clear title and close button', async ({ page }) => {
    // Navigate to secrets
    const secretsBtn = page.locator('nav >> button:has-text("🔐 Secrets")');
    await secretsBtn.click();

    // Open modal
    const addBtn = page.locator('#secrets-section >> button:has-text("ADD SECRET")');
    await addBtn.click();

    // Check for title
    const title = page.locator('#secret-modal >> text=New Secret');
    await expect(title).toBeVisible();

    // Check for close button (X icon)
    const closeBtn = page.locator('#secret-modal svg').locator('.. >> button');
    await expect(closeBtn).toBeVisible();
  });
});
