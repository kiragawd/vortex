const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

(async () => {
  const outDir = '/Users/Ashwin/Desktop/vortex_screenshots';
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1366, height: 900 } });

  const base = 'http://localhost:3000';
  console.log('Opening', base);
  await page.goto(base, { waitUntil: 'networkidle' });

  // Try several common selectors to log in
  const creds = { username: 'admin', password: 'admin' };
  const loginSelectors = [
    { user: 'input[name="username"]', pass: 'input[name="password"]', submit: 'button[type="submit"]' },
    { user: 'input[name="email"]', pass: 'input[name="password"]', submit: 'button[type="submit"]' },
    { user: 'input#username', pass: 'input#password', submit: 'button[type="submit"]' },
    { user: 'input[type="text"]', pass: 'input[type="password"]', submit: 'button[type="submit"]' }
  ];

  let loggedIn = false;
  for (const s of loginSelectors) {
    try {
      const hasUser = await page.$(s.user);
      const hasPass = await page.$(s.pass);
      if (!hasUser || !hasPass) continue;
      await page.fill(s.user, creds.username);
      await page.fill(s.pass, creds.password);
      // try submit
      const btn = await page.$(s.submit);
      if (btn) {
        await Promise.all([page.waitForNavigation({ waitUntil: 'networkidle', timeout: 5000 }).catch(()=>{}), btn.click()]);
      } else {
        await page.keyboard.press('Enter');
        await page.waitForTimeout(1000);
      }
      // crude check for login success: presence of logout link or dashboard element
      const logout = await page.$('a[href*="logout"], button[aria-label="logout"], text=Logout');
      if (logout) { loggedIn = true; break; }
      // or check if path changed
      if (page.url() !== base) { loggedIn = true; break; }
    } catch (e) {
      // ignore and try next
    }
  }

  // take screenshot of landing (post-login or login page)
  const timestamp = Date.now();
  const firstShot = path.join(outDir, `page-0-home-${timestamp}.png`);
  await page.screenshot({ path: firstShot, fullPage: true });
  console.log('Saved', firstShot);

  // collect internal links from the nav and page
  const hrefs = await page.$$eval('a[href^="/"], a[href^="./"], a[href^="#"]', els => els.map(e => e.getAttribute('href')));
  const unique = Array.from(new Set(hrefs))
    .filter(h => h && h !== '#' && !h.toLowerCase().includes('logout') && !h.startsWith('mailto:'))
    .map(h => h.startsWith('./') ? h.slice(1) : h);

  // limit to reasonable number
  const pagesToVisit = unique.slice(0, 40);
  console.log('Found', pagesToVisit.length, 'internal links to capture');

  let idx = 1;
  for (const href of pagesToVisit) {
    try {
      const url = href.startsWith('/') ? base + href : (href.startsWith('#') ? base : base + href);
      console.log('Visiting', url);
      await page.goto(url, { waitUntil: 'networkidle', timeout: 15000 });
      await page.waitForTimeout(500); // let dynamic UI settle
      const name = href.replace(/[^a-z0-9\-_.]/ig, '_').slice(0, 80) || `page_${idx}`;
      const out = path.join(outDir, `page-${idx}-${name}-${timestamp}.png`);
      await page.screenshot({ path: out, fullPage: true });
      console.log('Saved', out);
      idx++;
    } catch (e) {
      console.log('Skip', href, e.message);
    }
  }

  await browser.close();
  console.log('Done. Screenshots in', outDir);
})();
