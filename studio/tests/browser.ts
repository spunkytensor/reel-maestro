// Offline end-to-end verification against the real Studio server and bundled web app.
// Paid generation uses a fixture; credential-free local revisions use the real Rust CLI.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { chromium, type Page } from "playwright-core";
import { createApp } from "../server/app";
import { verifyNativeRevision } from "./revision-browser";

const root = fileURLToPath(new URL("../../", import.meta.url));
const temporary = await mkdtemp(path.join(os.tmpdir(), "reelmaestro-browser-"));
const outDir = path.join(temporary, "out");
const assets = path.join(root, "docs/design/src/assets");
const captures = path.join(root, "out/studio-verification");
const command = promisify(execFile);
await mkdir(outDir);
await mkdir(captures, { recursive: true });
const words = [
  "It began with a simple idea: a better cup of coffee.",
  "In Turin, steam changed everything.",
  "A new machine made coffee in moments.",
  "Espresso brought the city together.",
  "Small cups became a daily ritual.",
  "The craft spread across the world.",
  "Every detail matters, from bean to cup.",
  "And the story is still being written.",
];

async function fixture(
  name: string,
  title: string,
  prefix: string,
  format: "youtube" | "reel",
) {
  const dir = path.join(outDir, name);
  await mkdir(dir);
  const scenes = words.map((line, index) => ({
    line,
    image_prompt: `A cinematic image of the story, scene ${index + 1}`,
    motion_prompt: "A slow, gentle camera move.",
  }));
  await writeFile(
    path.join(dir, "script.json"),
    JSON.stringify({
      title,
      format,
      narration: words.join(" "),
      scenes,
      music_prompt: "A warm, understated instrumental.",
      chapters: [],
    }),
  );
  await copyFile(
    path.join(assets, `${prefix}-poster.jpg`),
    path.join(dir, "poster.jpg"),
  );
  for (let i = 0; i < scenes.length; i++)
    await copyFile(
      path.join(
        assets,
        `${prefix}-${String(i % (prefix === "cre" ? 6 : 8)).padStart(2, "0")}.jpg`,
      ),
      path.join(dir, `scene-${String(i).padStart(2, "0")}.jpg`),
    );
  await writeFile(
    path.join(dir, "captions.ass"),
    "[Script Info]\nTitle: Offline fixture\n",
  );
  await writeFile(
    path.join(dir, "youtube.md"),
    `# ${title}\n\nAn offline Studio verification fixture.\n`,
  );
  const geometry = format === "youtube" ? "640:360" : "360:640";
  await command("ffmpeg", [
    "-v",
    "error",
    "-loop",
    "1",
    "-i",
    path.join(dir, "scene-00.jpg"),
    "-t",
    "4",
    "-vf",
    `scale=${geometry}:force_original_aspect_ratio=decrease,pad=${geometry}:(ow-iw)/2:(oh-ih)/2`,
    "-r",
    "24",
    "-c:v",
    "libx264",
    "-pix_fmt",
    "yuv420p",
    "-movflags",
    "+faststart",
    path.join(dir, "reel.mp4"),
  ]);
  return dir;
}
const espresso = await fixture(
  "espresso",
  "The history of espresso",
  "esp",
  "youtube",
);
await fixture("creatine", "Creatine, explained simply", "cre", "reel");

const log = path.join(temporary, "calls.jsonl");
const binary = path.join(temporary, "fixture.cjs");
await writeFile(
  binary,
  `#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const args = process.argv.slice(2);
if (args.includes('--studio-inspect-run')) {
  const source = args[args.indexOf('--studio-inspect-run')+1];
  const bytes = fs.readFileSync(path.join(source,'script.json'));
  const script = JSON.parse(bytes);
  script.scenes = script.scenes.map((scene,index) => ({...scene,id:scene.id || 'fixture-scene-'+index}));
  console.log(JSON.stringify({version:1,source_fingerprint:'sha256:'+crypto.createHash('sha256').update(bytes).digest('hex'),script}));
  process.exit(0);
}
if (args.includes('--studio-schema')) { console.log(JSON.stringify({version:1, arguments:[], availability:{editing:false, revision_execution:false}})); process.exit(0); }
if (args.includes('--studio-estimate')) { fs.appendFileSync(${JSON.stringify(log)}, 'estimate\\n'); console.log(JSON.stringify({version:1,total_usd:.25,script_usd:.05,narration_usd:.05,images_usd:.15,video_usd:0,music_usd:0,warning:'Offline fixture estimate.'})); process.exit(0); }
fs.appendFileSync(${JSON.stringify(log)}, 'generate\\n');
const prompt = args[args.indexOf('--topic')+1] || '';
setTimeout(() => {
  if (prompt.includes('failure')) process.exit(1);
  const dir = path.join(args[args.indexOf('--out')+1], 'fixture-result');
  fs.cpSync(${JSON.stringify(espresso)}, dir, {recursive:true});
}, prompt.includes('cancel') ? 15000 : 2000);
`,
  { mode: 0o700 },
);
await chmod(binary, 0o700);

const port = 3197;
const app = await createApp({
  root: temporary,
  outDir,
  stateDir: path.join(temporary, "state"),
  binary,
  webDir: path.join(root, "studio/web/dist"),
  port,
  urlFetcher: async (url) => ({
    url,
    title: "Offline article",
    text: "A checked offline article about coffee.",
  }),
  env: {
    PATH: process.env.PATH,
    HOME: temporary,
    OPENROUTER_API_KEY: "offline-fixture-not-a-real-key",
  },
  terminateGraceMs: 50,
});
await app.listen({ host: "127.0.0.1", port });
const executablePath =
  process.env.CHROME ??
  (existsSync(chromium.executablePath())
    ? chromium.executablePath()
    : path.join(
        os.homedir(),
        ".cache/ms-playwright/chromium-1200/chrome-linux64/chrome",
      ));
const browser = await chromium.launch({
  executablePath,
  args: ["--no-sandbox"],
});
const failures: string[] = [];
async function screenshot(page: Page, name: string) {
  await page.waitForFunction(() => {
    const logo = document.querySelector<HTMLImageElement>(".corner-logo");
    return logo?.complete && logo.naturalWidth > 0;
  });
  assert.equal(
    await page
      .locator("header")
      .getByText("by Spunky Tensor", { exact: true })
      .count(),
    0,
  );
  assert.equal(
    await page.locator(".corner-logo").evaluate((logo) => {
      const box = logo.getBoundingClientRect();
      const action = document
        .querySelector(".mobile-primary")
        ?.getBoundingClientRect();
      return (
        box.right <= innerWidth &&
        Math.abs(box.bottom - (innerHeight - 14)) < 1 &&
        (!action ||
          action.width === 0 ||
          action.right <= box.left ||
          action.bottom <= box.top ||
          action.top >= box.bottom)
      );
    }),
    true,
    `${name}: corner logo fits without covering primary action`,
  );
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
  });
  await page.screenshot({
    path: path.join(captures, `${name}.png`),
    fullPage: true,
  });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
    `${name}: horizontal overflow`,
  );
  assert.ok(
    (await page.locator(".btn.primary:visible").count()) <= 1,
    `${name}: multiple primary actions`,
  );
  if (
    (page.viewportSize()?.width ?? 0) >= 1024 &&
    (await page.locator(".track").count()) &&
    !(await page.locator(".overlay.sheet").count())
  ) {
    assert.equal(
      await page.locator(".track").evaluate((element) => {
        const last = element.lastElementChild?.getBoundingClientRect();
        return (
          !!last && last.right <= element.getBoundingClientRect().right + 1
        );
      }),
      true,
      `${name}: final timeline thumbnail fits`,
    );
  }
  if (
    (page.viewportSize()?.width ?? 1440) <= 600 &&
    (await page.locator(".mobile-primary:visible").count())
  ) {
    const action = await page
      .locator(".mobile-primary:visible")
      .first()
      .boundingBox();
    assert.ok(
      action &&
        Math.abs(
          action.y + action.height - ((page.viewportSize()?.height ?? 0) - 12),
        ) < 2,
      `${name}: primary must be pinned to viewport bottom`,
    );
  }
}
async function open(page: Page, route: string) {
  await page.goto(`http://127.0.0.1:${port}/#${route}`);
  await page.getByRole("link", { name: "Reel Maestro home" }).waitFor();
  await page.getByRole("heading", { level: 1 }).first().waitFor();
}

try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    colorScheme: "light",
    reducedMotion: "reduce",
    acceptDownloads: true,
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => failures.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") failures.push(message.text());
  });
  await open(page, "new");
  await page.getByText("Use a text file", { exact: true }).click();
  await page.getByLabel("Script or brief", { exact: true }).setInputFiles({
    name: "brief.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("An uploaded offline script about coffee."),
  });
  await page.getByText("brief.txt", { exact: true }).waitFor();
  assert.equal(
    await page.getByLabel("Describe your video").inputValue(),
    "An uploaded offline script about coffee.",
  );
  await page.getByRole("button", { name: "Customize", exact: true }).click();
  await page.getByRole("radio", { name: "Link", exact: true }).click();
  await page.keyboard.press("Escape");
  await page
    .getByLabel("Describe your video")
    .fill("https://example.com/offline-article");
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page.getByRole("dialog", { name: "Generate video" }).waitFor();
  await page.keyboard.press("Escape");
  assert.equal(
    await page.getByLabel("Describe your video").inputValue(),
    "A checked offline article about coffee.",
  );
  await page
    .getByLabel("Describe your video")
    .fill(
      "The history of espresso — how a rushed Italian invention became the world’s favourite coffee",
    );
  await page.getByRole("radio", { name: "Widescreen", exact: true }).click();
  await screenshot(page, "start-light");
  await page.getByRole("button", { name: "Customize", exact: true }).click();
  await page.getByRole("dialog", { name: "Customize", exact: true }).waitFor();
  await screenshot(page, "customize-light");
  await page.keyboard.press("Escape");
  assert.equal(
    await page
      .getByRole("button", { name: "Customize", exact: true })
      .evaluate((element) => element === document.activeElement),
    true,
    "Escape restores focus",
  );
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  const review = page.getByRole("dialog", { name: "Generate video" });
  await review.waitFor();
  assert.equal(
    (await readFile(log, "utf8")).includes("generate"),
    false,
    "estimate must not launch generation",
  );
  await screenshot(page, "review-light");
  await review.getByRole("button", { name: "Cancel", exact: true }).click();

  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Your videos" }).waitFor();
  await screenshot(page, "projects-light");
  await page
    .getByRole("searchbox", { name: "Search videos" })
    .fill("not present");
  await page
    .getByText("No videos match your search.", { exact: false })
    .waitFor();
  await page.getByRole("searchbox", { name: "Search videos" }).fill("");
  await page.getByRole("link", { name: /The history of espresso/ }).click();
  await page.getByRole("button", { name: "Play video", exact: true }).waitFor();
  await page.waitForFunction(
    () => (document.querySelector("video")?.duration ?? 0) > 0,
  );
  await screenshot(page, "editor-light");
  await page.getByRole("button", { name: "Play video", exact: true }).click();
  await page.waitForFunction(
    () => (document.querySelector("video")?.currentTime ?? 0) > 0.1,
  );
  await page.getByRole("button", { name: "Pause video", exact: true }).click();
  await page.getByRole("option").first().focus();
  await page.keyboard.press("ArrowDown");
  assert.equal(
    await page.getByRole("option").nth(1).getAttribute("aria-selected"),
    "true",
  );
  await page.getByRole("button", { name: "Customize scene" }).click();
  await screenshot(page, "scene-light");
  const originalImage = await page
    .getByLabel("Image", { exact: true })
    .inputValue();
  await page
    .getByLabel("Image", { exact: true })
    .fill("A new close-up of an espresso machine, warm morning light");
  await page.getByLabel("Current image", { exact: true }).selectOption("keep");
  await page.getByRole("switch", { name: "Animate this scene" }).click();
  await screenshot(page, "scene-edited-light");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /Apply \d+ changes/ }).waitFor();
  await page
    .getByRole("button", { name: "Undo", exact: true })
    .filter({ visible: true })
    .click();
  await page
    .getByRole("button", { name: "Redo", exact: true })
    .filter({ visible: true })
    .click();
  await page.reload();
  await page.getByRole("button", { name: /Apply \d+ changes/ }).waitFor();
  await page
    .getByRole("listbox", { name: "Scenes" })
    .getByRole("option")
    .nth(1)
    .click();
  await page.getByRole("button", { name: "Customize scene" }).click();
  assert.notEqual(
    await page.getByLabel("Image", { exact: true }).inputValue(),
    originalImage,
    "scene draft survives reload",
  );
  assert.equal(
    await page.getByLabel("Current image", { exact: true }).inputValue(),
    "keep",
  );
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Customize video" }).click();
  await page.getByRole("button", { name: "Discard unapplied changes" }).click();
  await screenshot(page, "video-customize-light");
  await page.keyboard.press("Escape");
  await page
    .getByRole("listbox", { name: "Scenes" })
    .getByRole("option")
    .first()
    .focus();
  await page.keyboard.press("Control+ArrowDown");
  await page
    .getByRole("button", { name: "Undo", exact: true })
    .filter({ visible: true })
    .click();
  assert.equal(
    (await readFile(path.join(espresso, "script.json"), "utf8")).includes(
      "A new close-up",
    ),
    false,
    "draft edits never rewrite originals",
  );
  await page.getByRole("button", { name: "Export MP4", exact: true }).click();
  await screenshot(page, "export-options-light");
  await page
    .getByRole("button", { name: "Download current MP4", exact: true })
    .click();
  await screenshot(page, "download-light");
  const download = page.waitForEvent("download");
  await page.getByRole("link", { name: "Download MP4", exact: true }).click();
  assert.ok((await download).suggestedFilename().endsWith(".mp4"));
  await page.keyboard.press("Escape");

  await page.getByRole("link", { name: "Back to projects" }).click();
  await page.getByRole("link", { name: "Settings", exact: true }).click();
  for (const label of [
    "AI provider",
    "Reduce transparency",
    "Reduce motion",
    "Existing videos",
    "Export watermark",
    "Versions",
  ]) {
    assert.equal(
      await page.getByText(label, { exact: true }).count(),
      0,
      `${label} removed from Settings`,
    );
  }
  await screenshot(page, "settings-light");
  await page.getByRole("radio", { name: "Dark", exact: true }).click();
  await screenshot(page, "settings-dark");
  await page.setViewportSize({ width: 390, height: 844 });
  await screenshot(page, "settings-mobile-dark");
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() =>
    localStorage.setItem(
      "reelmaestro.appearance",
      JSON.stringify({ theme: "dark", solid: true, motion: true }),
    ),
  );
  await page.reload();
  await page.getByRole("heading", { name: "Settings", exact: true }).waitFor();
  assert.equal(await page.locator("html").getAttribute("data-theme"), "dark");
  assert.equal(
    await page.locator("html").getAttribute("data-solid"),
    "0",
    "removed stored overrides do not affect appearance",
  );
  assert.equal(
    await page.locator("html").getAttribute("data-motion"),
    "reduce",
    "OS reduced-motion preference is respected",
  );
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.waitForFunction(
    () => document.documentElement.dataset.motion === "normal",
  );
  await page.getByRole("radio", { name: "System", exact: true }).click();
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForFunction(
    () => document.documentElement.dataset.theme === "dark",
  );
  await page.getByRole("radio", { name: "Dark", exact: true }).click();
  await page.getByRole("link", { name: "Reel Maestro home" }).click();
  await page
    .getByLabel("Describe your video")
    .fill("An offline espresso fixture");
  await screenshot(page, "start-dark");
  await page.getByRole("button", { name: "Customize", exact: true }).click();
  await page.getByLabel("Spending limit in dollars", { exact: true }).fill("0");
  await page.keyboard.press("Escape");
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page.getByRole("dialog", { name: "Generate video" }).waitFor();
  assert.equal(
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Generate video", exact: true })
      .isDisabled(),
    true,
  );
  await screenshot(page, "over-budget-dark");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Customize", exact: true }).click();
  await page.getByLabel("Spending limit in dollars", { exact: true }).fill("5");
  await page.keyboard.press("Escape");
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page.getByRole("dialog", { name: "Activity", exact: true }).waitFor();
  await screenshot(page, "activity-dark");
  await page.waitForURL(/#video\//);
  assert.equal(
    (await readFile(log, "utf8"))
      .split("\n")
      .filter((line) => line === "generate").length,
    1,
    "one approved generation",
  );
  await page.getByRole("button", { name: "Play video", exact: true }).waitFor();
  await screenshot(page, "editor-dark");
  const generatedUrl = page.url();

  await page.getByRole("link", { name: "Back to projects" }).click();
  await page.getByRole("link", { name: "Reel Maestro home" }).click();
  await page.getByLabel("Describe your video").fill("offline failure fixture");
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Activity", exact: true })
    .getByText("Needs attention", { exact: true })
    .waitFor();
  await screenshot(page, "failed-activity-dark");
  await page.keyboard.press("Escape");

  await page.getByLabel("Describe your video").fill("offline cancel fixture");
  await page
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Generate video", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Activity", exact: true })
    .getByText("Generating", { exact: true })
    .waitFor();
  await page.reload();
  await page
    .getByRole("button", { name: "1 in progress", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByText("Stopped", { exact: true })
    .waitFor();
  await screenshot(page, "cancelled-activity-dark");
  await page.keyboard.press("Escape");
  await page.goto(generatedUrl);
  await page.getByRole("button", { name: "Play video", exact: true }).waitFor();

  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 });
    await page.getByRole("link", { name: "Back to projects" }).click();
    await page
      .getByRole("link", { name: /Creatine, explained simply/ })
      .click();
    await page
      .getByRole("button", { name: "Play video", exact: true })
      .waitFor();
    await screenshot(page, `editor-mobile-${width}`);
    await page.getByRole("button", { name: "Scenes", exact: true }).click();
    await page
      .getByRole("button", { name: "Customize scene", exact: true })
      .waitFor();
    await screenshot(page, `scenes-mobile-${width}`);
    await page
      .getByRole("button", { name: "Close video details", exact: true })
      .click();
    await page.getByRole("button", { name: "Export MP4", exact: true }).click();
    await page
      .getByRole("button", { name: "Download current MP4", exact: true })
      .click();
    await screenshot(page, `download-mobile-${width}`);
    await page.keyboard.press("Escape");
  }
  await page.getByRole("link", { name: "Back to projects" }).click();
  await page.getByRole("link", { name: "Reel Maestro home" }).click();
  await page.getByLabel("Describe your video").fill("An espresso story");
  await screenshot(page, "start-mobile-dark");
  await page.getByRole("link", { name: "Settings", exact: true }).click();
  await page.getByRole("radio", { name: "Light", exact: true }).click();
  await screenshot(page, "settings-mobile-light");
  await page.getByRole("link", { name: "Reel Maestro home" }).click();
  await page
    .getByRole("heading", { name: "Describe the video you want" })
    .waitFor();
  await screenshot(page, "start-mobile-light");
  await page.getByRole("button", { name: "Customize", exact: true }).click();
  await screenshot(page, "customize-mobile-light");
  await page
    .getByLabel("Spending limit in dollars", { exact: true })
    .scrollIntoViewIfNeeded();
  await screenshot(page, "customize-mobile-light-scrolled");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 768, height: 1024 });
  await screenshot(page, "start-tablet-light");

  // Exercise empty library with a real empty output root, after closing the populated server.
  await context.close();
  await app.close();
  const emptyOut = path.join(temporary, "empty");
  await mkdir(emptyOut);
  const empty = await createApp({
    root: temporary,
    outDir: emptyOut,
    stateDir: path.join(temporary, "empty-state"),
    binary,
    webDir: path.join(root, "studio/web/dist"),
    port,
    env: { PATH: process.env.PATH, HOME: temporary },
  });
  await empty.listen({ host: "127.0.0.1", port });
  try {
    const emptyPage = await browser.newPage({
      viewport: { width: 1440, height: 900 },
      colorScheme: "light",
    });
    await emptyPage.goto(`http://127.0.0.1:${port}`);
    await emptyPage
      .getByRole("heading", { name: "Describe the video you want" })
      .waitFor();
    await screenshot(emptyPage, "first-run-light");
    await emptyPage
      .getByRole("link", { name: "Projects", exact: true })
      .click();
    await emptyPage
      .getByRole("heading", { name: "Your first video starts with an idea" })
      .waitFor();
    await screenshot(emptyPage, "empty-library-light");
    await emptyPage.close();
  } finally {
    await empty.close();
  }
  await verifyNativeRevision({
    browser,
    root,
    temporary,
    outDir,
    source: espresso,
    port,
    screenshot,
  });
  assert.deepEqual(failures, [], "browser errors");
  console.log(`Browser checks passed. Screenshots: ${captures}`);
} catch (error) {
  const page = browser
    .contexts()
    .flatMap((context) => context.pages())
    .at(-1);
  if (page && !page.isClosed())
    await page.screenshot({
      path: path.join(captures, "failure.png"),
      fullPage: true,
    });
  throw error;
} finally {
  await browser.close();
  await app.close();
  await rm(temporary, { recursive: true, force: true });
}
