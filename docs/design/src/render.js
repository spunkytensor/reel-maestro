// Render every mockup to PNG with an exact viewport (Playwright-core + the cached Chromium).
// Usage: npm install && node render.js            (writes ../NN-name.png)
const { chromium } = require('playwright-core');
const path = require('path');
const os = require('os');

const shots = [
  // name, file, query, width, height, scale, fullPage
  ['01-start-light',            'start.html',    'theme=light',                 1440, 900],
  ['02-start-dark',             'start.html',    'theme=dark',                  1440, 900],
  ['03-start-customize-dark',   'start.html',    'theme=dark&customize=1',      1440, 900],
  ['04-projects-light',         'projects.html', 'theme=light',                 1440, 900],
  ['05-projects-dark',          'projects.html', 'theme=dark',                  1440, 900],
  ['06-editor-light',           'editor.html',   'theme=light',                 1440, 900],
  ['07-editor-dark-drafts',     'editor.html',   'theme=dark&drafts=1',         1440, 900],
  ['08-scene-customize-dark',   'editor.html',   'theme=dark&drafts=1&scene=1', 1440, 900],
  ['09-review-changes-dark',    'editor.html',   'theme=dark&drafts=1&sheet=review', 1440, 900],
  ['10-applying-activity-light','editor.html',   'theme=light&applying=1&activity=1', 1440, 900],
  ['11-export-light',           'editor.html',   'theme=light&panel=export',    1440, 900],
  ['12-exported-dark',          'editor.html',   'theme=dark&exported=1',       1440, 900],
  ['13-settings-light',         'settings.html', 'theme=light',                 1440, 900],
  ['14-settings-dark-solid',    'settings.html', 'theme=dark&solid=1',          1440, 900],
  ['15-mobile-start-light',     'mobile.html',   'theme=light&view=start',      390, 844, 3],
  ['16-mobile-editor-dark',     'mobile.html',   'theme=dark&view=editor',      390, 844, 3],
  ['17-mobile-exported-dark',   'mobile.html',   'theme=dark&view=exported',    390, 844, 3],
  ['18-materials',              'materials.html','theme=light',                 1440, 1100, 2, true],
];

(async () => {
  const only = process.argv[2];
  const executablePath = process.env.CHROME || path.join(os.homedir(), '.cache/ms-playwright/chromium-1200/chrome-linux64/chrome');
  const browser = await chromium.launch({ executablePath, args: ['--no-sandbox'] });
  for (const [name, file, q, w, h, scale = 2, fullPage = false] of shots) {
    if (only && !name.includes(only)) continue;
    const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: scale, colorScheme: q.includes('theme=dark') ? 'dark' : 'light' });
    const page = await ctx.newPage();
    await page.goto('file://' + path.resolve(__dirname, file) + '?' + q);
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(250);
    await page.screenshot({ path: path.resolve(__dirname, '..', name + '.png'), fullPage });
    await ctx.close();
    console.log('rendered', name);
  }
  await browser.close();
})();
