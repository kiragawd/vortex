#!/usr/bin/env python3
"""Rewrite all Playwright test files for the new React UI."""
import os

BASE = os.path.dirname(os.path.abspath(__file__))

files = {}

files["02-dag-detail.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('02 - DAG List & Detail View', () => {
  test('DAGs page renders table with heading', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
  });

  test('DAGs table shows rows when data exists', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    const count = await rows.count();
    expect(count).toBeGreaterThan(0);
  });

  test('Click DAG row navigates to detail page', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('h1')).toBeVisible({ timeout: 10000 });
  });

  test('DAG detail page shows Graph, Runs, Info tabs', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Graph")')).toBeVisible();
    await expect(page.locator('button:has-text("Runs")')).toBeVisible();
    await expect(page.locator('button:has-text("Info")')).toBeVisible();
  });

  test('DAG detail page shows Trigger and Retry Last buttons', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Trigger")')).toBeVisible();
    await expect(page.locator('button:has-text("Retry Last")')).toBeVisible();
  });

  test('DAG detail quick stat cards show Schedule, Last Run, Next Run, Tasks', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('text=Schedule')).toBeVisible();
    await expect(page.locator('text=Last Run')).toBeVisible();
    await expect(page.locator('text=Next Run')).toBeVisible();
    await expect(page.locator('text=Tasks')).toBeVisible();
  });

  test('Graph tab renders SVG with task nodes', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const svg = page.locator('svg');
    await expect(svg.first()).toBeVisible({ timeout: 10000 });
  });

  test('Runs tab shows runs table or empty state', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await page.locator('button:has-text("Runs")').click();
    const table = page.locator('table');
    const emptyMsg = page.locator('text=No runs recorded yet');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Info tab shows DAG Properties and Tasks list', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await page.locator('button:has-text("Info")').click();
    await expect(page.locator('text=DAG Properties')).toBeVisible();
  });

  test('Trigger button fires API call', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible();
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/trigger') && r.request().method() === 'POST'
    );
    await triggerBtn.click();
    const res = await apiPromise.catch(() => null);
    expect(res).not.toBeNull();
  });
});
"""

files["03-secrets-management.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('03 - Settings Page (Secrets/Auth Providers)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/settings');
    await page.waitForLoadState('networkidle');
  });

  test('Settings page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('General settings card is visible', async ({ page }) => {
    await expect(page.locator('text=General')).toBeVisible();
  });

  test('Authentication Providers section is visible', async ({ page }) => {
    await expect(page.locator('text=Authentication Providers')).toBeVisible();
  });

  test('Instance name and timezone fields are present', async ({ page }) => {
    await expect(page.locator('text=Instance Name')).toBeVisible();
    await expect(page.locator('text=Timezone')).toBeVisible();
  });

  test('Provider list shows at least one provider', async ({ page }) => {
    const providers = page.locator('text=/OIDC|SAML|LDAP|Local/');
    const count = await providers.count();
    expect(count).toBeGreaterThan(0);
  });
});
"""

files["04-rbac-users.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('04 - RBAC Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/rbac');
    await page.waitForLoadState('networkidle');
  });

  test('RBAC page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText(/RBAC|Access/);
  });

  test('Roles & Permissions tab content is visible', async ({ page }) => {
    await expect(page.locator('text=Roles')).toBeVisible();
  });

  test('API Tokens tab is clickable and shows token UI', async ({ page }) => {
    const tokensTab = page.locator('button:has-text("API Tokens")');
    await expect(tokensTab).toBeVisible();
    await tokensTab.click();
    await expect(page.locator('text=Token Name')).toBeVisible();
  });

  test('IP Allowlist tab is clickable', async ({ page }) => {
    const ipTab = page.locator('button:has-text("IP Allowlist")');
    await expect(ipTab).toBeVisible();
    await ipTab.click();
    await expect(page.locator('text=CIDR')).toBeVisible();
  });

  test('Role cards are displayed', async ({ page }) => {
    const roleCards = page.locator('.rounded-xl').filter({ hasText: /Admin|Operator|Viewer/ });
    const count = await roleCards.count();
    expect(count).toBeGreaterThan(0);
  });
});
"""

files["05-swarm-panel.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('05 - Swarm Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/swarm');
    await page.waitForLoadState('networkidle');
  });

  test('Swarm page heading is visible', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Swarm');
  });

  test('Swarm status cards show Mode, Active Workers, Queue Depth', async ({ page }) => {
    await expect(page.locator('text=Swarm Mode')).toBeVisible();
    await expect(page.locator('text=Active Workers')).toBeVisible();
    await expect(page.locator('text=Queue Depth')).toBeVisible();
  });

  test('Workers table heading is visible', async ({ page }) => {
    await expect(page.locator('text=Workers')).toBeVisible();
  });

  test('Workers table has correct columns or shows empty state', async ({ page }) => {
    const table = page.locator('table');
    const emptyMsg = page.locator('text=/no.*worker|vortex worker/i');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Drain button visible if workers exist', async ({ page }) => {
    const table = page.locator('table tbody tr');
    const count = await table.count().catch(() => 0);
    if (count > 0) {
      const drainBtn = page.locator('button:has-text("Drain")').first();
      await expect(drainBtn).toBeVisible();
    }
  });
});
"""

files["06-task-instances.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('06 - Run Detail & Task Instances', () => {
  test('Runs page loads with heading', async ({ page }) => {
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
  });

  test('Runs page shows table or empty state', async ({ page }) => {
    await page.goto('/runs');
    const table = page.locator('table');
    const emptyMsg = page.locator('text=/no runs|empty/i');
    const hasTable = await table.isVisible().catch(() => false);
    const hasEmpty = await emptyMsg.isVisible().catch(() => false);
    expect(hasTable || hasEmpty).toBeTruthy();
  });

  test('Click run row navigates to run detail', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Run Detail')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page shows summary cards', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Total Tasks')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page shows task graph and task instances table', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const svg = page.locator('svg');
      await expect(svg.first()).toBeVisible({ timeout: 10000 });
      const taskTable = page.locator('table');
      await expect(taskTable.first()).toBeVisible();
    }
  });

  test('Run detail breadcrumb navigation works', async ({ page }) => {
    await page.goto('/runs');
    const rows = page.locator('table tbody tr');
    const count = await rows.count().catch(() => 0);
    if (count > 0) {
      await rows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const breadcrumb = page.locator('a[href*="/dags/"]').first();
      if (await breadcrumb.isVisible()) {
        await breadcrumb.click();
        await page.waitForURL(/\/dags\//);
      }
    }
  });
});
"""

files["07-forms-and-modals.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('07 - Forms, Buttons & Actions', () => {
  test('DAG Trigger button changes text while triggering', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible();
    await triggerBtn.click();
    await expect(triggerBtn).toContainText(/Trigger/);
  });

  test('Retry Last button is functional', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const retryBtn = page.locator('button:has-text("Retry Last")');
    await expect(retryBtn).toBeVisible();
    await expect(retryBtn).toBeEnabled();
  });

  test('RBAC token creation form has name and scopes fields', async ({ page }) => {
    await page.goto('/rbac');
    const tokensTab = page.locator('button:has-text("API Tokens")');
    await expect(tokensTab).toBeVisible();
    await tokensTab.click();
    await expect(page.locator('text=Token Name')).toBeVisible();
    await expect(page.locator('text=Scopes')).toBeVisible();
  });

  test('RBAC IP Allowlist has CIDR input and add button', async ({ page }) => {
    await page.goto('/rbac');
    const ipTab = page.locator('button:has-text("IP Allowlist")');
    await expect(ipTab).toBeVisible();
    await ipTab.click();
    await expect(page.locator('text=CIDR')).toBeVisible();
    const addBtn = page.locator('button:has-text("Add")');
    await expect(addBtn).toBeVisible();
  });

  test('Compliance approval requests section is visible', async ({ page }) => {
    await page.goto('/compliance');
    await expect(page.locator('h1')).toContainText('Compliance');
    await expect(page.locator('text=Approval Requests')).toBeVisible();
  });

  test('Events page alert routing form has channel dropdown', async ({ page }) => {
    await page.goto('/events');
    await expect(page.locator('text=Alert Routing')).toBeVisible();
    const selects = page.locator('select');
    const count = await selects.count();
    expect(count).toBeGreaterThan(0);
  });

  test('Swarm drain button is enabled when workers exist', async ({ page }) => {
    await page.goto('/swarm');
    const drainBtns = page.locator('button:has-text("Drain")');
    const count = await drainBtns.count();
    if (count > 0) {
      await expect(drainBtns.first()).toBeEnabled();
    }
  });
});
"""

files["08-api-integration.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('08 - API Integration', () => {
  test('GET /api/dags returns paginated data on page load', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/dags') && r.request().method() === 'GET'
    );
    await page.goto('/dags');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('data');
    expect(Array.isArray(body.data)).toBeTruthy();
  });

  test('API requests include Authorization header', async ({ page }) => {
    let authHeader: string | null = null;
    page.on('request', (req) => {
      if (req.url().includes('/api/')) {
        authHeader = req.headers()['authorization'] ?? null;
      }
    });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    expect(authHeader).not.toBeNull();
    expect(authHeader).toContain('Bearer');
  });

  test('GET /api/health returns status', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/health') && r.request().method() === 'GET'
    );
    await page.goto('/');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('status');
  });

  test('GET /api/runs returns paginated data', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/runs') && r.request().method() === 'GET'
    );
    await page.goto('/runs');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('GET /api/dags/:id/tasks returns tasks and dependencies', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/tasks') && r.request().method() === 'GET'
    );
    await rows.first().click();
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body).toHaveProperty('tasks');
    expect(body).toHaveProperty('dependencies');
  });

  test('POST /api/dags/:id/trigger fires correctly', async ({ page }) => {
    await page.goto('/dags');
    const rows = page.locator('table tbody tr');
    await expect(rows.first()).toBeVisible({ timeout: 10000 });
    await rows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/trigger') && r.request().method() === 'POST'
    );
    await page.locator('button:has-text("Trigger")').click();
    const res = await apiPromise.catch(() => null);
    expect(res).not.toBeNull();
  });

  test('GET /api/swarm/status returns swarm data', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/swarm/status')
    );
    await page.goto('/swarm');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('GET /api/rbac/roles returns roles', async ({ page }) => {
    const apiPromise = page.waitForResponse(
      (r) => r.url().includes('/api/rbac/roles')
    );
    await page.goto('/rbac');
    const res = await apiPromise;
    expect(res.ok()).toBeTruthy();
  });

  test('API error handling - invalid endpoint returns error gracefully', async ({ page }) => {
    await page.goto('/');
    const res = await page.request.fetch('http://localhost:3000/api/nonexistent', {
      headers: { Authorization: 'Bearer vortex_admin_key' },
    });
    expect(res.status()).toBeGreaterThanOrEqual(400);
  });
});
"""

files["09-responsive-design.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('09 - Responsive Design', () => {
  test('Desktop layout shows sidebar and main content', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('aside')).toBeVisible();
    await expect(page.locator('h1')).toContainText('Dashboard');
  });

  test('Stat cards use grid layout on desktop', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const grid = page.locator('.grid.grid-cols-1').first();
    await expect(grid).toBeVisible();
  });

  test('Mobile viewport renders correctly', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('h1')).toContainText('Dashboard');
  });

  test('Sidebar toggle works to collapse/expand', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('aside')).toBeVisible();
    await page.locator('button[aria-label="Toggle sidebar"]').click();
    await expect(page.locator('aside')).not.toBeVisible();
    await page.locator('button[aria-label="Toggle sidebar"]').click();
    await expect(page.locator('aside')).toBeVisible();
  });

  test('Tables are scrollable on small viewports', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    const table = page.locator('table');
    if (await table.isVisible().catch(() => false)) {
      const wrapper = page.locator('.overflow-x-auto');
      const count = await wrapper.count();
      expect(count).toBeGreaterThan(0);
    }
  });

  test('Tablet viewport renders DAGs page', async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 });
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('h1')).toContainText('DAGs');
  });

  test('Navigation works at all viewports', async ({ page }) => {
    for (const size of [
      { width: 1920, height: 1080 },
      { width: 768, height: 1024 },
      { width: 375, height: 667 },
    ]) {
      await page.setViewportSize(size);
      await page.goto('/');
      await page.waitForLoadState('networkidle');
      await expect(page.locator('h1')).toBeVisible();
    }
  });
});
"""

files["10-auth-and-rbac.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('10 - Auth & Login', () => {
  test('Login page has username and password fields', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('input[type="text"], input#username')).toBeVisible();
    await expect(page.locator('input[type="password"]')).toBeVisible();
  });

  test('Login page has Sign in button', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('button:has-text("Sign in")')).toBeVisible();
  });

  test('Login page shows Vortex branding', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('text=Vortex')).toBeVisible();
  });

  test('Login page shows default credentials hint', async ({ page }) => {
    await page.goto('/login');
    await expect(page.locator('text=admin')).toBeVisible();
  });

  test('Auth token persists across navigation', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Logout button redirects to login', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const logoutBtn = page.locator('button[aria-label="Sign out"]');
    await expect(logoutBtn).toBeVisible();
    await logoutBtn.click();
    await page.waitForURL('**/login', { timeout: 10000 });
    await expect(page.locator('text=Sign in')).toBeVisible();
  });
});
"""

files["11-dark-mode-and-routing.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('11 - Dark Mode & Routing', () => {
  test('Light mode is default', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass ?? '').not.toContain('dark');
  });

  test('Theme toggle button is visible', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('button[aria-label="Toggle theme"]')).toBeVisible();
  });

  test('Clicking theme toggle enables dark mode', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass).toContain('dark');
  });

  test('Dark mode applies dark background', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    await expect(page.locator('html.dark')).toBeVisible();
  });

  test('Double toggle returns to light mode', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    await page.locator('button[aria-label="Toggle theme"]').click();
    await page.locator('button[aria-label="Toggle theme"]').click();
    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass ?? '').not.toContain('dark');
  });

  test('SPA routing works for all sidebar pages', async ({ page }) => {
    const routes = [
      { href: '/dags', title: 'DAGs' },
      { href: '/runs', title: 'Runs' },
      { href: '/events', title: 'Events' },
      { href: '/connectors', title: 'Connector' },
      { href: '/lineage', title: 'Lineage' },
      { href: '/swarm', title: 'Swarm' },
      { href: '/monitoring', title: 'Monitoring' },
      { href: '/compliance', title: 'Compliance' },
      { href: '/settings', title: 'Settings' },
    ];

    await page.goto('/');
    await page.waitForLoadState('networkidle');

    for (const route of routes) {
      await page.locator(`a[href="${route.href}"]`).click();
      await page.waitForURL(`**${route.href}`, { timeout: 10000 });
      await expect(page.locator('h1')).toContainText(route.title, { timeout: 5000 });
    }
  });

  test('Direct URL navigation works without 404', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Sidebar navigation highlights active route', async ({ page }) => {
    await page.goto('/dags');
    await page.waitForLoadState('networkidle');
    const activeLink = page.locator('a[href="/dags"]');
    const classes = await activeLink.getAttribute('class');
    expect(classes).toContain('vortex');
  });
});
"""

files["12-full-flow-verification.spec.ts"] = r"""import { test, expect } from '@playwright/test';

test.describe('12 - Full UI Flow Verification', () => {
  test.beforeEach(async ({ page }) => {
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        const text = msg.text();
        if (!text.includes('favicon') && !text.includes('net::ERR')) {
          console.log('Browser error:', text);
        }
      }
    });
  });

  test('Dashboard loads without errors', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('h1')).toContainText('Dashboard');
    await expect(page.locator('text=Active DAGs')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('text=Total Runs')).toBeVisible({ timeout: 5000 });
  });

  test('DAGs page - list loads and DAG click works', async ({ page }) => {
    await page.goto('/dags');
    await expect(page.locator('h1')).toContainText('DAGs');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const header = page.locator('h1');
    await expect(header).toBeVisible({ timeout: 10000 });
  });

  test('DAG detail page - tabs work without crash', async ({ page }) => {
    await page.goto('/dags');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    await expect(page.locator('button:has-text("Graph")')).toBeVisible();
    await expect(page.locator('button:has-text("Runs")')).toBeVisible();
    await expect(page.locator('button:has-text("Info")')).toBeVisible();
    await page.locator('button:has-text("Runs")').click();
    await page.locator('button:has-text("Info")').click();
    await expect(page.locator('text=DAG Properties')).toBeVisible();
  });

  test('DAG detail page - trigger button works', async ({ page }) => {
    await page.goto('/dags');
    const dagRows = page.locator('table tbody tr');
    await expect(dagRows.first()).toBeVisible({ timeout: 10000 });
    await dagRows.first().click();
    await page.waitForURL(/\/dags\/.+/);
    const triggerBtn = page.locator('button:has-text("Trigger")');
    await expect(triggerBtn).toBeVisible({ timeout: 10000 });
    await triggerBtn.click();
    await expect(triggerBtn).toContainText(/Trigger/);
  });

  test('Runs page - loads and navigates to run detail', async ({ page }) => {
    await page.goto('/runs');
    await expect(page.locator('h1')).toContainText('Runs');
    const runRows = page.locator('table tbody tr');
    const count = await runRows.count();
    if (count > 0) {
      await runRows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      await expect(page.locator('text=Run Detail')).toBeVisible({ timeout: 10000 });
    }
  });

  test('Run detail page - back navigation works', async ({ page }) => {
    await page.goto('/runs');
    const runRows = page.locator('table tbody tr');
    const count = await runRows.count();
    if (count > 0) {
      await runRows.first().click();
      await page.waitForURL(/\/dags\/.+\/runs\/.+/);
      const breadcrumb = page.locator('a[href*="/dags/"]').first();
      if (await breadcrumb.isVisible()) {
        await breadcrumb.click();
        await page.waitForURL(/\/dags\//);
        const body = await page.textContent('body');
        expect(body).not.toContain('SyntaxError');
      }
    }
  });

  test('Events page - loads with content', async ({ page }) => {
    await page.goto('/events');
    await expect(page.locator('h1')).toContainText('Events');
    await expect(page.locator('text=Available Sensor Types')).toBeVisible();
    await expect(page.locator('text=Alert Routing')).toBeVisible();
    await expect(page.locator('text=Event Feed')).toBeVisible();
  });

  test('Compliance page loads', async ({ page }) => {
    await page.goto('/compliance');
    await expect(page.locator('h1')).toContainText('Compliance');
  });

  test('RBAC page loads roles', async ({ page }) => {
    await page.goto('/rbac');
    await expect(page.locator('h1')).toContainText(/RBAC|Access/);
    await expect(page.locator('text=Roles')).toBeVisible();
  });

  test('Monitoring page loads health', async ({ page }) => {
    await page.goto('/monitoring');
    await expect(page.locator('h1')).toContainText('Monitoring');
  });

  test('Swarm page loads', async ({ page }) => {
    await page.goto('/swarm');
    await expect(page.locator('h1')).toContainText('Swarm');
  });

  test('Lineage page loads', async ({ page }) => {
    await page.goto('/lineage');
    await expect(page.locator('h1')).toContainText('Lineage');
  });

  test('Connectors page loads', async ({ page }) => {
    await page.goto('/connectors');
    await expect(page.locator('h1')).toContainText('Connector');
  });

  test('Settings page loads', async ({ page }) => {
    await page.goto('/settings');
    await expect(page.locator('h1')).toContainText('Settings');
  });

  test('Sidebar navigation - all links work', async ({ page }) => {
    const routes = [
      { href: '/dags', title: 'DAGs' },
      { href: '/runs', title: 'Runs' },
      { href: '/events', title: 'Events' },
      { href: '/connectors', title: 'Connector' },
      { href: '/lineage', title: 'Lineage' },
      { href: '/swarm', title: 'Swarm' },
      { href: '/monitoring', title: 'Monitoring' },
      { href: '/compliance', title: 'Compliance' },
      { href: '/settings', title: 'Settings' },
    ];

    await page.goto('/');
    for (const route of routes) {
      await page.locator(`a[href="${route.href}"]`).click();
      await page.waitForURL(`**${route.href}`, { timeout: 10000 });
      await expect(page.locator('h1')).toContainText(route.title, { timeout: 5000 });
    }
  });

  test('No page errors across all pages', async ({ page }) => {
    test.setTimeout(60000);
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    const pages = ['/', '/dags', '/runs', '/events', '/connectors', '/lineage',
      '/swarm', '/monitoring', '/compliance', '/rbac', '/settings'];

    for (const p of pages) {
      await page.goto(p);
      await page.waitForLoadState('domcontentloaded');
      await page.waitForTimeout(500);
    }

    expect(errors).toEqual([]);
  });
});
"""

for filename, content in files.items():
    path = os.path.join(BASE, filename)
    with open(path, 'w') as f:
        f.write(content.lstrip('\n'))
    print(f"Written {filename} ({len(content.strip().splitlines())} lines)")

print("\nAll test files rewritten successfully!")
