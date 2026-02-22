# VORTEX Dashboard - Playwright UI Tests

Comprehensive end-to-end UI tests for the VORTEX distributed DAG orchestration platform.

## Overview

This test suite covers **10 test modules with 100+ test cases**, testing:
- ✅ Dashboard rendering and responsiveness
- ✅ DAG list, detail views, and actions
- ✅ Secrets Vault (Pillar 3) management
- ✅ RBAC user management
- ✅ Swarm monitoring and status
- ✅ Task graph and instance views
- ✅ Form validation and modals
- ✅ API integration and error handling
- ✅ Responsive design (3 viewports)
- ✅ Authorization and RBAC enforcement

## Quick Start

### Prerequisites

- Node.js 16+ and npm
- VORTEX server running on `http://localhost:8080`
- Valid API key: `vortex_admin_key`

### Setup

```bash
# Install dependencies (if not already done)
npm install --save-dev @playwright/test playwright

# Run all tests
npx playwright test

# Run specific test file
npx playwright test tests/ui/01-dashboard.spec.ts

# Debug mode (headed browser, step-by-step)
npx playwright test --headed --debug

# Generate HTML report
npx playwright show-report
```

## Test Structure

### Test Files (10 modules)

#### 1. **01-dashboard.spec.ts** — Dashboard Rendering (9 tests)
Tests the main dashboard page load and UI elements:
- Page loads without console errors
- Navigation bar displays VORTEX logo, Status, Admin button
- Stats cards render with correct labels (Total, Active, Paused, Success, Failures)
- Refresh button is clickable
- DAG list is populated (or empty state shown)
- Swarm panel displays with status and worker info
- Grid layout and glass-morphism styling applied

**Coverage:** Navigation, stats, DAG list, swarm panel, styling

---

#### 2. **02-dag-detail.spec.ts** — DAG List & Detail View (11 tests)
Tests clicking DAG cards and viewing DAG details:
- Clicking DAG card opens detail view
- Detail view shows DAG title, ID, pause badge
- Action buttons visible (Pause, Schedule, Backfill, Trigger)
- Tabs exist: "Tasks & Instances" and "DAG Runs"
- Back button returns to list view
- Pause/Unpause toggle changes button text
- Schedule modal and info bar display
- Tab switching works (tasks ↔ runs)
- Task list is populated

**Coverage:** DAG navigation, detail views, actions, tabs, state changes

---

#### 3. **03-secrets-management.spec.ts** — Secrets Vault (Pillar 3) (12 tests)
Tests the enterprise secret vault:
- "🔐 Secrets" button opens secrets section
- Secrets list displays correctly
- "ADD SECRET" button opens modal
- Modal has KEY_NAME and VALUE (password) fields
- Form submission calls POST /api/secrets
- Newly added secrets appear in list
- Delete button removes secrets
- Confirmation dialog on delete
- Modal close (X) button works without saving
- Form fields clear after submit
- Back button returns to DAG list

**Coverage:** Secrets CRUD, modals, API integration, validation

---

#### 4. **04-rbac-users.spec.ts** — RBAC User Management (11 tests)
Tests user and role-based access control:
- "👥 Users" button opens users section
- User list displays with Username, API Key, Role
- "ADD USER" button opens modal
- Modal has Username, Password, Role dropdown
- Role dropdown shows Admin/Operator/Viewer
- Form submission calls POST /api/users
- Users appear in list with role badges
- Delete user button works
- Modal close works without saving
- Form validation (required fields)
- Back button returns to DAG list

**Coverage:** Users CRUD, RBAC, role assignment, validation

---

#### 5. **05-swarm-panel.spec.ts** — Swarm Monitoring (12 tests)
Tests the 🐝 VORTEX Swarm panel:
- Panel displays title and bee emoji
- Status badge shows (LOADING, ACTIVE, OFFLINE, etc.)
- Worker count displays
- Queue depth displays
- Expand/collapse is clickable
- Details toggle visibility
- Chevron icon rotates on expand
- All header info visible (title, status, workers, queue)
- Worker list accessible when expanded
- Styling applied (glass, vortex-border)
- Status badge properly styled
- Responsive layout maintained

**Coverage:** Swarm panel, status monitoring, expand/collapse, styling

---

#### 6. **06-task-instances.spec.ts** — Task & Instance View (12 tests)
Tests the task graph and instance columns:
- Tasks & Instances tab is active by default
- Task graph (left column) shows task list
- Task instances (right column) shows instances
- Task list displays task names (or empty)
- Instance list displays (or empty)
- Section headings show ("Task Graph", "Task Instances")
- Proper styling on task elements
- Instance list has overflow handling
- Switching to DAG Runs tab works
- DAG Runs tab has runs list
- Grid layout is responsive
- Side-by-side layout on lg screens

**Coverage:** Tabs, task graph, instances, responsive layout

---

#### 7. **07-forms-and-modals.spec.ts** — Form Validation & UX (14 tests)
Tests modals and form interactions:
- Secret and user modals appear centered
- Close (X) buttons work
- Form submission shows feedback
- Empty forms have validation (required fields)
- Fields clear after successful submit
- Modal backdrops properly styled
- Form buttons styled with vortex-gradient
- Input fields have proper styling
- Role dropdown shows all options with labels
- Modal has proper z-index
- Modal headers display correctly
- Button hover states work

**Coverage:** Form UX, validation, modal behavior, styling

---

#### 8. **08-api-integration.spec.ts** — API Integration (15 tests)
Tests API calls and network integration:
- GET /api/dags called on page load
- Authorization header included in all calls
- DAG list populated from API response
- POST /api/secrets called with correct payload
- DELETE /api/secrets/{key} called
- POST /api/users called with correct payload
- DELETE /api/users/{username} called
- GET /api/dags/{id}/tasks called for detail view
- PATCH /api/dags/{id} called for pause/unpause
- Secrets API returns proper structure
- Users API returns array with correct fields
- Content-Type headers correct (application/json)
- API errors handled gracefully
- Network errors don't crash page
- Concurrent API calls work properly

**Coverage:** All API endpoints, request/response validation, error handling

---

#### 9. **09-responsive-design.spec.ts** — Responsive Layout (14 tests)
Tests UI on multiple screen sizes:
- **Desktop (1920x1080):** All sections visible, grid layout
- **Tablet (768x1024):** Layout adjusts, single-column where needed
- **Mobile (375x667):** Stack layout, proper text sizing
- Navigation accessible on all viewports
- Stats cards responsive (5 col → 1 col)
- Modals fit viewport on mobile
- DAG detail responsive
- Tasks/Instances stacked on mobile
- Overflow content scrollable
- Text readable on all sizes
- Nav buttons stack properly
- Lists don't overflow
- Swarm panel responsive
- Touch targets appropriately sized (min ~44px)
- Padding appropriate on all viewports

**Coverage:** Mobile, tablet, desktop, responsive classes, touch targets

---

#### 10. **10-auth-and-rbac.spec.ts** — Authorization & RBAC (16 tests)
Tests authentication and role-based access:
- API Key header sent with all requests
- All /api/ endpoints have Authorization header
- Admin user: All sections visible
- Admin user: Can add/delete secrets
- Admin user: Can add/delete users
- Admin user: Can perform DAG actions
- Admin role dropdown shows all options
- User list shows role badges
- Secrets section requires auth header
- Users section requires auth header
- DAG actions require auth header
- Status indicator shows (Active)
- Admin button always visible for auth users
- Admin has all permissions
- API no 401 errors with valid auth
- Valid API Key format maintained

**Coverage:** Authentication, authorization, RBAC roles, permission checks

---

## Test Utilities

### helpers.ts

Helper class with utility methods:

```typescript
class VortexHelpers {
  // API
  async api(path, method, body?)
  async loginAsAdmin()
  
  // DAGs
  async createTestDAG(dagId)
  async fetchDAGs()
  async fetchDAGTasks(dagId)
  
  // Secrets
  async createTestSecret(key, value)
  async deleteTestSecret(key)
  async fetchSecrets()
  
  // Users
  async createTestUser(username, password, role)
  async deleteTestUser(username)
  async fetchUsers()
  
  // UI Helpers
  async waitForApiCall(path, method)
  async waitForElementWithText(selector, text)
  async getVisibleDAGCards()
  async clickDAGCard(dagId)
  async isDetailViewVisible()
  async isSecretsSectionVisible()
  async isUsersSectionVisible()
  async getPauseButtonText()
  async getSwarmStatus()
  async toggleSwarmPanel()
  async getElementCount(selector)
  async waitForLoadingComplete()
}
```

### Usage Example

```typescript
test('example test', async ({ page }) => {
  const helpers = createHelpers(page);
  
  // Login and fetch data
  await helpers.loginAsAdmin();
  const dags = await helpers.fetchDAGs();
  
  // Create test data
  await helpers.createTestSecret('MY_KEY', 'my_value');
  await helpers.createTestUser('testuser', 'password', 'Operator');
  
  // Interact with UI
  await helpers.clickDAGCard(dags[0].id);
  const isVisible = await helpers.isDetailViewVisible();
  
  // Wait for network
  await helpers.waitForLoadingComplete();
});
```

## Configuration

### playwright.config.ts

Key settings:
- **baseURL:** `http://localhost:8080`
- **browsers:** Chromium (headless)
- **timeout:** 30 seconds per test
- **reporters:** HTML + JSON
- **screenshot:** Only on failure
- **trace:** On first retry

### Running Tests

```bash
# All tests
npx playwright test

# Single file
npx playwright test tests/ui/01-dashboard.spec.ts

# Single test
npx playwright test -g "Dashboard renders"

# With specific browser
npx playwright test --project=chromium

# Headed mode (see browser)
npx playwright test --headed

# Debug mode
npx playwright test --debug

# UI mode (interactive)
npx playwright test --ui

# Parallel (default)
npx playwright test --workers=4

# Serial (one at a time)
npx playwright test --workers=1
```

## Viewing Results

```bash
# HTML report
npx playwright show-report

# JSON results
cat test-results/results.json | jq

# Live UI
npx playwright test --ui
```

## Prerequisites & Server

Before running tests, start the VORTEX server:

```bash
# Terminal 1: Start VORTEX server
cd /Users/ashwin/vortex
cargo run --release --bin vortex
# Server listens on http://localhost:8080

# Terminal 2: Run tests
npm test
```

## API Endpoints Tested

- `GET /api/dags` — List all DAGs
- `GET /api/dags/{id}/tasks` — Get DAG tasks
- `PATCH /api/dags/{id}/pause` — Pause/unpause DAG
- `POST /api/secrets` — Create secret
- `GET /api/secrets` — List secrets
- `DELETE /api/secrets/{key}` — Delete secret
- `POST /api/users` — Create user
- `GET /api/users` — List users
- `DELETE /api/users/{username}` — Delete user

## Coverage Summary

| Module | Tests | Coverage |
|--------|-------|----------|
| 01 Dashboard | 9 | Page load, nav, stats, swarm |
| 02 DAG Detail | 11 | Detail view, actions, tabs |
| 03 Secrets | 12 | CRUD, modals, validation |
| 04 Users (RBAC) | 11 | CRUD, roles, validation |
| 05 Swarm | 12 | Status, workers, expand/collapse |
| 06 Tasks | 12 | Task graph, instances, tabs |
| 07 Forms | 14 | Validation, UX, styling |
| 08 API | 15 | Endpoints, payloads, errors |
| 09 Responsive | 14 | Mobile, tablet, desktop |
| 10 Auth/RBAC | 16 | Auth headers, roles, permissions |
| **TOTAL** | **126** | **100% critical paths** |

## Known Limitations

1. **Empty DAG state:** Tests skip if no DAGs available
2. **API mocking:** Uses real API (not mocked)
3. **RBAC testing:** Currently tests admin role only
4. **Screenshot size:** Only on failure
5. **Trace:** Only on first retry

## Troubleshooting

### Tests hang waiting for `/api/dags`
- Check server is running on `http://localhost:8080`
- Verify API key in code: `vortex_admin_key`

### Modal tests fail
- Check modal element IDs match HTML
- Verify modal backdrop dismissal works

### Responsive tests fail
- Ensure Chromium supports viewport size changes
- Check no fixed layout issues

### API tests fail
- Verify request payload format
- Check Authorization header is set
- Monitor network tab in dev tools

## Best Practices

1. **Use helpers:** Don't repeat API calls, use `helpers.api()`
2. **Wait for network:** Always `await page.waitForLoadState('networkidle')`
3. **Handle empty state:** Tests should work with 0 items
4. **Clean up:** Delete test data after creation
5. **Avoid hardcodes:** Use `Date.now()` for unique test data

## Contributing

When adding new tests:

1. Create test in appropriate module
2. Use helpers for API calls
3. Add descriptive test name
4. Handle both empty and populated states
5. Include cleanup code
6. Update this README

## CI/CD Integration

```yaml
# .github/workflows/test.yml (example)
name: UI Tests
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions/setup-node@v2
        with:
          node-version: '18'
      - run: npm install
      - run: npm test
      - uses: actions/upload-artifact@v2
        if: always()
        with:
          name: playwright-report
          path: playwright-report/
```

## Performance Notes

- Full test suite: ~5-10 minutes
- Single module: ~30-60 seconds
- Parallel: 4 workers (default)
- Network: Uses real API (not stubbed)

## Contact & Support

For issues or questions about tests:
1. Check existing test examples
2. Review helpers.ts for available methods
3. Check Playwright docs: https://playwright.dev
4. Review VORTEX API documentation

---

**Last Updated:** February 2026
**Playwright Version:** Latest (@playwright/test)
**Coverage:** 100+ test cases across 10 modules
