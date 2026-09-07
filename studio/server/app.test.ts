import assert from "node:assert/strict";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import {
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  symlink,
  writeFile,
} from "node:fs/promises";
import {
  request as httpRequest,
  type ClientRequest,
  type IncomingMessage,
} from "node:http";
import { createServer } from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import type { FastifyInstance } from "fastify";
import type { InjectPayload } from "light-my-request";
import type { Job, Plan } from "../shared.js";
import { createApp, type AppOptions } from "./app.js";

type Harness = {
  app: FastifyInstance;
  root: string;
  outDir: string;
  stateDir: string;
  log: string;
  options: AppOptions;
};
type Session = { cookie: string; csrf: string };

const HOST = "localhost:34567";
const PLAN = {
  prompt: "fixture topic",
  source: "topic",
  format: "reel",
  quality: "draft",
  narration: true,
  music: false,
  video: "off",
  captions: true,
  captionStyle: "burst",
  maxCost: 1,
};

async function harness(overrides: Partial<AppOptions> = {}): Promise<Harness> {
  const root = await mkdtemp(path.join(os.tmpdir(), "reel-studio-test-"));
  const outDir = path.join(root, "out");
  const stateDir = path.join(root, "state");
  const log = path.join(root, "fixture.log");
  const binary = path.join(root, "reelmaestro-fixture");
  const ffprobe = path.join(root, "ffprobe-fixture");
  const ffmpeg = path.join(root, "ffmpeg-fixture");
  const whisper = path.join(root, "whisper-fixture");
  await Promise.all([mkdir(outDir), mkdir(stateDir)]);
  await executable(binary, fixtureSource(log));
  await executable(ffprobe, ffprobeSource());
  await executable(ffmpeg, `#!${process.execPath}\nprocess.exit(0);\n`);
  await executable(whisper, `#!${process.execPath}\nprocess.exit(0);\n`);
  const options: AppOptions = {
    root,
    outDir,
    stateDir,
    binary,
    ffprobe,
    ffmpeg,
    whisper,
    port: 34567,
    commandTimeoutMs: 1_000,
    terminateGraceMs: 30,
    env: {
      PATH: process.env.PATH,
      HOME: root,
      OPENROUTER_API_KEY: "fixture-not-a-real-key",
    },
    ...overrides,
  };
  return {
    app: await createApp(options),
    root,
    outDir,
    stateDir,
    log,
    options,
  };
}

async function executable(file: string, source: string): Promise<void> {
  await writeFile(file, source, { mode: 0o700 });
  await chmod(file, 0o700);
}

function fixtureSource(log: string): string {
  return `#!${process.execPath}
const fs = require("node:fs");
const path = require("node:path");
const args = process.argv.slice(2);
const launchStatus = () => { if (!args.includes("--studio-schema") && !args.includes("--studio-estimate") && !args.includes("--studio-inspect-run") && !args.includes("--revision-plan")) { const { DatabaseSync } = require("node:sqlite"); const db = new DatabaseSync(path.join(path.dirname(${JSON.stringify(log)}), "state", "studio.sqlite")); const row = db.prepare("SELECT status FROM jobs WHERE id=?").get(path.basename(process.cwd())); db.close(); return row?.status; } };
const record = mode => fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ mode, args, cwd: process.cwd(), keyPresent: Boolean(process.env.OPENROUTER_API_KEY), noDotenv: args.includes("--no-dotenv"), noNarration: args.includes("--no-narration"), noCaptions: args.includes("--no-captions"), narrationEnvPresent: process.env.REELMAESTRO_NO_NARRATION !== undefined, captionsEnvPresent: process.env.REELMAESTRO_NO_CAPTIONS !== undefined, launchStatus: launchStatus() }) + "\\n");
if (args.includes("--studio-schema")) { record("schema"); console.log(JSON.stringify({ version: 1, arguments: [] })); process.exit(0); }
if (args.includes("--studio-estimate")) { record("estimate"); console.log(JSON.stringify({ version: 1, total_usd: 0.25, script_usd: 0.05, narration_usd: 0.05, images_usd: 0.15, video_usd: 0, music_usd: 0, warning: "fixture" })); process.exit(0); }
if (args.includes("--studio-inspect-run")) { const source = args[args.indexOf("--studio-inspect-run") + 1]; const scriptBytes = fs.readFileSync(path.join(source, "script.json")); const script = JSON.parse(scriptBytes); script.scenes = (script.scenes || []).map((scene, index) => ({ ...scene, id: scene.id || "scene-fixture-" + index })); const fingerprint = "sha256:" + require("node:crypto").createHash("sha256").update(scriptBytes).digest("hex"); console.log(JSON.stringify({ version: 1, source, source_fingerprint: fingerprint, source_snapshot: {}, manifest_state: "legacy", script, outputs: [] })); process.exit(0); }
if (args.includes("--revision-plan")) { record("revision-plan"); const request = JSON.parse(fs.readFileSync(args[args.indexOf("--revision-plan") + 1])); const destination = path.join(request.source, "revisions", request.revision_id); console.log(JSON.stringify({ version: 1, source: request.source, destination, request, parent_revision: null, source_snapshot: {}, resolved_config: {}, actions: [{ artifact: "final:video", action: "derive", reason: "fixture", destination: "reel-video.mp4", dependencies: {} }], cost: { total_usd: 0, script_usd: 0, narration_usd: 0, images_usd: 0, video_usd: 0, music_usd: 0, uncertain: false }, local_work: true, warnings: [], approval_hash: "sha256:" + "b".repeat(64) })); process.exit(0); }
if (args.includes("--revision-execute") || args.includes("--revision-recover")) { const recovering = args.includes("--revision-recover"); record(recovering ? "revision-recover" : "revision-execute"); const plan = JSON.parse(fs.readFileSync(args[args.findIndex(value => value === "--revision-execute" || value === "--revision-recover") + 1])); const narration = plan.request.script.narration; if (!recovering && narration === "hold revision") { process.on("SIGTERM", () => {}); setInterval(() => {}, 1000); } else if (!recovering && (narration === "pending revision" || narration === "ambiguous revision")) { console.log(JSON.stringify({ version: 1, sequence: 1, timestamp_unix_ms: Date.now(), run_id: "fixture", revision_id: plan.request.revision_id, job_id: plan.request.job_id, stage: "publish", scene_id: null, state: "failed", elapsed_ms: 1, artifact: null, code: narration === "pending revision" ? "accepted_provider_job_pending" : "ambiguous_paid_work", message: "fixture interruption", cost: null })); process.exit(75); } else { fs.mkdirSync(plan.destination, { recursive: true }); fs.writeFileSync(path.join(plan.destination, "script.json"), JSON.stringify(plan.request.script)); fs.writeFileSync(path.join(plan.destination, "reel-video.mp4"), "fixture-video"); console.log(JSON.stringify({ version: 1, sequence: 1, timestamp_unix_ms: Date.now(), run_id: "fixture", revision_id: plan.request.revision_id, job_id: plan.request.job_id, stage: "publish", scene_id: null, state: "completed", elapsed_ms: 1, artifact: "reel-video.mp4", code: null, message: "Published", cost: null })); process.exit(0); } }
record("generate");
const prompt = args[args.indexOf("--topic") + 1] || "fixture";
if (prompt.includes("hold")) { process.on("SIGTERM", () => {}); setInterval(() => {}, 1000); }
else {
  const output = args[args.indexOf("--out") + 1];
  const run = path.join(output, prompt.includes("second") ? "run-second" : "run-exact");
  fs.mkdirSync(run, { recursive: true });
  fs.writeFileSync(path.join(run, "script.json"), JSON.stringify({ title: prompt, format: "reel", narration: prompt, scenes: [] }));
  fs.writeFileSync(path.join(run, prompt.includes("youtube") ? "youtube.mp4" : "reel-video.mp4"), "fixture-video");
}
`;
}

function ffprobeSource(): string {
  return `#!${process.execPath}
console.log(JSON.stringify({ streams: [{ codec_type: "video" }], format: { duration: "2.5" } }));
`;
}

async function getSession(
  app: FastifyInstance,
  cookie?: string,
): Promise<Session> {
  const response = await app.inject({
    method: "GET",
    url: "/api/session",
    headers: { host: HOST, ...(cookie ? { cookie } : {}) },
  });
  assert.equal(response.statusCode, 200);
  const body = object(JSON.parse(response.body));
  assert.equal(typeof body.csrfToken, "string");
  const setCookie = response.headers["set-cookie"];
  const nextCookie =
    typeof setCookie === "string" ? setCookie.split(";", 1)[0] : cookie;
  assert.ok(nextCookie);
  return { cookie: nextCookie, csrf: text(body.csrfToken) };
}

async function request(
  app: FastifyInstance,
  session: Session,
  method: "GET" | "POST" | "HEAD",
  url: string,
  payload?: InjectPayload,
  extraHeaders: Record<string, string> = {},
) {
  return app.inject({
    method,
    url,
    payload,
    headers: {
      host: HOST,
      cookie: session.cookie,
      "x-csrf-token": session.csrf,
      ...(method === "POST" ? { origin: `http://${HOST}` } : {}),
      ...extraHeaders,
    },
  });
}

async function createPlan(
  app: FastifyInstance,
  session: Session,
  prompt = PLAN.prompt,
): Promise<Plan> {
  const response = await request(app, session, "POST", "/api/plans", {
    ...PLAN,
    prompt,
  });
  assert.equal(response.statusCode, 200, response.body);
  return plan(JSON.parse(response.body));
}

async function createJob(
  app: FastifyInstance,
  session: Session,
  approved: Plan,
  key = "fixture-key-0001",
): Promise<Job> {
  const response = await request(app, session, "POST", "/api/jobs", {
    planId: approved.planId,
    planHash: approved.planHash,
    idempotencyKey: key,
  });
  assert.equal(response.statusCode, 202, response.body);
  return job(JSON.parse(response.body));
}

async function waitFor(
  app: FastifyInstance,
  session: Session,
  id: string,
  statuses: Job["status"][],
): Promise<Job> {
  const deadline = Date.now() + 3_000;
  while (Date.now() < deadline) {
    const response = await request(app, session, "GET", `/api/jobs/${id}`);
    const current = job(JSON.parse(response.body));
    if (statuses.includes(current.status)) return current;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`Job ${id} did not reach ${statuses.join(",")}`);
}

test("explicit LAN host preserves exact Host and Origin checks", async (t) => {
  const h = await harness({ host: "192.168.1.34" });
  t.after(() => h.app.close());
  for (const [host, origin, status] of [
    ["192.168.1.34:34567", "http://192.168.1.34:34567", 200],
    ["192.168.1.35:34567", "http://192.168.1.35:34567", 403],
    ["192.168.1.34:34567", "http://localhost:34567", 403],
  ] as const) {
    const response = await h.app.inject({
      method: "GET",
      url: "/api/session",
      headers: { host, origin },
    });
    assert.equal(response.statusCode, status);
  }
});

test("session reuse, exact host/origin, CSRF, and no-store", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const first = await getSession(h.app);
  const second = await getSession(h.app, first.cookie);
  assert.equal(second.csrf, first.csrf);
  const alternateOrigin = await h.app.inject({
    method: "POST",
    url: "/api/settings",
    payload: { apiKey: "x" },
    headers: {
      host: HOST,
      origin: "http://127.0.0.1:34567",
      cookie: first.cookie,
      "x-csrf-token": first.csrf,
    },
  });
  assert.equal(alternateOrigin.statusCode, 403);
  const httpsOrigin = await h.app.inject({
    method: "POST",
    url: "/api/settings",
    payload: { apiKey: "x" },
    headers: {
      host: HOST,
      origin: `https://${HOST}`,
      cookie: first.cookie,
      "x-csrf-token": first.csrf,
    },
  });
  assert.equal(httpsOrigin.statusCode, 403);
  const badHost = await h.app.inject({
    method: "GET",
    url: "/api/session",
    headers: { host: "evil.test" },
  });
  assert.equal(badHost.statusCode, 403);
  const missingCsrf = await h.app.inject({
    method: "POST",
    url: "/api/settings",
    payload: { apiKey: "x" },
    headers: { host: HOST, origin: `http://${HOST}`, cookie: first.cookie },
  });
  assert.equal(missingCsrf.statusCode, 403);
  const settings = await request(h.app, first, "GET", "/api/settings");
  assert.equal(settings.headers["cache-control"], "no-store");
});

test("offline readiness is unauthenticated and reports every component", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const response = await h.app.inject({
    method: "GET",
    url: "/api/ready",
    headers: { host: HOST },
  });
  assert.equal(response.statusCode, 200, response.body);
  const ready = object(JSON.parse(response.body));
  assert.equal(ready.ready, true);
  assert.equal(ready.providerConfigured, true);
  assert.ok(
    Object.values(object(ready.components)).every((value) => value === true),
  );

  const unavailable = await harness({
    whisper: path.join(h.root, "missing-whisper"),
  });
  t.after(() => unavailable.app.close());
  const failed = await unavailable.app.inject({
    method: "GET",
    url: "/api/ready",
    headers: { host: HOST },
  });
  assert.equal(failed.statusCode, 503);
  const failedBody = object(JSON.parse(failed.body));
  assert.equal(failedBody.ready, false);
  assert.equal(object(failedBody.components).whisper, false);
});

test("plan validation rejects unsafe and invalid numeric input", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  for (const payload of [
    { ...PLAN, maxCost: -1 },
    { ...PLAN, minutes: 0 },
    { ...PLAN, speed: -0.1 },
    { ...PLAN, prompt: "https://example.test/article" },
    { ...PLAN, unknown: true },
  ]) {
    const response = await request(
      h.app,
      session,
      "POST",
      "/api/plans",
      payload,
    );
    assert.equal(response.statusCode, 400, response.body);
  }
  const overBudget = await request(h.app, session, "POST", "/api/plans", {
    ...PLAN,
    maxCost: 0.1,
  });
  assert.equal(overBudget.statusCode, 200);
  const overBudgetPlan = plan(JSON.parse(overBudget.body));
  assert.equal(overBudgetPlan.estimate.total_usd, 0.25);
  const blockedJob = await request(h.app, session, "POST", "/api/jobs", {
    planId: overBudgetPlan.planId,
    planHash: overBudgetPlan.planHash,
    idempotencyKey: "fixture-over-budget",
  });
  assert.equal(blockedJob.statusCode, 409);
  assert.equal(
    (await logRecords(h.log)).some((record) => record.mode === "generate"),
    false,
  );
});

test("uploads exceed the default body limit, validate magic, and return signed ids", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const image = Buffer.alloc(1_200_000);
  Buffer.from("89504e470d0a1a0a", "hex").copy(image);
  const uploaded = await request(h.app, session, "POST", "/api/uploads", {
    name: "large.png",
    kind: "image",
    data: image.toString("base64"),
  });
  assert.equal(uploaded.statusCode, 200, uploaded.body);
  const asset = object(JSON.parse(uploaded.body));
  assert.match(text(asset.id), /^[A-Za-z0-9_-]{32}\.[A-Za-z0-9_-]{43}$/);
  assert.equal(asset.size, image.length);
  assert.equal(asset.mime, "image/png");

  const invalid = await request(h.app, session, "POST", "/api/uploads", {
    name: "fake.png",
    kind: "image",
    data: Buffer.from("not an image").toString("base64"),
  });
  assert.equal(invalid.statusCode, 400);
});

test("URL ingestion uses the bounded server fetch contract", async (t) => {
  const h = await harness({
    urlFetcher: async (url) => ({
      url,
      title: "Fixture",
      text: "Extracted article",
    }),
  });
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const response = await request(h.app, session, "POST", "/api/sources/url", {
    url: "https://example.test/article",
  });
  assert.equal(response.statusCode, 200, response.body);
  assert.deepEqual(JSON.parse(response.body), {
    url: "https://example.test/article",
    title: "Fixture",
    text: "Extracted article",
  });
});

test("uploaded text and media assets map only to constrained advanced argv", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const upload = async (
    name: string,
    kind: "text" | "image" | "audio",
    bytes: Buffer,
  ): Promise<string> => {
    const response = await request(h.app, session, "POST", "/api/uploads", {
      name,
      kind,
      data: bytes.toString("base64"),
    });
    assert.equal(response.statusCode, 200, response.body);
    return text(object(JSON.parse(response.body)).id);
  };
  const textId = await upload(
    "script.txt",
    "text",
    Buffer.from("Uploaded script"),
  );
  const imageId = await upload(
    "reference.png",
    "image",
    Buffer.from("89504e470d0a1a0a00", "hex"),
  );
  const audioId = await upload("music.mp3", "audio", Buffer.from("ID3fixture"));
  const response = await request(h.app, session, "POST", "/api/plans", {
    ...PLAN,
    prompt: "",
    source: "script",
    sourceAssetId: textId,
    advanced: {
      textModel: "fixture/model",
      validateScene: 2,
      videoScenes: 1,
      characterReferenceAssetId: imageId,
      watermarkAssetId: imageId,
      musicAssetId: audioId,
      noImages: true,
      verbose: true,
      whisperExecutable: "whisper_timestamped",
    },
  });
  assert.equal(response.statusCode, 200, response.body);
  const estimate = (await logRecords(h.log)).findLast(
    (entry) => entry.mode === "estimate",
  );
  assert.ok(estimate);
  for (const flag of [
    "--no-dotenv",
    "--script",
    "--text-model",
    "--validate-scene",
    "--video-scenes",
    "--character-ref",
    "--watermark",
    "--music",
    "--no-images",
    "--verbose",
    "--whisper-cmd",
  ])
    assert.ok(estimate.args.includes(flag), `missing ${flag}`);
  assert.ok(!estimate.args.some((argument) => argument === "Uploaded script"));
});

test("revision planning preserves canonical ids and executes the immutable approved plan", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const run = path.join(h.outDir, "source-run");
  await mkdir(run);
  await writeFile(
    path.join(run, "script.json"),
    JSON.stringify({
      title: "Source",
      narration: "Original",
      music_prompt: "none",
      characters: [],
      locations: [],
      poster_prompt: "poster",
      narrator_gender: "neutral",
      format: "reel",
      chapters: [],
      scenes: [
        {
          line: "Original",
          image_prompt: "still",
          cast_ids: [],
          location_id: "",
          transition: "cut",
          motion_prompt: "move",
        },
      ],
    }),
  );
  await writeFile(path.join(run, "reel-video.mp4"), "fixture-video");
  const session = await getSession(h.app);
  const listed = await request(h.app, session, "GET", "/api/runs");
  const runs = object(JSON.parse(listed.body)).runs;
  assert.ok(Array.isArray(runs) && runs.length === 1);
  const runId = text(object(runs[0]).id);
  const detailResponse = await request(
    h.app,
    session,
    "GET",
    `/api/runs/${runId}`,
  );
  const detail = object(JSON.parse(detailResponse.body));
  assert.ok(Array.isArray(detail.scenes) && detail.scenes.length === 1);
  const canonicalId = text(object(detail.scenes[0]).id);
  assert.equal(canonicalId, "scene-fixture-0");
  const planned = await request(
    h.app,
    session,
    "POST",
    `/api/runs/${runId}/plans`,
    {
      scenes: [
        {
          id: canonicalId,
          line: "Edited",
          imagePrompt: "still",
          motionPrompt: "move",
          imageAction: "keep",
          motionAction: "regenerate",
        },
      ],
      narrationEnabled: true,
      captions: true,
      musicAction: "keep",
      videoSceneIds: [canonicalId],
      maxCost: 1,
      legacyReuse: "trust",
      expectedSourceFingerprint: detail.sourceFingerprint,
    },
  );
  assert.equal(planned.statusCode, 200, planned.body);
  const revisionPlan = object(JSON.parse(planned.body));
  assert.ok(Array.isArray(revisionPlan.actions));
  assert.equal(revisionPlan.actions.length, 1);
  const approved = await request(h.app, session, "POST", "/api/jobs", {
    planId: revisionPlan.planId,
    planHash: revisionPlan.planHash,
    idempotencyKey: "revision-fixture-key",
  });
  assert.equal(approved.statusCode, 202, approved.body);
  const created = object(JSON.parse(approved.body));
  assert.equal(created.sourceRunId, runId);
  const finished = await waitFor(h.app, session, text(created.id), [
    "succeeded",
  ]);
  assert.equal(finished.sourceRunId, runId);
  assert.equal(finished.stage, "publish");
  assert.equal(typeof finished.runId, "string");
  const logs = await logRecords(h.log);
  assert.ok(
    logs.some(
      (entry) => entry.mode === "revision-plan" && entry.noDotenv === true,
    ),
  );
  assert.ok(
    logs.some(
      (entry) =>
        entry.mode === "revision-execute" && entry.launchStatus === "running",
    ),
  );
});

test("interrupted revisions recover only through the explicit native recovery path", async () => {
  const h = await harness();
  const run = path.join(h.outDir, "recover-source");
  await mkdir(run);
  await writeFile(
    path.join(run, "script.json"),
    JSON.stringify({
      title: "Source",
      narration: "Original",
      music_prompt: "none",
      characters: [],
      locations: [],
      poster_prompt: "poster",
      narrator_gender: "neutral",
      format: "reel",
      chapters: [],
      scenes: [
        {
          id: "scene-recover",
          line: "Original",
          image_prompt: "still",
          cast_ids: [],
          location_id: "",
          transition: "cut",
          motion_prompt: "move",
        },
      ],
    }),
  );
  await writeFile(path.join(run, "reel-video.mp4"), "fixture-video");
  const session = await getSession(h.app);
  const listed = object(
    JSON.parse((await request(h.app, session, "GET", "/api/runs")).body),
  ).runs;
  assert.ok(Array.isArray(listed));
  const runId = text(object(listed[0]).id);
  const detail = object(
    JSON.parse(
      (await request(h.app, session, "GET", `/api/runs/${runId}`)).body,
    ),
  );
  const planned = object(
    JSON.parse(
      (
        await request(h.app, session, "POST", `/api/runs/${runId}/plans`, {
          scenes: [
            {
              id: "scene-recover",
              line: "Original",
              imagePrompt: "still",
              motionPrompt: "move",
            },
          ],
          narration: "hold revision",
          musicAction: "keep",
          videoSceneIds: [],
          maxCost: 1,
          legacyReuse: "trust",
          expectedSourceFingerprint: detail.sourceFingerprint,
        })
      ).body,
    ),
  );
  const approved = object(
    JSON.parse(
      (
        await request(h.app, session, "POST", "/api/jobs", {
          planId: planned.planId,
          planHash: planned.planHash,
          idempotencyKey: "revision-recovery-key",
        })
      ).body,
    ),
  );
  await waitFor(h.app, session, text(approved.id), ["running"]);
  const launchDeadline = Date.now() + 1_000;
  while (
    Date.now() < launchDeadline &&
    !(await logRecords(h.log)).some(
      (entry) => entry.mode === "revision-execute",
    )
  )
    await new Promise((resolve) => setTimeout(resolve, 10));
  await h.app.close();
  const restarted = await createApp(h.options);
  const restartedSession = await getSession(restarted);
  const interrupted = await waitFor(
    restarted,
    restartedSession,
    text(approved.id),
    ["interrupted"],
  );
  assert.equal(interrupted.recoverable, true);
  const resumed = await request(
    restarted,
    restartedSession,
    "POST",
    `/api/jobs/${approved.id}/resume`,
  );
  assert.equal(resumed.statusCode, 202, resumed.body);
  await waitFor(restarted, restartedSession, text(approved.id), ["succeeded"]);
  await restarted.close();
  const logs = await logRecords(h.log);
  assert.equal(
    logs.filter((entry) => entry.mode === "revision-execute").length,
    1,
  );
  assert.equal(
    logs.filter((entry) => entry.mode === "revision-recover").length,
    1,
  );
});

test("native pending IDs are recoverable while ambiguous paid work requires operator action", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const run = path.join(h.outDir, "terminal-source");
  await mkdir(run);
  await writeFile(
    path.join(run, "script.json"),
    JSON.stringify({
      title: "Source",
      narration: "Original",
      music_prompt: "none",
      characters: [],
      locations: [],
      poster_prompt: "poster",
      narrator_gender: "neutral",
      format: "reel",
      chapters: [],
      scenes: [
        {
          id: "scene-terminal",
          line: "Original",
          image_prompt: "still",
          cast_ids: [],
          location_id: "",
          transition: "cut",
          motion_prompt: "move",
        },
      ],
    }),
  );
  await writeFile(path.join(run, "reel-video.mp4"), "fixture-video");
  const session = await getSession(h.app);
  const listed = object(
    JSON.parse((await request(h.app, session, "GET", "/api/runs")).body),
  ).runs;
  assert.ok(Array.isArray(listed));
  const runId = text(object(listed[0]).id);
  const detail = object(
    JSON.parse(
      (await request(h.app, session, "GET", `/api/runs/${runId}`)).body,
    ),
  );
  const launch = async (narration: string, key: string): Promise<Job> => {
    const planned = object(
      JSON.parse(
        (
          await request(h.app, session, "POST", `/api/runs/${runId}/plans`, {
            scenes: [
              {
                id: "scene-terminal",
                line: "Original",
                imagePrompt: "still",
                motionPrompt: "move",
              },
            ],
            narration,
            musicAction: "keep",
            videoSceneIds: [],
            maxCost: 1,
            legacyReuse: "trust",
            expectedSourceFingerprint: detail.sourceFingerprint,
          })
        ).body,
      ),
    );
    const approved = job(
      JSON.parse(
        (
          await request(h.app, session, "POST", "/api/jobs", {
            planId: planned.planId,
            planHash: planned.planHash,
            idempotencyKey: key,
          })
        ).body,
      ),
    );
    return waitFor(h.app, session, approved.id, ["interrupted"]);
  };
  const pending = await launch("pending revision", "pending-revision-key");
  assert.equal(pending.recoverable, true);
  assert.equal(pending.requiresOperatorAction, undefined);
  const ambiguous = await launch(
    "ambiguous revision",
    "ambiguous-revision-key",
  );
  assert.equal(ambiguous.recoverable, undefined);
  assert.equal(ambiguous.requiresOperatorAction, true);
  const refused = await request(
    h.app,
    session,
    "POST",
    `/api/jobs/${ambiguous.id}/resume`,
  );
  assert.equal(refused.statusCode, 409);
});

test("library excludes symlinks, recognizes real outputs, and serves compliant ranges", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const run = path.join(h.outDir, "legacy-run");
  const outside = path.join(h.root, "outside.mp4");
  await mkdir(run);
  await writeFile(
    path.join(run, "script.json"),
    JSON.stringify({
      title: "Legacy",
      format: "youtube",
      scenes: [{ line: "line" }],
    }),
  );
  await writeFile(path.join(run, "youtube.mp4"), "0123456789");
  await writeFile(path.join(run, "reel.ass"), "captions");
  await writeFile(path.join(run, "scene-100.jpg"), "image");
  await writeFile(path.join(run, "audio.mp3"), "");
  await writeFile(outside, "secret");
  await symlink(outside, path.join(run, "poster.jpg"));
  const session = await getSession(h.app);
  const listing = object(
    JSON.parse((await request(h.app, session, "GET", "/api/runs")).body),
  );
  assert.ok(Array.isArray(listing.runs));
  const summary = object(listing.runs[0]);
  assert.equal(summary.status, "complete");
  const id = text(summary.id);
  const detail = object(
    JSON.parse((await request(h.app, session, "GET", `/api/runs/${id}`)).body),
  );
  assert.ok(Array.isArray(detail.artifacts));
  const names = detail.artifacts.map((value) => text(object(value).name));
  assert.ok(names.includes("youtube.mp4"));
  assert.ok(names.includes("reel.ass"));
  assert.ok(names.includes("scene-100.jpg"));
  assert.ok(!names.includes("poster.jpg"));
  assert.equal(
    (
      await request(
        h.app,
        session,
        "GET",
        `/api/runs/${id}/artifacts/poster.jpg`,
      )
    ).statusCode,
    404,
  );
  assert.equal(
    (
      await request(
        h.app,
        session,
        "GET",
        `/api/runs/${id}/artifacts/%2e%2e%2foutside.mp4`,
      )
    ).statusCode,
    404,
  );
  const range = await request(
    h.app,
    session,
    "GET",
    `/api/runs/${id}/artifacts/youtube.mp4`,
    undefined,
    { range: "bytes=4-999" },
  );
  assert.equal(range.statusCode, 206);
  assert.equal(range.headers["content-range"], "bytes 4-9/10");
  assert.equal(range.body, "456789");
  const suffixZero = await request(
    h.app,
    session,
    "GET",
    `/api/runs/${id}/artifacts/youtube.mp4`,
    undefined,
    { range: "bytes=-0" },
  );
  assert.equal(suffixZero.statusCode, 416);
  const emptyRange = await request(
    h.app,
    session,
    "GET",
    `/api/runs/${id}/artifacts/audio.mp3`,
    undefined,
    { range: "bytes=0-" },
  );
  assert.equal(emptyRange.statusCode, 416);
  const head = await request(
    h.app,
    session,
    "HEAD",
    `/api/runs/${id}/artifacts/youtube.mp4`,
  );
  assert.equal(head.statusCode, 200);
  assert.equal(head.headers["content-length"], "10");
  assert.equal(head.body, "");
});

test("library indexes nested revision families and excludes hidden work directories", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const original = path.join(
    h.outDir,
    ".studio-jobs",
    "fresh-job",
    "run-exact",
  );
  const revisions = path.join(original, "revisions");
  const revisionA = path.join(revisions, "revision-a");
  const revisionB = path.join(revisions, "revision-b");
  const hiddenDraft = path.join(revisions, ".revision-c.work");
  const hiddenLock = path.join(revisions, ".locks", "revision-d");
  await Promise.all([
    mkdir(revisionA, { recursive: true }),
    mkdir(revisionB, { recursive: true }),
    mkdir(hiddenDraft, { recursive: true }),
    mkdir(hiddenLock, { recursive: true }),
  ]);
  const writeRun = async (directory: string, title: string): Promise<void> => {
    await writeFile(
      path.join(directory, "script.json"),
      JSON.stringify({ title, format: "reel", scenes: [] }),
    );
    await writeFile(path.join(directory, "reel-video.mp4"), "fixture-video");
  };
  await Promise.all([
    writeRun(original, "Nested original"),
    writeRun(revisionA, "Nested revision A"),
    writeRun(revisionB, "Nested revision B"),
    writeRun(hiddenDraft, "Hidden revision draft"),
    writeRun(hiddenLock, "Hidden revision lock"),
  ]);

  const session = await getSession(h.app);
  const listing = object(
    JSON.parse((await request(h.app, session, "GET", "/api/runs")).body),
  );
  assert.ok(Array.isArray(listing.runs));
  const indexed = listing.runs.map(object);
  assert.deepEqual(
    new Set(indexed.map((run) => text(run.title))),
    new Set(["Nested original", "Nested revision A", "Nested revision B"]),
  );
  const originalSummary = indexed.find(
    (run) => run.title === "Nested original",
  );
  const revisionSummary = indexed.find(
    (run) => run.title === "Nested revision A",
  );
  assert.ok(originalSummary && revisionSummary);

  const originalDetail = object(
    JSON.parse(
      (await request(h.app, session, "GET", `/api/runs/${originalSummary.id}`))
        .body,
    ),
  );
  const revisionDetail = object(
    JSON.parse(
      (await request(h.app, session, "GET", `/api/runs/${revisionSummary.id}`))
        .body,
    ),
  );
  const familyTitles = (detail: Record<string, unknown>): Set<string> => {
    assert.ok(Array.isArray(detail.revisions));
    return new Set(detail.revisions.map((run) => text(object(run).title)));
  };
  const expectedFamily = new Set([
    "Nested original",
    "Nested revision A",
    "Nested revision B",
  ]);
  assert.deepEqual(familyTitles(originalDetail), expectedFamily);
  assert.deepEqual(familyTitles(revisionDetail), expectedFamily);
  assert.equal(revisionDetail.parentRunId, originalSummary.id);
  assert.equal(originalDetail.parentRunId, undefined);
});

test("estimation is offline, bounded, and does not launch generation", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const approved = await createPlan(h.app, session);
  assert.equal(approved.estimate.total_usd, 0.25);
  const records = await logRecords(h.log);
  assert.deepEqual(
    records.map((record) => record.mode),
    ["estimate"],
  );
  assert.equal(records[0]?.keyPresent, true);
  assert.equal(records[0]?.noDotenv, true);
  assert.ok(records[0]?.cwd.startsWith(h.stateDir));
});

test("schema invocation disables dotenv", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const response = await request(h.app, session, "GET", "/api/capabilities");
  assert.equal(response.statusCode, 200);
  assert.equal(object(JSON.parse(response.body)).available, true);
  const records = await logRecords(h.log);
  assert.deepEqual(
    records.map((record) => record.mode),
    ["schema"],
  );
  assert.equal(records[0]?.noDotenv, true);
});

test("plan narration and caption toggles cannot be overridden by operator env", async (t) => {
  const h = await harness({
    env: {
      PATH: process.env.PATH,
      HOME: os.tmpdir(),
      OPENROUTER_API_KEY: "fixture-not-a-real-key",
      REELMAESTRO_NO_NARRATION: "1",
      REELMAESTRO_NO_CAPTIONS: "1",
    },
  });
  t.after(() => h.app.close());
  const session = await getSession(h.app);

  const enabled = await createPlan(h.app, session, "enabled media toggles");
  const enabledJob = await createJob(
    h.app,
    session,
    enabled,
    "fixture-enabled",
  );
  await waitFor(h.app, session, enabledJob.id, ["succeeded"]);

  const disabledResponse = await request(h.app, session, "POST", "/api/plans", {
    ...PLAN,
    prompt: "disabled media toggles",
    narration: false,
    captions: false,
  });
  assert.equal(disabledResponse.statusCode, 200);
  const disabledJob = await createJob(
    h.app,
    session,
    plan(JSON.parse(disabledResponse.body)),
    "fixture-disabled",
  );
  await waitFor(h.app, session, disabledJob.id, ["succeeded"]);

  const records = await logRecords(h.log);
  const enabledRecords = records.filter(
    (record, index) => index < 2 && record.mode !== "schema",
  );
  const disabledRecords = records.slice(2);
  assert.equal(enabledRecords.length, 2);
  assert.equal(disabledRecords.length, 2);
  for (const record of enabledRecords) {
    assert.equal(record.noNarration, false);
    assert.equal(record.noCaptions, false);
    assert.equal(record.narrationEnvPresent, false);
    assert.equal(record.captionsEnvPresent, false);
  }
  for (const record of disabledRecords) {
    assert.equal(record.noNarration, true);
    assert.equal(record.noCaptions, true);
    assert.equal(record.narrationEnvPresent, false);
    assert.equal(record.captionsEnvPresent, false);
  }
});

test("cost helpers reject timeout and oversized output", async (t) => {
  const timeout = await harness({ commandTimeoutMs: 30 });
  t.after(() => timeout.app.close());
  await executable(
    timeout.options.binary ?? "",
    `#!${process.execPath}\nsetInterval(() => {}, 1000);\n`,
  );
  const timeoutSession = await getSession(timeout.app);
  const timedOut = await request(
    timeout.app,
    timeoutSession,
    "POST",
    "/api/plans",
    PLAN,
  );
  assert.equal(timedOut.statusCode, 503);

  const oversized = await harness();
  t.after(() => oversized.app.close());
  await executable(
    oversized.options.binary ?? "",
    `#!${process.execPath}\nprocess.stdout.write("x".repeat(2100000));\n`,
  );
  const oversizedSession = await getSession(oversized.app);
  const tooLarge = await request(
    oversized.app,
    oversizedSession,
    "POST",
    "/api/plans",
    PLAN,
  );
  assert.equal(tooLarge.statusCode, 503);
});

test("submission requires a provider and stale configuration is rejected", async (t) => {
  const env: NodeJS.ProcessEnv = {
    PATH: process.env.PATH,
    HOME: os.tmpdir(),
    OPENROUTER_API_KEY: "fixture-a",
  };
  const h = await harness({ env });
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const approved = await createPlan(h.app, session);
  env.REELMAESTRO_TEXT_MODEL = "changed-after-approval";
  const stale = await request(h.app, session, "POST", "/api/jobs", {
    planId: approved.planId,
    planHash: approved.planHash,
    idempotencyKey: "fixture-stale-1",
  });
  assert.equal(stale.statusCode, 409);

  delete env.REELMAESTRO_TEXT_MODEL;
  const blocker = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "hold queue blocker"),
    "fixture-blocker",
  );
  await waitFor(h.app, session, blocker.id, ["running"]);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "second queued job"),
    "fixture-queued-2",
  );
  env.REELMAESTRO_IMAGE_MODEL = "changed-while-queued";
  await request(h.app, session, "POST", `/api/jobs/${blocker.id}/cancel`, {});
  const rejectedQueued = await waitFor(h.app, session, queued.id, ["failed"]);
  assert.match(rejectedQueued.error ?? "", /configuration changed/i);

  const missing = await harness({
    env: { PATH: process.env.PATH, HOME: os.tmpdir() },
  });
  t.after(() => missing.app.close());
  const missingSession = await getSession(missing.app);
  const missingPlan = await createPlan(missing.app, missingSession);
  const response = await request(
    missing.app,
    missingSession,
    "POST",
    "/api/jobs",
    {
      planId: missingPlan.planId,
      planHash: missingPlan.planHash,
      idempotencyKey: "fixture-no-key",
    },
  );
  assert.equal(response.statusCode, 409);
});

test("idempotency is bound both to key and approved plan", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const approved = await createPlan(h.app, session);
  const first = await createJob(h.app, session, approved);
  const repeat = await request(h.app, session, "POST", "/api/jobs", {
    planId: approved.planId,
    planHash: approved.planHash,
    idempotencyKey: "fixture-key-0001",
  });
  assert.equal(repeat.statusCode, 200);
  assert.equal(job(JSON.parse(repeat.body)).id, first.id);
  const duplicatePlan = await request(h.app, session, "POST", "/api/jobs", {
    planId: approved.planId,
    planHash: approved.planHash,
    idempotencyKey: "fixture-key-0002",
  });
  assert.equal(duplicatePlan.statusCode, 409);

  const racingPlan = await createPlan(h.app, session, "second plan race");
  const racingResponses = await Promise.all([
    request(h.app, session, "POST", "/api/jobs", {
      planId: racingPlan.planId,
      planHash: racingPlan.planHash,
      idempotencyKey: "fixture-race-1",
    }),
    request(h.app, session, "POST", "/api/jobs", {
      planId: racingPlan.planId,
      planHash: racingPlan.planHash,
      idempotencyKey: "fixture-race-2",
    }),
  ]);
  assert.deepEqual(
    racingResponses.map((response) => response.statusCode).sort(),
    [202, 409],
  );
});

test("SSE validates replay cursors and stays live for new events", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "hold SSE stream"),
  );
  await waitFor(h.app, session, queued.id, ["running"]);
  const invalid = await request(
    h.app,
    session,
    "GET",
    `/api/jobs/${queued.id}/events`,
    undefined,
    { "last-event-id": "-1" },
  );
  assert.equal(invalid.statusCode, 400);
  await h.app.listen({ host: "127.0.0.1", port: 34567 });
  const stream = await openEventStream(queued.id, session, "1");
  t.after(() => stream.request.destroy());
  let data = "";
  let ended = false;
  stream.response.setEncoding("utf8");
  stream.response.on("data", (chunk) => {
    data += String(chunk);
  });
  stream.response.on("end", () => {
    ended = true;
  });
  await waitUntil(() => data.includes('"status":"running"'));
  assert.equal(data.includes('"status":"queued"'), false);
  assert.equal(ended, false);
  await request(h.app, session, "POST", `/api/jobs/${queued.id}/cancel`, {});
  await waitUntil(() => data.includes('"status":"cancelled"'));
  assert.equal(ended, false);
  stream.request.destroy();
});

test("fixture generation validates video and records the exact produced run", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "youtube fixture"),
  );
  const finished = await waitFor(h.app, session, queued.id, ["succeeded"]);
  assert.ok(finished.runId);
  const detail = object(
    JSON.parse(
      (await request(h.app, session, "GET", `/api/runs/${finished.runId}`))
        .body,
    ),
  );
  assert.equal(detail.title, "youtube fixture");
  assert.equal(
    detail.videoUrl,
    `/api/runs/${finished.runId}/artifacts/youtube.mp4`,
  );
  const generation = (await logRecords(h.log)).find(
    (record) => record.mode === "generate",
  );
  assert.equal(generation?.noDotenv, true);
  assert.equal(generation?.launchStatus, "running");
});

test("shutdown closes an open SSE stream without client abort", async () => {
  const h = await harness();
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "hold open SSE shutdown"),
  );
  await waitFor(h.app, session, queued.id, ["running"]);
  await h.app.listen({ host: "127.0.0.1", port: 34567 });
  const stream = await openEventStream(queued.id, session, "0");
  let ended = false;
  stream.response.resume();
  stream.response.once("end", () => {
    ended = true;
  });
  await Promise.race([
    h.app.close(),
    new Promise<never>((_resolve, reject) =>
      setTimeout(() => reject(new Error("Server close timed out")), 3_000),
    ),
  ]);
  await waitUntil(() => ended || stream.response.destroyed);
  stream.request.destroy();

  const restarted = await createApp(h.options);
  const restartedSession = await getSession(restarted);
  const interrupted = await waitFor(restarted, restartedSession, queued.id, [
    "interrupted",
  ]);
  assert.equal(interrupted.status, "interrupted");
  await restarted.close();
});

test("ffprobe requires a positive-duration video stream", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  await executable(
    h.options.ffprobe ?? "",
    `#!${process.execPath}\nconsole.log(JSON.stringify({ streams: [{ codec_type: "audio" }], format: { duration: "0" } }));\n`,
  );
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "invalid video output"),
  );
  const failed = await waitFor(h.app, session, queued.id, ["failed"]);
  assert.match(failed.error ?? "", /valid resulting video/i);
  const listing = object(
    JSON.parse((await request(h.app, session, "GET", "/api/runs")).body),
  );
  assert.ok(Array.isArray(listing.runs));
  assert.equal(object(listing.runs[0]).status, "damaged");
});

test("cancellation escalates and reaches one cancelled terminal state", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "hold for cancellation"),
  );
  await waitFor(h.app, session, queued.id, ["running"]);
  const cancelling = await request(
    h.app,
    session,
    "POST",
    `/api/jobs/${queued.id}/cancel`,
    {},
  );
  assert.equal(cancelling.statusCode, 200);
  const finished = await waitFor(h.app, session, queued.id, ["cancelled"]);
  assert.equal(finished.status, "cancelled");
});

test("shutdown interrupts active work and restart never retries it", async () => {
  const h = await harness();
  const session = await getSession(h.app);
  const queued = await createJob(
    h.app,
    session,
    await createPlan(h.app, session, "hold for shutdown"),
  );
  await waitFor(h.app, session, queued.id, ["running"]);
  await h.app.close();
  const before = (await logRecords(h.log)).filter(
    (record) => record.mode === "generate",
  ).length;
  const restarted = await createApp(h.options);
  const restartedSession = await getSession(restarted);
  const interrupted = await waitFor(restarted, restartedSession, queued.id, [
    "interrupted",
  ]);
  assert.equal(interrupted.status, "interrupted");
  await new Promise((resolve) => setTimeout(resolve, 80));
  const after = (await logRecords(h.log)).filter(
    (record) => record.mode === "generate",
  ).length;
  assert.equal(after, before);
  await restarted.close();
});

test("state directory lock rejects a concurrent server", async (t) => {
  const h = await harness();
  t.after(() => h.app.close());
  await assert.rejects(createApp(h.options), /already in use/);
});

test("malformed state locks are never removed automatically", async () => {
  const h = await harness();
  await h.app.close();
  await writeFile(path.join(h.stateDir, "server.lock"), "", { mode: 0o600 });
  await assert.rejects(createApp(h.options), /lock is malformed/);
  assert.equal(
    await readFile(path.join(h.stateDir, "server.lock"), "utf8"),
    "",
  );
});

test("entrypoint SIGTERM closes the server and interrupts its paid child", async () => {
  const port = 34569;
  const h = await harness({ port });
  await h.app.close();
  const server = startEntrypoint(h, port);
  try {
    await waitForListening(server);
    const sessionResponse = await actualRequest(port, "GET", "/api/session");
    assert.equal(sessionResponse.status, 200);
    const cookieHeader = sessionResponse.headers["set-cookie"];
    assert.ok(Array.isArray(cookieHeader));
    const cookie = cookieHeader[0]?.split(";", 1)[0];
    assert.ok(cookie);
    const csrf = text(object(JSON.parse(sessionResponse.body)).csrfToken);
    const headers = {
      Cookie: cookie,
      Origin: `http://localhost:${port}`,
      "X-CSRF-Token": csrf,
    };
    const planResponse = await actualRequest(
      port,
      "POST",
      "/api/plans",
      headers,
      { ...PLAN, prompt: "hold entrypoint shutdown" },
    );
    assert.equal(planResponse.status, 200, planResponse.body);
    const approved = plan(JSON.parse(planResponse.body));
    const jobResponse = await actualRequest(
      port,
      "POST",
      "/api/jobs",
      headers,
      {
        planId: approved.planId,
        planHash: approved.planHash,
        idempotencyKey: "fixture-entrypoint",
      },
    );
    assert.equal(jobResponse.status, 202, jobResponse.body);
    const queued = job(JSON.parse(jobResponse.body));
    await waitUntil(async () => {
      const response = await actualRequest(
        port,
        "GET",
        `/api/jobs/${queued.id}`,
        { Cookie: cookie },
      );
      return job(JSON.parse(response.body)).status === "running";
    });
    assert.equal(server.kill("SIGTERM"), true);
    assert.equal(await waitForExit(server), 0);

    const restarted = await createApp({ ...h.options, port: 34567 });
    const restartedSession = await getSession(restarted);
    const interrupted = await waitFor(restarted, restartedSession, queued.id, [
      "interrupted",
    ]);
    assert.equal(interrupted.status, "interrupted");
    await restarted.close();
  } finally {
    if (server.exitCode === null) server.kill("SIGKILL");
  }
});

test("entrypoint listen failure releases the state lock", async () => {
  const port = 34570;
  const blocker = createServer();
  await new Promise<void>((resolve, reject) => {
    blocker.once("error", reject);
    blocker.listen(port, "127.0.0.1", resolve);
  });
  const h = await harness({ port });
  await h.app.close();
  const server = startEntrypoint(h, port);
  try {
    assert.notEqual(await waitForExit(server), 0);
  } finally {
    blocker.close();
    if (server.exitCode === null) server.kill("SIGKILL");
  }
  const reopened = await createApp({ ...h.options, port: 34567 });
  await reopened.close();
});

function object(value: unknown): Record<string, unknown> {
  assert.ok(
    value !== null && typeof value === "object" && !Array.isArray(value),
  );
  return Object.fromEntries(Object.entries(value));
}
function text(value: unknown): string {
  if (typeof value !== "string") throw new Error("Expected string");
  return value;
}
function plan(value: unknown): Plan {
  const result = object(value);
  const estimate = object(result.estimate);
  assert.equal(typeof result.planId, "string");
  assert.equal(typeof result.planHash, "string");
  assert.equal(typeof result.expiresAt, "string");
  assert.equal(estimate.version, 1);
  for (const name of [
    "total_usd",
    "script_usd",
    "narration_usd",
    "images_usd",
    "video_usd",
    "music_usd",
  ])
    assert.equal(typeof estimate[name], "number");
  assert.equal(typeof estimate.warning, "string");
  return {
    planId: text(result.planId),
    planHash: text(result.planHash),
    expiresAt: text(result.expiresAt),
    estimate: {
      version: 1,
      total_usd: number(estimate.total_usd),
      script_usd: number(estimate.script_usd),
      narration_usd: number(estimate.narration_usd),
      images_usd: number(estimate.images_usd),
      video_usd: number(estimate.video_usd),
      music_usd: number(estimate.music_usd),
      warning: text(estimate.warning),
    },
  };
}
function job(value: unknown): Job {
  const result = object(value);
  const status = result.status;
  assert.ok(
    status === "queued" ||
      status === "running" ||
      status === "succeeded" ||
      status === "failed" ||
      status === "interrupted" ||
      status === "cancelled" ||
      status === "cancelling",
  );
  return {
    id: text(result.id),
    status,
    createdAt: text(result.createdAt),
    updatedAt: text(result.updatedAt),
    ...(typeof result.runId === "string" ? { runId: result.runId } : {}),
    ...(typeof result.sourceRunId === "string"
      ? { sourceRunId: result.sourceRunId }
      : {}),
    ...(typeof result.stage === "string" ? { stage: result.stage } : {}),
    ...(typeof result.progress === "number"
      ? { progress: result.progress }
      : {}),
    ...(typeof result.recoverable === "boolean"
      ? { recoverable: result.recoverable }
      : {}),
    ...(typeof result.requiresOperatorAction === "boolean"
      ? { requiresOperatorAction: result.requiresOperatorAction }
      : {}),
    ...(typeof result.error === "string" ? { error: result.error } : {}),
  };
}
function number(value: unknown): number {
  if (typeof value !== "number") throw new Error("Expected number");
  return value;
}
async function logRecords(file: string): Promise<
  Array<{
    mode: string;
    args: string[];
    cwd: string;
    keyPresent: boolean;
    noDotenv: boolean;
    noNarration: boolean;
    noCaptions: boolean;
    narrationEnvPresent: boolean;
    captionsEnvPresent: boolean;
    launchStatus?: string;
  }>
> {
  const source = await readFile(file, "utf8").catch(() => "");
  return source.trim()
    ? source
        .trim()
        .split("\n")
        .map((line) => {
          const value = object(JSON.parse(line));
          const args = value.args;
          assert.ok(
            Array.isArray(args) &&
              args.every((argument) => typeof argument === "string"),
          );
          return {
            mode: text(value.mode),
            args,
            cwd: text(value.cwd),
            keyPresent: value.keyPresent === true,
            noDotenv: value.noDotenv === true,
            noNarration: value.noNarration === true,
            noCaptions: value.noCaptions === true,
            narrationEnvPresent: value.narrationEnvPresent === true,
            captionsEnvPresent: value.captionsEnvPresent === true,
            ...(typeof value.launchStatus === "string"
              ? { launchStatus: value.launchStatus }
              : {}),
          };
        })
    : [];
}

async function openEventStream(
  jobId: string,
  session: Session,
  lastEventId: string,
): Promise<{ request: ClientRequest; response: IncomingMessage }> {
  return new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        hostname: "127.0.0.1",
        port: 34567,
        path: `/api/jobs/${jobId}/events`,
        method: "GET",
        headers: {
          Host: HOST,
          Cookie: session.cookie,
          "Last-Event-ID": lastEventId,
        },
      },
      (response) => resolve({ request, response }),
    );
    request.once("error", reject);
    request.end();
  });
}

async function waitUntil(
  predicate: () => boolean | Promise<boolean>,
): Promise<void> {
  const deadline = Date.now() + 3_000;
  while (!(await predicate())) {
    if (Date.now() >= deadline) throw new Error("Condition was not reached");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function startEntrypoint(
  h: Harness,
  port: number,
): ChildProcessWithoutNullStreams {
  return spawn(
    process.execPath,
    [path.resolve("node_modules/tsx/dist/cli.mjs"), "server/index.ts"],
    {
      cwd: process.cwd(),
      env: {
        HOME: h.root,
        PATH: process.env.PATH,
        OPENROUTER_API_KEY: "fixture-not-a-real-key",
        REELMAESTRO_PORT: String(port),
        REELMAESTRO_OUT_DIR: h.outDir,
        REELMAESTRO_STATE_DIR: h.stateDir,
        REELMAESTRO_BINARY: h.options.binary,
      },
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
}

async function waitForListening(
  server: ChildProcessWithoutNullStreams,
): Promise<void> {
  let output = "";
  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(
      () => reject(new Error("Server did not listen")),
      3_000,
    );
    server.stdout.on("data", (chunk) => {
      output += String(chunk);
      if (output.includes("Studio listening")) {
        clearTimeout(timeout);
        resolve();
      }
    });
    server.once("exit", (code) => {
      clearTimeout(timeout);
      reject(new Error(`Server exited before listening (${code})`));
    });
  });
}

async function waitForExit(
  server: ChildProcessWithoutNullStreams,
): Promise<number | null> {
  if (server.exitCode !== null) return server.exitCode;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(
      () => reject(new Error("Server did not exit")),
      3_000,
    );
    server.once("exit", (code) => {
      clearTimeout(timeout);
      resolve(code);
    });
  });
}

async function actualRequest(
  port: number,
  method: "GET" | "POST",
  url: string,
  headers: Record<string, string> = {},
  payload?: object,
): Promise<{
  status: number | undefined;
  headers: IncomingMessage["headers"];
  body: string;
}> {
  const body = payload === undefined ? undefined : JSON.stringify(payload);
  return new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        hostname: "127.0.0.1",
        port,
        path: url,
        method,
        headers: {
          Host: `localhost:${port}`,
          ...headers,
          ...(body
            ? {
                "Content-Type": "application/json",
                "Content-Length": String(Buffer.byteLength(body)),
              }
            : {}),
        },
      },
      (response) => {
        let responseBody = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => {
          responseBody += String(chunk);
        });
        response.once("end", () =>
          resolve({
            status: response.statusCode,
            headers: response.headers,
            body: responseBody,
          }),
        );
      },
    );
    request.once("error", reject);
    request.end(body);
  });
}
