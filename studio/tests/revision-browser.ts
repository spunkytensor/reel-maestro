// Real Rust + server + browser integration. No credentials; only local reused media.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import type { Browser, Page } from "playwright-core";
import type { Job, RevisionInput, RunDetail, RunSummary } from "../shared";
import { createApp } from "../server/app";

export async function verifyNativeRevision({
  browser,
  root,
  temporary,
  outDir,
  source,
  port,
  screenshot,
}: {
  browser: Browser;
  root: string;
  temporary: string;
  outDir: string;
  source: string;
  port: number;
  screenshot: (page: Page, name: string) => Promise<void>;
}) {
  const originalScript = await readFile(path.join(source, "script.json"));
  const originalVideo = await readFile(path.join(source, "reel.mp4"));
  const hash = (bytes: Buffer) =>
    createHash("sha256").update(bytes).digest("hex");
  const secondStill = hash(await readFile(path.join(source, "scene-01.jpg")));
  const app = await createApp({
    root: temporary,
    outDir,
    stateDir: path.join(temporary, "native-state"),
    binary: path.join(root, "target/debug/reelmaestro"),
    webDir: path.join(root, "studio/web/dist"),
    port,
    env: { PATH: process.env.PATH, HOME: temporary },
  });
  await app.listen({ host: "127.0.0.1", port });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    colorScheme: "light",
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const failures: string[] = [];
  page.on("pageerror", (error) => failures.push(error.message));
  try {
    const id = createHash("sha256")
      .update(source)
      .digest("base64url")
      .slice(0, 24);
    await page.goto(`http://127.0.0.1:${port}/#projects`);
    await page.locator(`a[href="#video/${id}"]`).click();
    await page
      .getByRole("button", { name: "Play video", exact: true })
      .waitFor();
    const originalUrl = page.url();
    const response = await context.request.get(
      `http://127.0.0.1:${port}/api/runs/${id}`,
    );
    assert.equal(response.status(), 200);
    const detail: RunDetail = await response.json();
    // Seed optional local-only settings through the same persisted-draft contract as reload.
    const draft: RevisionInput = {
      scenes: detail.scenes.map(
        ({
          id,
          line,
          imagePrompt,
          motionPrompt,
          castIds,
          locationId,
          transition,
        }) => ({
          id,
          line,
          imagePrompt,
          motionPrompt,
          castIds,
          locationId,
          transition,
        }),
      ),
      chapters: detail.chapters,
      narration: detail.narration,
      musicPrompt: detail.musicPrompt,
      musicAction: "remove",
      videoSceneIds: [],
      maxCost: 0,
      expectedSourceFingerprint: detail.sourceFingerprint,
      narrationEnabled: false,
      captions: false,
      advanced: {
        sceneSeconds: 0.5,
        grade: false,
        loudnorm: false,
        dissolve: false,
        embedPoster: false,
      },
    };
    await page.evaluate(
      ({ id, draft }) =>
        localStorage.setItem(`reelmaestro.draft.${id}`, JSON.stringify(draft)),
      { id, draft },
    );
    await page.reload();
    await page
      .getByRole("listbox", { name: "Scenes" })
      .getByRole("option")
      .first()
      .focus();
    await page.keyboard.press("Control+ArrowDown");
    await page.getByRole("button", { name: /Apply \d+ changes/ }).click();
    await page.getByRole("checkbox").check();
    await page
      .getByRole("button", { name: "Review changes", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Apply changes", exact: true })
      .waitFor();
    await screenshot(page, "revision-review-light");
    assert.equal(
      await page
        .locator(".overlay")
        .getByText("$0.00", { exact: true })
        .count(),
      1,
    );
    await page
      .getByRole("button", { name: "Apply changes", exact: true })
      .scrollIntoViewIfNeeded();
    await screenshot(page, "revision-review-light-scrolled");
    await page
      .getByRole("button", { name: "Apply changes", exact: true })
      .click();
    await page.waitForURL(
      (url) => url.href.includes("#video/") && url.href !== originalUrl,
      { timeout: 180_000 },
    );
    await page
      .getByRole("button", { name: "Play video", exact: true })
      .waitFor();
    await page.waitForFunction(
      () => (document.querySelector("video")?.duration ?? 0) > 0,
    );
    await screenshot(page, "revision-completed-light");
    assert.deepEqual(
      await readFile(path.join(source, "script.json")),
      originalScript,
    );
    assert.equal(
      hash(await readFile(path.join(source, "reel.mp4"))),
      hash(originalVideo),
    );
    const directories = (await readdir(path.join(source, "revisions"))).filter(
      (name) => !name.startsWith("."),
    );
    assert.equal(
      directories.length,
      1,
      "one approval publishes one immutable revision",
    );
    const destination = path.join(source, "revisions", directories[0]);
    assert.equal(
      hash(await readFile(path.join(destination, "scene-00.jpg"))),
      secondStill,
      "stable IDs remap copied media after reorder",
    );
    assert.ok(
      (await readFile(path.join(destination, "run-manifest.json"))).length > 0,
    );
    await page.getByRole("button", { name: "Versions", exact: true }).click();
    const options = await page
      .getByLabel("Compare with")
      .locator("option")
      .evaluateAll((options) =>
        options.map((option) => option.getAttribute("value")).filter(Boolean),
      );
    assert.ok(options.includes(id), "version history contains original");
    await page.getByLabel("Compare with").selectOption(id);
    await page
      .getByText("Scene 1: changed position", { exact: true })
      .waitFor();
    await screenshot(page, "version-comparison-light");
    await page.emulateMedia({ colorScheme: "dark" });
    await page.evaluate(() => {
      document.documentElement.dataset.theme = "dark";
    });
    await screenshot(page, "version-comparison-dark");
    await page.keyboard.press("Escape");
    await page.setViewportSize({ width: 390, height: 844 });
    await screenshot(page, "revision-mobile-dark");
    const libraryResponse = await context.request.get(
      `http://127.0.0.1:${port}/api/runs`,
    );
    const library: { runs: RunSummary[] } = await libraryResponse.json();
    assert.ok(
      library.runs.some((run) => run.id === id && run.status === "complete"),
    );
    // Recovery controls use a browser transport fixture; native recovery guards are covered by server tests.
    let resumed = 0;
    const recoveryJob: Job = {
      id: "offline-recovery",
      status: "interrupted",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      recoverable: true,
    };
    await page.route("**/api/jobs", (route) =>
      route.fulfill({ json: { jobs: [recoveryJob] } }),
    );
    await page.route("**/api/jobs/offline-recovery/resume", async (route) => {
      assert.equal(route.request().method(), "POST");
      resumed++;
      recoveryJob.recoverable = false;
      await route.fulfill({ status: 202, json: recoveryJob });
    });
    await page.reload();
    await page.getByRole("button", { name: "Activity", exact: true }).click();
    await page
      .getByRole("button", { name: "Review recovery", exact: true })
      .click();
    assert.equal(resumed, 0);
    await screenshot(page, "recovery-mobile-dark");
    await page
      .getByRole("button", { name: "Resume approved work", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Resume approved work", exact: true })
      .waitFor({ state: "hidden" });
    assert.equal(resumed, 1);
    assert.deepEqual(failures, []);
  } catch (error) {
    await page.screenshot({
      path: path.join(root, "out/studio-verification/native-failure.png"),
      fullPage: true,
    });
    throw error;
  } finally {
    await context.close();
    await app.close();
  }
}
