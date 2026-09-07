import Fastify, { type FastifyInstance } from "fastify";
import fastifyStatic from "@fastify/static";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import {
  access,
  chmod,
  lstat,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  spawn,
  type ChildProcess,
  type SpawnOptionsWithoutStdio,
} from "node:child_process";
import type { ServerResponse } from "node:http";
import type {
  CostEstimate,
  JobEvent,
  PlanInput,
  RevisionInput,
  RevisionPlan,
  UrlSource,
} from "../shared.js";
import { validateAdvanced, validateRevision } from "../validation.js";
import { AssetStore } from "./assets.js";
import { Library } from "./library.js";
import { fetchUrlSource } from "./sources.js";
import { acquireStateLock, Store, type StoredJob } from "./store.js";

export type AppOptions = {
  root?: string;
  outDir?: string;
  stateDir?: string;
  binary?: string;
  ffprobe?: string;
  ffmpeg?: string;
  whisper?: string;
  webDir?: string;
  port?: number;
  host?: string;
  jobTimeoutMs?: number;
  commandTimeoutMs?: number;
  terminateGraceMs?: number;
  env?: NodeJS.ProcessEnv;
  urlFetcher?: (url: string) => Promise<UrlSource | string>;
};

type CommandResult = {
  code: number | null;
  stdout: string;
  timedOut: boolean;
  outputExceeded: boolean;
};
type Session = { csrf: string; expires: number };
type ActiveJob = {
  id: string;
  child: ChildProcess;
  terminal: boolean;
  done: Promise<void>;
  resolveDone: () => void;
  killTimer?: NodeJS.Timeout;
  timedOut: boolean;
  revisionId?: string;
  timingOnly: boolean;
  nativeFailureCode?: string;
};

const MAX_STDOUT = 2_000_000;
const SESSION_TTL_MS = 8 * 60 * 60_000;
const MAX_SESSIONS = 64;
const CHILD_ENV_NAMES = new Set([
  "HOME",
  "USER",
  "LOGNAME",
  "PATH",
  "TMPDIR",
  "TMP",
  "TEMP",
  "LANG",
  "LC_ALL",
  "SSL_CERT_FILE",
  "SSL_CERT_DIR",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
  "XDG_CACHE_HOME",
  "HF_HOME",
  "PYTHONPATH",
  "OPENROUTER_API_KEY",
]);

export async function createApp(
  options: AppOptions = {},
): Promise<FastifyInstance> {
  const operatorEnv = options.env ?? process.env;
  const root = path.resolve(
    options.root ?? fileURLToPath(new URL("../../", import.meta.url)),
  );
  const port = options.port ?? parsePort(operatorEnv.REELMAESTRO_PORT) ?? 3000;
  const outDir = path.resolve(
    options.outDir ?? operatorEnv.REELMAESTRO_OUT_DIR ?? path.join(root, "out"),
  );
  const stateDir = path.resolve(
    options.stateDir ??
      operatorEnv.REELMAESTRO_STATE_DIR ??
      path.join(root, ".studio-state"),
  );
  const jobsDir = path.join(outDir, ".studio-jobs");
  const binary = path.resolve(
    options.binary ??
      operatorEnv.REELMAESTRO_BINARY ??
      path.join(root, "target/debug/reelmaestro"),
  );
  const ffprobe = options.ffprobe ? path.resolve(options.ffprobe) : "ffprobe";
  const ffmpeg = options.ffmpeg ? path.resolve(options.ffmpeg) : "ffmpeg";
  const whisper = options.whisper
    ? path.resolve(options.whisper)
    : "whisper_timestamped";
  const commandTimeoutMs = options.commandTimeoutMs ?? 15_000;
  const terminateGraceMs = options.terminateGraceMs ?? 2_000;
  await mkdir(jobsDir, { recursive: true });
  await mkdir(stateDir, { recursive: true, mode: 0o700 });
  const lock = acquireStateLock(stateDir);
  let store: Store | undefined;
  try {
    const app = Fastify({ logger: false, bodyLimit: 1_100_000 });
    const library = new Library(
      outDir,
      async (file) => {
        const env = await childEnvironment(operatorEnv, credentialFile);
        delete env.OPENROUTER_API_KEY;
        return validVideo(ffprobe, file, commandTimeoutMs, env);
      },
      async (directory) => {
        const inspectEnv = await childEnvironment(operatorEnv, credentialFile);
        delete inspectEnv.OPENROUTER_API_KEY;
        const result = await runCommand(
          binary,
          ["--no-dotenv", "--studio-inspect-run", directory],
          {
            cwd: stateDir,
            env: inspectEnv,
          },
          commandTimeoutMs,
        );
        return result.code === 0 && !result.timedOut && !result.outputExceeded
          ? parseJsonObject(result.stdout)
          : undefined;
      },
    );
    const assets = new AssetStore(stateDir);
    store = new Store(stateDir);
    const currentStore = store;
    const sessions = new Map<string, Session>();
    const subscribers = new Map<string, Set<ServerResponse>>();
    let active: ActiveJob | undefined;
    let pumping = false;
    let pumpTask: Promise<void> | undefined;
    let pumpAgain = false;
    let paused = false;
    let shuttingDown = false;
    const credentialFile = path.join(stateDir, "credentials.json");
    const allowedHosts = new Set([`localhost:${port}`, `127.0.0.1:${port}`]);
    if (options.host) allowedHosts.add(`${options.host}:${port}`);

    app.setErrorHandler((error, _request, reply) => {
      const details = plainObject(error);
      const reportedStatus = details?.statusCode;
      const status =
        typeof reportedStatus === "number" &&
        reportedStatus >= 400 &&
        reportedStatus < 600
          ? reportedStatus
          : 500;
      return reply
        .code(status)
        .send({ error: "Request could not be completed" });
    });

    const notify = (jobId: string): void => {
      const clients = subscribers.get(jobId);
      if (!clients?.size) return;
      const event = currentStore.latestEvent(jobId);
      if (!event) return;
      const data = encodeEvent(event);
      for (const client of clients) client.write(data);
    };
    const transition = (
      id: string,
      status: Parameters<Store["setStatus"]>[1],
      message: string,
      extra: { runId?: string; error?: string } = {},
    ): void => {
      currentStore.setStatus(id, status, message, undefined, extra);
      notify(id);
    };

    app.addHook("onRequest", async (request, reply) => {
      reply.header(
        "Content-Security-Policy",
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; media-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
      );
      reply.header("X-Content-Type-Options", "nosniff");
      reply.header("Referrer-Policy", "no-referrer");
      if (request.url.startsWith("/api/"))
        reply.header("Cache-Control", "no-store");
      const host = request.headers.host;
      if (!host || !allowedHosts.has(host))
        return reply.code(403).send({ error: "Invalid Host" });
      const origin = request.headers.origin;
      if (origin !== undefined && !exactHttpOrigin(origin, host))
        return reply.code(403).send({ error: "Foreign Origin rejected" });
      if (request.headers["sec-fetch-site"] === "cross-site")
        return reply.code(403).send({ error: "Cross-site request rejected" });
      if (
        !request.url.startsWith("/api/") ||
        request.url === "/api/session" ||
        request.url === "/api/ready"
      )
        return;
      pruneSessions(sessions);
      const sid = sessionCookie(request.headers.cookie);
      const session = sid ? sessions.get(sid) : undefined;
      if (!session || session.expires <= Date.now())
        return reply.code(401).send({ error: "Session required" });
      session.expires = Date.now() + SESSION_TTL_MS;
      if (
        !["GET", "HEAD", "OPTIONS"].includes(request.method) &&
        request.headers["x-csrf-token"] !== session.csrf
      ) {
        return reply.code(403).send({ error: "Invalid CSRF token" });
      }
    });

    app.get("/api/session", async (request, reply) => {
      pruneSessions(sessions);
      const existingId = sessionCookie(request.headers.cookie);
      const existing = existingId ? sessions.get(existingId) : undefined;
      if (existing && existing.expires > Date.now()) {
        existing.expires = Date.now() + SESSION_TTL_MS;
        return { csrfToken: existing.csrf };
      }
      while (sessions.size >= MAX_SESSIONS)
        sessions.delete(sessions.keys().next().value ?? "");
      const sid = randomBytes(24).toString("base64url");
      const csrf = randomBytes(24).toString("base64url");
      sessions.set(sid, { csrf, expires: Date.now() + SESSION_TTL_MS });
      reply.header(
        "Set-Cookie",
        `rm_session=${sid}; Path=/; HttpOnly; SameSite=Strict`,
      );
      return { csrfToken: csrf };
    });

    app.get("/api/ready", async (_request, reply) => {
      const offlineEnv = await childEnvironment(operatorEnv, credentialFile);
      delete offlineEnv.OPENROUTER_API_KEY;
      const [
        output,
        state,
        reelmaestro,
        ffmpegReady,
        ffprobeReady,
        whisperReady,
      ] = await Promise.all([
        writableDirectory(outDir),
        writableDirectory(stateDir),
        executableAvailable(
          binary,
          ["--no-dotenv", "--studio-schema"],
          stateDir,
          offlineEnv,
        ),
        executableAvailable(ffmpeg, ["-version"], stateDir, offlineEnv),
        executableAvailable(ffprobe, ["-version"], stateDir, offlineEnv),
        executableAvailable(whisper, ["--help"], stateDir, offlineEnv),
      ]);
      let sqlite = false;
      try {
        sqlite = currentStore.db.prepare("SELECT 1").get() !== undefined;
      } catch {
        sqlite = false;
      }
      const components = {
        output,
        state,
        sqlite,
        reelmaestro,
        ffmpeg: ffmpegReady,
        ffprobe: ffprobeReady,
        whisper: whisperReady,
      };
      const ready = Object.values(components).every(Boolean);
      const response = {
        ready,
        components,
        providerConfigured: Boolean(
          await providerKey(operatorEnv, credentialFile),
        ),
      };
      return ready ? response : reply.code(503).send(response);
    });

    app.get("/api/runs", async () => ({ runs: await library.summaries() }));
    app.get<{ Params: { id: string } }>(
      "/api/runs/:id",
      async (request, reply) =>
        (await library.detail(request.params.id)) ??
        reply.code(404).send({ error: "Run not found" }),
    );

    app.post<{ Body: unknown }>(
      "/api/uploads",
      { bodyLimit: 70_000_000 },
      async (request, reply) => {
        const result = await assets.create(request.body);
        return typeof result === "string"
          ? reply.code(400).send({ error: result })
          : result;
      },
    );
    app.post<{ Body: unknown }>("/api/sources/url", async (request, reply) => {
      const body = plainObject(request.body);
      if (
        !body ||
        Object.keys(body).some((key) => key !== "url") ||
        typeof body.url !== "string" ||
        body.url.length > 4096
      )
        return reply.code(400).send({ error: "Invalid URL" });
      const result = await (options.urlFetcher ?? fetchUrlSource)(body.url);
      return typeof result === "string"
        ? reply.code(400).send({ error: result })
        : result;
    });
    app.route<{ Params: { id: string; name: string } }>({
      method: ["GET", "HEAD"],
      url: "/api/runs/:id/artifacts/:name",
      handler: async (request, reply) => {
        const artifact = await library.artifact(
          request.params.id,
          request.params.name,
        );
        if (!artifact)
          return reply.code(404).send({ error: "Artifact not found" });
        reply.header("Accept-Ranges", "bytes");
        reply.header("Content-Type", artifact.mime);
        reply.header(
          "Content-Disposition",
          `inline; filename="${request.params.name}"`,
        );
        const artifactSize = Number(artifact.stat.size);
        if (!Number.isSafeInteger(artifactSize) || artifactSize < 0)
          return reply.code(500).send({ error: "Artifact is unavailable" });
        const parsed = parseRange(request.headers.range, artifactSize);
        if (parsed === "invalid")
          return reply
            .code(416)
            .header("Content-Range", `bytes */${artifactSize}`)
            .send();
        const start = parsed?.start ?? 0;
        const end = parsed?.end ?? artifactSize - 1;
        if (parsed)
          reply
            .code(206)
            .header("Content-Range", `bytes ${start}-${end}/${artifactSize}`);
        reply.header(
          "Content-Length",
          String(artifactSize === 0 ? 0 : end - start + 1),
        );
        if (request.method === "HEAD" || artifactSize === 0)
          return reply.send();
        return reply.send(createReadStream(artifact.path, { start, end }));
      },
    });

    app.get("/api/settings", async () => ({
      providerConnected: Boolean(
        await providerKey(operatorEnv, credentialFile),
      ),
      outputLocation: outDir,
      generationAvailable: await regularFile(binary),
    }));
    app.post<{ Body: unknown }>("/api/settings", async (request, reply) => {
      const apiKey = apiKeyBody(request.body);
      if (apiKey === undefined)
        return reply
          .code(400)
          .send({ error: "apiKey must be a non-empty string or null" });
      if (apiKey === null) await rm(credentialFile, { force: true });
      else {
        const temporary = `${credentialFile}.${randomUUID()}.tmp`;
        await writeFile(temporary, JSON.stringify({ apiKey }), { mode: 0o600 });
        await chmod(temporary, 0o600);
        await rename(temporary, credentialFile);
      }
      return { ok: true };
    });

    app.get("/api/capabilities", async () => {
      if (!(await regularFile(binary)))
        return {
          available: false,
          error: "Reel Maestro binary is unavailable",
        };
      const env = await childEnvironment(operatorEnv, credentialFile);
      const result = await runCommand(
        binary,
        ["--no-dotenv", "--studio-schema"],
        { cwd: stateDir, env },
        commandTimeoutMs,
      );
      const schema =
        result.code === 0 && !result.timedOut && !result.outputExceeded
          ? parseJsonObject(result.stdout)
          : undefined;
      return schema
        ? { available: true, schema }
        : {
            available: false,
            error: "This Reel Maestro binary does not support Studio",
          };
    });

    app.post<{ Body: unknown }>("/api/plans", async (request, reply) => {
      let checked = validatePlan(request.body);
      if (typeof checked === "string")
        return reply.code(400).send({ error: checked });
      if (checked.sourceAssetId) {
        const uploaded = await assets.resolve(checked.sourceAssetId, "text");
        if (!uploaded?.metadata.text)
          return reply.code(400).send({ error: "Source asset is invalid" });
        checked = { ...checked, prompt: uploaded.metadata.text };
      }
      if (!(await regularFile(binary)))
        return reply.code(503).send({ error: "Generation is unavailable" });
      const planDirectory = path.join(stateDir, "plans", randomUUID());
      await mkdir(planDirectory, { recursive: true, mode: 0o700 });
      const args = await buildArgs(checked, planDirectory, jobsDir, assets);
      const env = await childEnvironment(operatorEnv, credentialFile);
      const fingerprint = await configurationFingerprint(binary, env, args);
      const result = await runCommand(
        binary,
        ["--studio-estimate", ...args],
        { cwd: planDirectory, env },
        commandTimeoutMs,
      );
      const estimate =
        result.code === 0 && !result.timedOut && !result.outputExceeded
          ? parseEstimate(result.stdout)
          : undefined;
      if (!estimate) {
        await rm(planDirectory, { recursive: true, force: true });
        return reply.code(503).send({
          error:
            "Cost estimation is unavailable; generation cannot be approved safely",
        });
      }
      const canonical = JSON.stringify({
        input: checked,
        args,
        estimate,
        fingerprint,
      });
      const hash = createHash("sha256").update(canonical).digest("hex");
      const id = path.basename(planDirectory);
      const expires = Date.now() + 15 * 60_000;
      currentStore.addPlan({
        id,
        hash,
        expires,
        input: checked,
        args,
        fingerprint,
        estimatedTotal: estimate.total_usd,
      });
      return {
        planId: id,
        planHash: hash,
        expiresAt: new Date(expires).toISOString(),
        estimate,
      };
    });

    app.post<{ Params: { id: string }; Body: unknown }>(
      "/api/runs/:id/plans",
      async (request, reply) => {
        const input = validateRevision(request.body);
        if (typeof input === "string")
          return reply.code(400).send({ error: input });
        if (input.advanced?.noImages)
          return reply
            .code(400)
            .send({ error: "Timing-only diagnostics are not valid revisions" });
        const source = await library.source(request.params.id);
        if (!source) return reply.code(404).send({ error: "Run not found" });
        if (source.fingerprint !== input.expectedSourceFingerprint)
          return reply
            .code(409)
            .send({ error: "Source changed; reopen or rebase this draft" });
        const directory = path.join(stateDir, "revision-plans", randomUUID());
        await mkdir(directory, { recursive: true, mode: 0o700 });
        const revisionId = randomUUID();
        const jobId = randomUUID();
        const args = await revisionArgs(input, assets);
        const revisionRequest = {
          version: 1,
          source: source.directory,
          revision_id: revisionId,
          script: overlayScript(source.script, input),
          arguments: args,
          legacy_reuse: input.legacyReuse ?? "reject",
          job_id: jobId,
        };
        const requestPath = path.join(directory, "request.json");
        const planPath = path.join(directory, "plan.json");
        await writeFile(requestPath, JSON.stringify(revisionRequest), {
          mode: 0o600,
        });
        const env = await childEnvironment(operatorEnv, credentialFile);
        const fingerprint = await configurationFingerprint(binary, env);
        const result = await runCommand(
          binary,
          ["--no-dotenv", "--revision-plan", requestPath],
          { cwd: directory, env },
          commandTimeoutMs,
        );
        const parsed =
          result.code === 0 && !result.timedOut && !result.outputExceeded
            ? parseRevisionPlan(result.stdout, {
                source: source.directory,
                revisionId,
                jobId,
              })
            : undefined;
        if (!parsed) {
          await rm(directory, { recursive: true, force: true });
          return reply
            .code(409)
            .send({ error: "Revision could not be planned safely" });
        }
        await writeFile(planPath, result.stdout, { mode: 0o600 });
        const planId = path.basename(directory);
        const expires = Date.now() + 15 * 60_000;
        currentStore.addRevisionPlan({
          id: planId,
          hash: parsed.approvalHash,
          expires,
          requestPath,
          planPath,
          sourceId: request.params.id,
          fingerprint,
          estimatedTotal: parsed.estimate.total_usd,
          localWork: parsed.localWork,
          revisionId,
          maxCost: input.maxCost,
          jobId,
        });
        const response: RevisionPlan = {
          planId,
          planHash: parsed.approvalHash,
          expiresAt: new Date(expires).toISOString(),
          estimate: parsed.estimate,
          revisionId,
          parentRunId: request.params.id,
          actions: parsed.actions,
          localWork: parsed.localWork,
          warnings: parsed.warnings,
        };
        return response;
      },
    );

    app.post<{ Body: unknown }>("/api/jobs", async (request, reply) => {
      const approval = approvalBody(request.body);
      if (!approval)
        return reply.code(400).send({
          error: "Explicit plan approval and valid idempotencyKey required",
        });
      const old = currentStore.byIdem(approval.idempotencyKey);
      if (old)
        return old.planId === approval.planId
          ? old.job
          : reply
              .code(409)
              .send({ error: "Idempotency key belongs to a different plan" });
      const existingPlanJob = currentStore.byPlan(approval.planId);
      if (existingPlanJob)
        return reply
          .code(409)
          .send({ error: "This approved plan already created a job" });
      const revisionPlan = currentStore.revisionPlan(approval.planId);
      if (revisionPlan) {
        if (
          revisionPlan.hash !== approval.planHash ||
          revisionPlan.expires < Date.now()
        )
          return reply.code(409).send({
            error: "Plan is stale or changed; request a new estimate",
          });
        const env = await childEnvironment(operatorEnv, credentialFile);
        if (
          revisionPlan.fingerprint !==
          (await configurationFingerprint(binary, env))
        )
          return reply.code(409).send({
            error:
              "Credentials or generation configuration changed; request a new estimate",
          });
        if (revisionPlan.estimatedTotal > revisionPlan.maxCost)
          return reply
            .code(409)
            .send({ error: "Estimated cost exceeds the configured maximum" });
        if (
          revisionPlan.estimatedTotal > 0 &&
          !(await providerKey(operatorEnv, credentialFile))
        )
          return reply
            .code(409)
            .send({ error: "Connect a provider before submitting generation" });
        const racedKey = currentStore.byIdem(approval.idempotencyKey);
        if (racedKey)
          return racedKey.planId === approval.planId
            ? racedKey.job
            : reply
                .code(409)
                .send({ error: "Idempotency key belongs to a different plan" });
        if (currentStore.byPlan(approval.planId))
          return reply
            .code(409)
            .send({ error: "This approved plan already created a job" });
        const job = currentStore.createRevisionJob({
          id: revisionPlan.jobId,
          planId: revisionPlan.id,
          idem: approval.idempotencyKey,
          plan: revisionPlan,
          maxCost: revisionPlan.maxCost,
        });
        schedulePump();
        return reply.code(202).send(job);
      }
      const plan = currentStore.plan(approval.planId);
      if (!plan || plan.hash !== approval.planHash || plan.expires < Date.now())
        return reply
          .code(409)
          .send({ error: "Plan is stale or changed; request a new estimate" });
      if (!(await providerKey(operatorEnv, credentialFile)))
        return reply
          .code(409)
          .send({ error: "Connect a provider before submitting generation" });
      const env = await childEnvironment(operatorEnv, credentialFile);
      if (
        plan.fingerprint !==
        (await configurationFingerprint(binary, env, plan.args))
      )
        return reply.code(409).send({
          error:
            "Credentials or generation configuration changed; request a new estimate",
        });
      if (plan.estimatedTotal > plan.input.maxCost)
        return reply
          .code(409)
          .send({ error: "Estimated cost exceeds the configured maximum" });
      const racedKey = currentStore.byIdem(approval.idempotencyKey);
      if (racedKey)
        return racedKey.planId === approval.planId
          ? racedKey.job
          : reply
              .code(409)
              .send({ error: "Idempotency key belongs to a different plan" });
      if (currentStore.byPlan(approval.planId))
        return reply
          .code(409)
          .send({ error: "This approved plan already created a job" });
      const job = currentStore.createJob({
        id: randomUUID(),
        planId: plan.id,
        idem: approval.idempotencyKey,
        args: plan.args,
        input: plan.input,
        fingerprint: plan.fingerprint,
        estimatedTotal: plan.estimatedTotal,
      });
      schedulePump();
      return reply.code(202).send(job);
    });

    app.get("/api/jobs", async () => ({ jobs: currentStore.jobs() }));
    app.get<{ Params: { id: string } }>(
      "/api/jobs/:id",
      async (request, reply) =>
        currentStore.job(request.params.id) ??
        reply.code(404).send({ error: "Job not found" }),
    );
    app.post<{ Params: { id: string } }>(
      "/api/jobs/:id/cancel",
      async (request, reply) => {
        const job = currentStore.job(request.params.id);
        if (!job) return reply.code(404).send({ error: "Job not found" });
        if (job.status === "queued")
          transition(job.id, "cancelled", "Cancelled before start");
        else if (job.status === "running" && active?.id === job.id) {
          transition(job.id, "cancelling", "Cancellation requested");
          terminate(active);
        }
        return currentStore.job(job.id);
      },
    );
    app.post<{ Params: { id: string } }>(
      "/api/jobs/:id/resume",
      async (request, reply) => {
        const job = currentStore.rawJob(request.params.id);
        const publicJob = currentStore.job(request.params.id);
        if (
          !job?.revisionPlanPath ||
          job.status !== "interrupted" ||
          publicJob?.recoverable !== true
        )
          return reply
            .code(409)
            .send({ error: "Only interrupted revisions can be recovered" });
        currentStore.requeueRevision(job.id);
        schedulePump();
        return reply.code(202).send(currentStore.job(job.id));
      },
    );
    app.get<{ Params: { id: string } }>(
      "/api/jobs/:id/events",
      async (request, reply) => {
        if (!currentStore.job(request.params.id))
          return reply.code(404).send({ error: "Job not found" });
        const after = lastEventId(request.headers["last-event-id"]);
        if (after === undefined)
          return reply.code(400).send({ error: "Invalid Last-Event-ID" });
        reply.hijack();
        reply.raw.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-store",
          Connection: "keep-alive",
          "X-Accel-Buffering": "no",
        });
        reply.raw.write(": connected\n\n");
        for (const event of currentStore.events(request.params.id, after, 1000))
          reply.raw.write(encodeEvent(event));
        const clients =
          subscribers.get(request.params.id) ?? new Set<ServerResponse>();
        clients.add(reply.raw);
        subscribers.set(request.params.id, clients);
        const heartbeat = setInterval(
          () => reply.raw.write(": heartbeat\n\n"),
          15_000,
        );
        heartbeat.unref();
        request.raw.on("close", () => {
          clearInterval(heartbeat);
          clients.delete(reply.raw);
          if (!clients.size) subscribers.delete(request.params.id);
        });
      },
    );

    function schedulePump(): void {
      if (paused) return;
      if (pumpTask) {
        pumpAgain = true;
        return;
      }
      pumpTask = pump()
        .catch(() => undefined)
        .finally(() => {
          pumpTask = undefined;
          if (pumpAgain) {
            pumpAgain = false;
            schedulePump();
          }
        });
    }

    async function pump(): Promise<void> {
      if (pumping || paused || active) return;
      pumping = true;
      try {
        while (!paused && !active) {
          const job = currentStore.next();
          if (!job) break;
          try {
            const env = await childEnvironment(operatorEnv, credentialFile);
            const fingerprint = await configurationFingerprint(
              binary,
              env,
              job.args,
            );
            if (paused) break;
            if (
              (!job.revisionPlanPath && !env.OPENROUTER_API_KEY) ||
              job.fingerprint !== fingerprint
            ) {
              transition(
                job.id,
                "failed",
                "Generation configuration changed; request a new estimate",
                {
                  error:
                    "Generation configuration changed; request a new estimate",
                },
              );
              continue;
            }
            if (job.estimatedTotal > job.input.maxCost) {
              transition(
                job.id,
                "failed",
                "Estimated cost exceeds the configured maximum",
                { error: "Estimated cost exceeds the configured maximum" },
              );
              continue;
            }
            await startJob(job, env);
          } catch {
            if (currentStore.job(job.id)?.status === "queued")
              transition(job.id, "failed", "Generation could not be started", {
                error: "Generation could not be started",
              });
          }
        }
      } finally {
        pumping = false;
      }
    }

    async function startJob(
      job: StoredJob,
      env: NodeJS.ProcessEnv,
    ): Promise<void> {
      const directory = path.join(jobsDir, job.id);
      await mkdir(directory, { recursive: true, mode: 0o700 });
      const revisionPlan = currentStore.revisionPlan(job.planId);
      const args = job.revisionPlanPath
        ? [
            "--no-dotenv",
            job.recovery ? "--revision-recover" : "--revision-execute",
            job.revisionPlanPath,
            "--approval-hash",
            revisionPlan?.hash ?? "",
            "--events-json",
          ]
        : await materialize(job.args, job.input, directory);
      transition(job.id, "running", "Generation launch committed");
      let child: ChildProcess;
      try {
        child = spawn(binary, args, {
          cwd: directory,
          detached: true,
          stdio: ["ignore", job.revisionPlanPath ? "pipe" : "ignore", "pipe"],
          env,
        });
      } catch {
        transition(job.id, "failed", "Generation process failed", {
          error: "Generation process failed",
        });
        return;
      }
      let resolveDone = (): void => undefined;
      const done = new Promise<void>((resolve) => {
        resolveDone = resolve;
      });
      const current: ActiveJob = {
        id: job.id,
        child,
        terminal: false,
        done,
        resolveDone,
        timedOut: false,
        ...(job.revisionId ? { revisionId: job.revisionId } : {}),
        timingOnly: job.args.includes("--no-images"),
      };
      active = current;
      const timeout = setTimeout(
        () => {
          current.timedOut = true;
          terminate(current);
        },
        options.jobTimeoutMs ?? 2 * 60 * 60_000,
      );
      timeout.unref();
      child.stderr?.resume();
      let eventBuffer = "";
      child.stdout?.on("data", (chunk) => {
        eventBuffer += Buffer.isBuffer(chunk)
          ? chunk.toString("utf8")
          : String(chunk);
        if (eventBuffer.length > MAX_STDOUT) {
          current.timedOut = true;
          terminate(current);
          return;
        }
        const lines = eventBuffer.split("\n");
        eventBuffer = lines.pop() ?? "";
        for (const line of lines) {
          const event = parseNativeEvent(line);
          if (!event) continue;
          if (event.state === "failed" && event.code)
            current.nativeFailureCode = event.code;
          currentStore.event(
            current.id,
            "running",
            event.message ?? event.stage,
            event.timestamp,
            {
              stage: event.stage,
              ...(event.sceneId ? { sceneId: event.sceneId } : {}),
              kind: event.state === "warning" ? "warning" : "progress",
            },
          );
          notify(current.id);
        }
      });
      child.once("error", () => {
        void finish(current, null, timeout, true);
      });
      child.once("close", (code) => {
        void finish(current, code, timeout, false);
      });
    }

    function terminate(job: ActiveJob): void {
      try {
        if (job.child.pid) process.kill(-job.child.pid, "SIGTERM");
      } catch {
        /* process already stopped */
      }
      if (!job.killTimer) {
        job.killTimer = setTimeout(() => {
          try {
            if (job.child.pid) process.kill(-job.child.pid, "SIGKILL");
          } catch {
            /* process already stopped */
          }
        }, terminateGraceMs);
        job.killTimer.unref();
      }
    }

    async function finish(
      current: ActiveJob,
      code: number | null,
      timeout: NodeJS.Timeout,
      spawnFailed: boolean,
    ): Promise<void> {
      if (current.terminal) return;
      current.terminal = true;
      clearTimeout(timeout);
      if (current.killTimer) clearTimeout(current.killTimer);
      try {
        if (!shuttingDown) {
          const status = currentStore.job(current.id)?.status;
          if (status === "cancelling")
            transition(current.id, "cancelled", "Generation cancelled", {
              error: "Generation cancelled",
            });
          else if (current.timedOut)
            transition(current.id, "failed", "Generation timed out", {
              error: "Generation timed out",
            });
          else if (
            current.revisionId &&
            code === 75 &&
            current.nativeFailureCode === "accepted_provider_job_pending"
          )
            currentStore.setRevisionInterruption(
              current.id,
              true,
              false,
              "Accepted provider work is pending; explicit recovery can poll it safely",
            );
          else if (
            current.revisionId &&
            code === 75 &&
            current.nativeFailureCode === "ambiguous_paid_work"
          )
            currentStore.setRevisionInterruption(
              current.id,
              false,
              true,
              "Paid work has ambiguous acceptance; operator resolution and a new approval are required",
            );
          else if (spawnFailed || code !== 0)
            transition(current.id, "failed", "Generation process failed", {
              error: "Generation process failed",
            });
          else if (current.revisionId) {
            const sourceRunId = currentStore.rawJob(current.id)?.sourceRunId;
            const runId = await library.runIdForRevision(
              current.revisionId,
              sourceRunId,
            );
            if (runId)
              transition(current.id, "succeeded", "Revision completed", {
                runId,
              });
            else
              transition(
                current.id,
                "failed",
                "Published revision could not be indexed",
                { error: "Published revision could not be indexed" },
              );
          } else if (current.timingOnly) {
            const runDirectory = await findDiagnostic(
              path.join(jobsDir, current.id),
            );
            const runId = runDirectory
              ? await library.runIdForDirectory(runDirectory)
              : undefined;
            if (runId)
              transition(
                current.id,
                "succeeded",
                "Timing diagnostic completed",
                { runId },
              );
            else
              transition(
                current.id,
                "failed",
                "Timing diagnostic output was invalid",
                { error: "Timing diagnostic output was invalid" },
              );
          } else {
            const result = await findVideo(path.join(jobsDir, current.id));
            if (
              !result ||
              !(await validVideo(
                ffprobe,
                result.path,
                commandTimeoutMs,
                await childEnvironment(operatorEnv, credentialFile).then(
                  (env) => {
                    delete env.OPENROUTER_API_KEY;
                    return env;
                  },
                ),
              ))
            ) {
              transition(
                current.id,
                "failed",
                "No valid resulting video was produced",
                { error: "No valid resulting video was produced" },
              );
            } else {
              const runId = await library.runIdForDirectory(
                result.runDirectory,
              );
              if (!runId)
                transition(
                  current.id,
                  "failed",
                  "Generated project could not be indexed",
                  { error: "Generated project could not be indexed" },
                );
              else
                transition(current.id, "succeeded", "Generation completed", {
                  runId,
                });
            }
          }
        }
      } catch {
        if (!shuttingDown)
          transition(
            current.id,
            "failed",
            "Generation result validation failed",
            { error: "Generation result validation failed" },
          );
      } finally {
        if (active === current) active = undefined;
        current.resolveDone();
        if (!shuttingDown) schedulePump();
      }
    }

    const webDir = path.resolve(
      options.webDir ?? fileURLToPath(new URL("../web/dist", import.meta.url)),
    );
    if (await directoryExists(webDir)) {
      await app.register(fastifyStatic, { root: webDir });
      app.setNotFoundHandler((request, reply) =>
        request.url.startsWith("/api/")
          ? reply.code(404).send({ error: "Not found" })
          : reply.sendFile("index.html"),
      );
    }
    app.addHook("preClose", async () => {
      paused = true;
      shuttingDown = true;
      for (const clients of subscribers.values())
        for (const client of clients) client.end();
      subscribers.clear();
      await pumpTask;
      const current = active;
      if (current) {
        terminate(current);
        await current.done;
        const status = currentStore.job(current.id)?.status;
        if (status === "running" || status === "cancelling")
          if (current.revisionId)
            currentStore.setRevisionInterruption(
              current.id,
              true,
              false,
              "Server stopped; revision recovery requires an explicit request",
            );
          else
            transition(
              current.id,
              "interrupted",
              "Server stopped; paid work was not relaunched",
              { error: "Generation was interrupted by server shutdown" },
            );
      }
    });
    app.addHook("onClose", async () => {
      currentStore.db.close();
      lock.release();
    });
    schedulePump();
    return app;
  } catch (error) {
    try {
      store?.db.close();
    } catch {
      /* initialization did not complete */
    }
    lock.release();
    throw error;
  }
}

async function runCommand(
  binary: string,
  args: string[],
  options: SpawnOptionsWithoutStdio,
  timeoutMs: number,
): Promise<CommandResult> {
  return new Promise((resolve) => {
    let stdout = "";
    let outputExceeded = false;
    let timedOut = false;
    let settled = false;
    const child = spawn(binary, args, {
      ...options,
      shell: false,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const finish = (code: number | null): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ code, stdout, timedOut, outputExceeded });
    };
    child.stdout?.on("data", (chunk) => {
      const value = Buffer.isBuffer(chunk)
        ? chunk.toString("utf8")
        : String(chunk);
      if (Buffer.byteLength(stdout) + Buffer.byteLength(value) > MAX_STDOUT) {
        outputExceeded = true;
        child.kill("SIGKILL");
      } else stdout += value;
    });
    child.once("error", () => finish(null));
    child.once("close", (code) => finish(code));
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    timer.unref();
  });
}

function validatePlan(value: unknown): PlanInput | string {
  const input = plainObject(value);
  if (!input) return "Invalid plan";
  const keys = new Set([
    "prompt",
    "source",
    "format",
    "quality",
    "minutes",
    "narration",
    "music",
    "video",
    "captions",
    "captionStyle",
    "maxCost",
    "speed",
    "voice",
    "sourceAssetId",
    "advanced",
  ]);
  if (Object.keys(input).some((key) => !keys.has(key)))
    return "Unknown plan field";
  if (
    typeof input.prompt !== "string" ||
    (!input.prompt.trim() && typeof input.sourceAssetId !== "string") ||
    input.prompt.length > 100_000
  )
    return "prompt must be 1-100000 characters";
  if (/https?:\/\//i.test(input.prompt))
    return "Fetch the URL through Studio before planning";
  if (!oneOf(input.source, ["topic", "script", "brief", "url"]))
    return "Invalid plan option";
  if (!oneOf(input.format, ["reel", "youtube"])) return "Invalid plan option";
  if (!oneOf(input.quality, ["draft", "standard", "premium"]))
    return "Invalid plan option";
  if (!oneOf(input.video, ["off", "all"])) return "Invalid plan option";
  if (!oneOf(input.captionStyle, ["burst", "karaoke", "boxed", "minimal"]))
    return "Invalid plan option";
  if (
    typeof input.narration !== "boolean" ||
    typeof input.music !== "boolean" ||
    typeof input.captions !== "boolean"
  )
    return "Boolean options are required";
  if (!finiteRange(input.maxCost, 0, 1000))
    return "maxCost must be between 0 and 1000";
  if (input.minutes !== undefined && !finiteRange(input.minutes, 1, 12))
    return "minutes must be 1-12";
  if (input.speed !== undefined && !finiteRange(input.speed, 0.5, 2))
    return "speed must be 0.5-2";
  if (
    input.voice !== undefined &&
    (typeof input.voice !== "string" ||
      input.voice.length > 100 ||
      !/^[\w .-]*$/.test(input.voice))
  )
    return "Invalid voice";
  if (
    input.sourceAssetId !== undefined &&
    (typeof input.sourceAssetId !== "string" ||
      !/^[A-Za-z0-9_-]{32}\.[A-Za-z0-9_-]{43}$/.test(input.sourceAssetId) ||
      input.source === "topic")
  )
    return "Invalid sourceAssetId";
  const advanced =
    input.advanced === undefined ? undefined : validateAdvanced(input.advanced);
  if (typeof advanced === "string") return advanced;
  return {
    prompt: input.prompt,
    source: input.source,
    format: input.format,
    quality: input.quality,
    narration: input.narration,
    music: input.music,
    video: input.video,
    captions: input.captions,
    captionStyle: input.captionStyle,
    maxCost: input.maxCost,
    ...(typeof input.minutes === "number" ? { minutes: input.minutes } : {}),
    ...(typeof input.speed === "number" ? { speed: input.speed } : {}),
    ...(typeof input.voice === "string" ? { voice: input.voice } : {}),
    ...(typeof input.sourceAssetId === "string"
      ? { sourceAssetId: input.sourceAssetId }
      : {}),
    ...(advanced ? { advanced } : {}),
  };
}

async function buildArgs(
  input: PlanInput,
  planDirectory: string,
  jobsDirectory: string,
  assets: AssetStore,
): Promise<string[]> {
  const args = [
    "--no-dotenv",
    "--out",
    jobsDirectory,
    "--format",
    input.format,
    "--quality",
    input.quality,
    "--caption-style",
    input.captionStyle,
    "--max-cost",
    String(input.maxCost),
  ];
  if (input.source === "topic") args.push("--topic", input.prompt);
  else {
    const sourceFlag = input.source === "url" ? "brief" : input.source;
    const file = path.join(planDirectory, `${sourceFlag}.txt`);
    await writeFile(file, input.prompt, { mode: 0o600 });
    args.push(`--${sourceFlag}`, file);
  }
  if (input.minutes !== undefined)
    args.push("--minutes", String(input.minutes));
  if (!input.narration) args.push("--no-narration");
  if (input.music) args.push("--music-gen");
  if (input.video === "all") args.push("--video");
  if (!input.captions) args.push("--no-captions");
  if (input.speed !== undefined) args.push("--speed", String(input.speed));
  if (input.voice) args.push("--voice", input.voice);
  if (input.advanced) await appendAdvancedArgs(args, input.advanced, assets);
  return args;
}

async function appendAdvancedArgs(
  args: string[],
  advanced: NonNullable<PlanInput["advanced"]>,
  assets: AssetStore,
): Promise<void> {
  const strings: ReadonlyArray<[keyof typeof advanced, string]> = [
    ["textModel", "--text-model"],
    ["imageModel", "--image-model"],
    ["judgeModel", "--judge-model"],
    ["ttsModel", "--tts-model"],
    ["musicModel", "--music-model"],
    ["captionFont", "--caption-font"],
    ["whisperModel", "--whisper-model"],
    ["videoProvider", "--video-provider"],
    ["videoModel", "--video-model"],
    ["videoResolution", "--video-resolution"],
    ["videoSize", "--video-size"],
    ["videoInputMode", "--video-input-mode"],
  ];
  const numbers: ReadonlyArray<[keyof typeof advanced, string]> = [
    ["sceneSeconds", "--scene-seconds"],
    ["validateScene", "--validate-scene"],
    ["videoSeed", "--video-seed"],
    ["videoSteps", "--video-steps"],
    ["videoWaitTimeout", "--video-wait-timeout"],
    ["videoScenes", "--video-scenes"],
    ["musicVolume", "--music-volume"],
    ["dissolveSeconds", "--dissolve-seconds"],
    ["posterScene", "--poster-scene"],
  ];
  for (const [name, flag] of [...strings, ...numbers]) {
    const value = advanced[name];
    if (typeof value === "string" || typeof value === "number")
      args.push(flag, String(value));
  }
  if (advanced.consistency !== undefined)
    args.push(advanced.consistency ? "--consistency" : "--no-consistency");
  if (advanced.mix !== undefined)
    args.push("--mix", advanced.mix ? "duck" : "bed");
  if (advanced.dissolve !== undefined)
    args.push(advanced.dissolve ? "--dissolve" : "--no-dissolve");
  if (advanced.grade !== undefined)
    args.push(advanced.grade ? "--grade" : "--no-grade");
  if (advanced.loudnorm !== undefined)
    args.push(advanced.loudnorm ? "--loudnorm" : "--no-loudnorm");
  if (advanced.embedPoster !== undefined)
    args.push(advanced.embedPoster ? "--embed-poster" : "--no-embed-poster");
  if (advanced.noImages) args.push("--no-images");
  if (advanced.verbose) args.push("--verbose");
  if (advanced.whisperExecutable)
    args.push("--whisper-cmd", advanced.whisperExecutable);
  if (advanced.exportPreset)
    args.push("--export-preset", advanced.exportPreset);
  for (const [name, kind, flag] of [
    ["characterReferenceAssetId", "image", "--character-ref"],
    ["musicAssetId", "audio", "--music"],
    ["watermarkAssetId", "image", "--watermark"],
  ] as const) {
    const assetId = advanced[name];
    if (assetId) {
      const uploaded = await assets.resolve(assetId, kind);
      if (!uploaded)
        throw Object.assign(new Error("Invalid upload asset"), {
          statusCode: 400,
        });
      args.push(flag, uploaded.path);
    }
  }
}

async function revisionArgs(
  input: RevisionInput,
  assets: AssetStore,
): Promise<string[]> {
  const args: string[] = [];
  if (input.narrationEnabled === false) args.push("--no-narration");
  else if (input.narrationEnabled === true) args.push("--narration");
  if (input.captions === false) args.push("--no-captions");
  else if (input.captions === true) args.push("--captions");
  if (input.captionStyle) args.push("--caption-style", input.captionStyle);
  if (input.voice) args.push("--voice", input.voice);
  if (input.speed !== undefined) args.push("--speed", String(input.speed));
  if (input.quality) args.push("--quality", input.quality);
  if (input.format) args.push("--format", input.format);
  if (input.exportPreset) args.push("--export-preset", input.exportPreset);
  args.push(
    "--max-cost",
    String(input.maxCost),
    "--music-action",
    input.musicAction,
  );
  for (const id of input.videoSceneIds) args.push("--video-scene-id", id);
  for (const scene of input.scenes) {
    if (scene.imageAction)
      args.push(`--${scene.imageAction}-image-scene-id`, scene.id);
    if (scene.motionAction) {
      if (!input.videoSceneIds.includes(scene.id))
        throw Object.assign(
          new Error("Motion action requires video selection"),
          { statusCode: 400 },
        );
      args.push(`--${scene.motionAction}-video-scene-id`, scene.id);
    }
  }
  if (input.advanced) await appendAdvancedArgs(args, input.advanced, assets);
  return args;
}

function overlayScript(
  source: Record<string, unknown>,
  input: RevisionInput,
): Record<string, unknown> {
  const priorScenes = new Map<string, Record<string, unknown>>();
  if (Array.isArray(source.scenes))
    for (const value of source.scenes) {
      const scene = plainObject(value);
      if (scene && typeof scene.id === "string")
        priorScenes.set(scene.id, scene);
    }
  return {
    ...source,
    scenes: input.scenes.map((scene) => ({
      ...(priorScenes.get(scene.id) ?? {}),
      id: scene.id,
      line: scene.line,
      image_prompt: scene.imagePrompt,
      motion_prompt: scene.motionPrompt,
      ...(scene.castIds ? { cast_ids: scene.castIds } : {}),
      ...(scene.locationId !== undefined
        ? { location_id: scene.locationId }
        : {}),
      ...(scene.transition ? { transition: scene.transition } : {}),
    })),
    ...(input.chapters
      ? {
          chapters: input.chapters.map((chapter) => ({
            title: chapter.title,
            summary: chapter.summary,
            narration: chapter.narration,
            scene_start: chapter.sceneStart,
            scene_count: chapter.sceneCount,
          })),
        }
      : {}),
    ...(input.characters ? { characters: input.characters } : {}),
    ...(input.locations ? { locations: input.locations } : {}),
    ...(input.posterPrompt !== undefined
      ? { poster_prompt: input.posterPrompt }
      : {}),
    ...(input.narration !== undefined ? { narration: input.narration } : {}),
    ...(input.musicPrompt !== undefined
      ? { music_prompt: input.musicPrompt }
      : {}),
  };
}

function parseRevisionPlan(
  source: string,
  expected: { source: string; revisionId: string; jobId: string },
):
  | {
      approvalHash: string;
      estimate: CostEstimate;
      actions: RevisionPlan["actions"];
      localWork: boolean;
      warnings: string[];
    }
  | undefined {
  const value = parseJsonObject(source);
  const cost = plainObject(value?.cost);
  const request = plainObject(value?.request);
  const runRoot =
    path.basename(path.dirname(expected.source)) === "revisions"
      ? path.dirname(path.dirname(expected.source))
      : expected.source;
  const expectedDestination = path.join(
    runRoot,
    "revisions",
    expected.revisionId,
  );
  if (
    !value ||
    value.version !== 1 ||
    value.source !== expected.source ||
    value.destination !== expectedDestination ||
    !request ||
    request.source !== expected.source ||
    request.revision_id !== expected.revisionId ||
    request.job_id !== expected.jobId ||
    typeof value.approval_hash !== "string" ||
    !/^sha256:[a-f0-9]{64}$/.test(value.approval_hash) ||
    typeof value.local_work !== "boolean" ||
    !Array.isArray(value.warnings) ||
    value.warnings.some((warning) => typeof warning !== "string") ||
    !Array.isArray(value.actions) ||
    !cost
  )
    return undefined;
  const total = nonnegative(cost.total_usd);
  const script = nonnegative(cost.script_usd);
  const narration = nonnegative(cost.narration_usd);
  const images = nonnegative(cost.images_usd);
  const video = nonnegative(cost.video_usd);
  const music = nonnegative(cost.music_usd);
  if (
    total === undefined ||
    script === undefined ||
    narration === undefined ||
    images === undefined ||
    video === undefined ||
    music === undefined
  )
    return undefined;
  const warnings = value.warnings
    .filter((warning): warning is string => typeof warning === "string")
    .map((warning) =>
      redactBackendPaths(warning, expected.source, expectedDestination),
    )
    .slice(0, 100);
  const actions: RevisionPlan["actions"] = [];
  for (const item of value.actions.slice(0, 5000)) {
    const action = plainObject(item);
    if (
      !action ||
      typeof action.artifact !== "string" ||
      !/^[A-Za-z0-9:_-]{1,500}$/.test(action.artifact) ||
      !oneOf(action.action, ["reuse", "generate", "derive", "exclude"]) ||
      typeof action.reason !== "string"
    )
      return undefined;
    const safeSource =
      typeof action.source === "string" &&
      path.basename(action.source) === action.source
        ? action.source
        : undefined;
    const safeDestination =
      typeof action.destination === "string" &&
      path.basename(action.destination) === action.destination
        ? action.destination
        : undefined;
    actions.push({
      artifact: action.artifact.slice(0, 500),
      action: action.action,
      reason: redactBackendPaths(
        action.reason,
        expected.source,
        expectedDestination,
      ).slice(0, 1000),
      ...(safeSource ? { source: safeSource } : {}),
      ...(safeDestination ? { destination: safeDestination } : {}),
    });
  }
  const estimate: CostEstimate = {
    version: 1,
    total_usd: total,
    script_usd: script,
    narration_usd: narration,
    images_usd: images,
    video_usd: video,
    music_usd: music,
    warning: [
      ...warnings,
      cost.uncertain === true ? "Cost estimate is uncertain" : "",
    ]
      .filter(Boolean)
      .join(" "),
  };
  return {
    approvalHash: value.approval_hash,
    estimate,
    actions,
    localWork: value.local_work,
    warnings,
  };
}

function redactBackendPaths(
  value: string,
  source: string,
  destination: string,
): string {
  return value.replaceAll(destination, "revision").replaceAll(source, "source");
}

function nonnegative(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : undefined;
}

async function materialize(
  args: string[],
  input: PlanInput,
  directory: string,
): Promise<string[]> {
  const result = [...args];
  const outIndex = result.indexOf("--out");
  if (outIndex < 0 || outIndex + 1 >= result.length)
    throw new Error("Invalid approved arguments");
  result[outIndex + 1] = directory;
  for (const flag of ["--script", "--brief"]) {
    const index = result.indexOf(flag);
    if (index >= 0) {
      const file = path.join(directory, `${flag.slice(2)}.txt`);
      await writeFile(file, input.prompt, { mode: 0o600 });
      result[index + 1] = file;
    }
  }
  return result;
}

async function childEnvironment(
  operatorEnv: NodeJS.ProcessEnv,
  credentialFile: string,
): Promise<NodeJS.ProcessEnv> {
  const result: NodeJS.ProcessEnv = {};
  for (const [name, value] of Object.entries(operatorEnv).sort(
    ([left], [right]) => left.localeCompare(right),
  )) {
    if (
      value !== undefined &&
      (CHILD_ENV_NAMES.has(name) || name.startsWith("REELMAESTRO_"))
    )
      result[name] = value;
  }
  delete result.REELMAESTRO_PORT;
  delete result.REELMAESTRO_STATE_DIR;
  delete result.REELMAESTRO_BINARY;
  delete result.REELMAESTRO_OUT_DIR;
  delete result.REELMAESTRO_NO_NARRATION;
  delete result.REELMAESTRO_NO_CAPTIONS;
  const key = await providerKey(operatorEnv, credentialFile);
  if (key) result.OPENROUTER_API_KEY = key;
  else delete result.OPENROUTER_API_KEY;
  return result;
}

async function providerKey(
  operatorEnv: NodeJS.ProcessEnv,
  credentialFile: string,
): Promise<string | undefined> {
  try {
    const parsed = parseJsonObject(await readFile(credentialFile, "utf8"));
    if (parsed && typeof parsed.apiKey === "string" && parsed.apiKey.trim())
      return parsed.apiKey;
  } catch {
    /* environment may provide the key */
  }
  const value = operatorEnv.OPENROUTER_API_KEY;
  return value?.trim() ? value : undefined;
}

async function configurationFingerprint(
  binary: string,
  env: NodeJS.ProcessEnv,
  args: string[] = [],
): Promise<string> {
  const hash = createHash("sha256");
  await new Promise<void>((resolve, reject) => {
    const stream = createReadStream(binary);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.once("error", reject);
    stream.once("end", resolve);
  });
  hash.update("\0environment\0");
  for (const [name, value] of Object.entries(env).sort(([left], [right]) =>
    left.localeCompare(right),
  ))
    hash.update(`${name}\0${value ?? ""}\0`);
  hash.update("\0argument-files\0");
  for (const file of [
    ...new Set(args.filter((value) => path.isAbsolute(value))),
  ].sort()) {
    if (!(await regularFile(file))) continue;
    hash.update(`${file}\0`);
    await new Promise<void>((resolve, reject) => {
      const stream = createReadStream(file);
      stream.on("data", (chunk) => hash.update(chunk));
      stream.once("error", reject);
      stream.once("end", resolve);
    });
    hash.update("\0");
  }
  return hash.digest("hex");
}

async function findVideo(
  directory: string,
): Promise<{ path: string; runDirectory: string } | undefined> {
  const entries = await readdir(directory, { withFileTypes: true }).catch(
    () => [],
  );
  for (const preferred of ["reel-video.mp4", "youtube.mp4", "reel.mp4"]) {
    const entry = entries.find((candidate) => candidate.name === preferred);
    if (
      entry?.isFile() &&
      !entry.isSymbolicLink() &&
      (await regularFile(path.join(directory, preferred)))
    )
      return { path: path.join(directory, preferred), runDirectory: directory };
  }
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
    const found = await findVideo(path.join(directory, entry.name));
    if (found) return found;
  }
  return undefined;
}

async function findDiagnostic(directory: string): Promise<string | undefined> {
  const entries = await readdir(directory, { withFileTypes: true }).catch(
    () => [],
  );
  if (
    entries.some(
      (entry) =>
        entry.isFile() &&
        !entry.isSymbolicLink() &&
        entry.name === "script.json",
    ) &&
    entries.some(
      (entry) =>
        entry.isFile() && !entry.isSymbolicLink() && entry.name === "audio.mp3",
    ) &&
    entries.some(
      (entry) =>
        entry.isFile() &&
        !entry.isSymbolicLink() &&
        entry.name === "words.json",
    )
  ) {
    try {
      const words: unknown = JSON.parse(
        await readFile(path.join(directory, "words.json"), "utf8"),
      );
      const audio = await lstat(path.join(directory, "audio.mp3"));
      if (
        Array.isArray(words) &&
        words.length > 0 &&
        audio.isFile() &&
        !audio.isSymbolicLink() &&
        audio.size > 0
      )
        return directory;
    } catch {
      /* continue searching */
    }
  }
  for (const entry of entries) {
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
    const found = await findDiagnostic(path.join(directory, entry.name));
    if (found) return found;
  }
  return undefined;
}

function parseNativeEvent(source: string):
  | {
      timestamp: string;
      stage: string;
      state: string;
      sceneId?: string;
      message?: string;
      code?: string;
    }
  | undefined {
  const event = parseJsonObject(source);
  if (
    !event ||
    event.version !== 1 ||
    typeof event.timestamp_unix_ms !== "number" ||
    !Number.isSafeInteger(event.timestamp_unix_ms) ||
    event.timestamp_unix_ms < 0 ||
    event.timestamp_unix_ms > 8_640_000_000_000_000 ||
    typeof event.stage !== "string" ||
    !oneOf(event.state, [
      "started",
      "progress",
      "completed",
      "warning",
      "failed",
    ])
  )
    return undefined;
  return {
    timestamp: new Date(event.timestamp_unix_ms).toISOString(),
    stage: event.stage.slice(0, 100),
    state: event.state,
    ...(typeof event.scene_id === "string"
      ? { sceneId: event.scene_id.slice(0, 128) }
      : {}),
    ...(typeof event.message === "string"
      ? { message: event.message.slice(0, 1000) }
      : {}),
    ...(typeof event.code === "string"
      ? { code: event.code.slice(0, 100) }
      : {}),
  };
}

async function validVideo(
  ffprobe: string,
  video: string,
  timeoutMs: number,
  env?: NodeJS.ProcessEnv,
): Promise<boolean> {
  const result = await runCommand(
    ffprobe,
    [
      "-v",
      "error",
      "-show_entries",
      "stream=codec_type:format=duration",
      "-of",
      "json",
      video,
    ],
    env ? { env } : {},
    timeoutMs,
  );
  if (result.code !== 0 || result.timedOut || result.outputExceeded)
    return false;
  const parsed = parseJsonObject(result.stdout);
  if (
    !parsed ||
    !Array.isArray(parsed.streams) ||
    !parsed.streams.some(
      (stream) => objectField(stream, "codec_type") === "video",
    )
  )
    return false;
  const format = plainObject(parsed.format);
  if (!format) return false;
  const duration =
    typeof format.duration === "string"
      ? Number(format.duration)
      : format.duration;
  return (
    typeof duration === "number" && Number.isFinite(duration) && duration > 0
  );
}

function parseEstimate(source: string): CostEstimate | undefined {
  const value = parseJsonObject(source);
  if (!value || value.version !== 1 || typeof value.warning !== "string")
    return undefined;
  const names = [
    "total_usd",
    "script_usd",
    "narration_usd",
    "images_usd",
    "video_usd",
    "music_usd",
  ];
  for (const name of names)
    if (
      typeof value[name] !== "number" ||
      !Number.isFinite(value[name]) ||
      value[name] < 0
    )
      return undefined;
  const total = value.total_usd;
  const script = value.script_usd;
  const narration = value.narration_usd;
  const images = value.images_usd;
  const video = value.video_usd;
  const music = value.music_usd;
  if (
    typeof total !== "number" ||
    typeof script !== "number" ||
    typeof narration !== "number" ||
    typeof images !== "number" ||
    typeof video !== "number" ||
    typeof music !== "number"
  )
    return undefined;
  return {
    version: 1,
    total_usd: total,
    script_usd: script,
    narration_usd: narration,
    images_usd: images,
    video_usd: video,
    music_usd: music,
    warning: value.warning,
  };
}

function parseRange(
  header: string | undefined,
  size: number,
): { start: number; end: number } | "invalid" | undefined {
  if (!header) return undefined;
  if (!Number.isSafeInteger(size) || size <= 0) return "invalid";
  const match = /^bytes=(\d*)-(\d*)$/.exec(header);
  if (!match || (!match[1] && !match[2])) return "invalid";
  if (!match[1]) {
    const suffix = Number(match[2]);
    if (!Number.isSafeInteger(suffix) || suffix <= 0) return "invalid";
    return { start: Math.max(0, size - suffix), end: size - 1 };
  }
  const start = Number(match[1]);
  const requestedEnd = match[2] ? Number(match[2]) : size - 1;
  if (
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(requestedEnd) ||
    start < 0 ||
    requestedEnd < start ||
    start >= size
  )
    return "invalid";
  return { start, end: Math.min(requestedEnd, size - 1) };
}

function approvalBody(
  value: unknown,
): { planId: string; planHash: string; idempotencyKey: string } | undefined {
  const body = plainObject(value);
  if (
    !body ||
    typeof body.planId !== "string" ||
    typeof body.planHash !== "string" ||
    typeof body.idempotencyKey !== "string" ||
    !/^[A-Za-z0-9_-]{8,128}$/.test(body.idempotencyKey)
  )
    return undefined;
  return {
    planId: body.planId,
    planHash: body.planHash,
    idempotencyKey: body.idempotencyKey,
  };
}
function apiKeyBody(value: unknown): string | null | undefined {
  const body = plainObject(value);
  if (!body || !("apiKey" in body)) return undefined;
  if (body.apiKey === null) return null;
  if (
    typeof body.apiKey !== "string" ||
    !body.apiKey.trim() ||
    body.apiKey.length > 4096
  )
    return undefined;
  return body.apiKey;
}
function plainObject(value: unknown): Record<string, unknown> | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return undefined;
  return Object.fromEntries(Object.entries(value));
}
function parseJsonObject(source: string): Record<string, unknown> | undefined {
  try {
    return plainObject(JSON.parse(source));
  } catch {
    return undefined;
  }
}
function objectField(value: unknown, name: string): unknown {
  return plainObject(value)?.[name];
}
function oneOf<const T extends string>(
  value: unknown,
  choices: readonly T[],
): value is T {
  return (
    typeof value === "string" && choices.some((choice) => choice === value)
  );
}
function finiteRange(
  value: unknown,
  minimum: number,
  maximum: number,
): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value >= minimum &&
    value <= maximum
  );
}
function exactHttpOrigin(origin: string, host: string): boolean {
  return origin === `http://${host}`;
}
function sessionCookie(cookie: string | undefined): string | undefined {
  return /(?:^|;\s*)rm_session=([A-Za-z0-9_-]{32})/.exec(cookie ?? "")?.[1];
}
function pruneSessions(sessions: Map<string, Session>): void {
  const now = Date.now();
  for (const [id, session] of sessions)
    if (session.expires <= now) sessions.delete(id);
}
function lastEventId(value: string | string[] | undefined): number | undefined {
  if (value === undefined) return 0;
  if (Array.isArray(value) || !/^(?:0|[1-9]\d*)$/.test(value)) return undefined;
  const result = Number(value);
  return Number.isSafeInteger(result) ? result : undefined;
}
function encodeEvent(event: JobEvent): string {
  return `id: ${event.id}\nevent: job\ndata: ${JSON.stringify(event)}\n\n`;
}
async function regularFile(file: string): Promise<boolean> {
  const info = await lstat(file).catch(() => undefined);
  return Boolean(info?.isFile() && !info.isSymbolicLink());
}
async function directoryExists(directory: string): Promise<boolean> {
  const info = await lstat(directory).catch(() => undefined);
  return Boolean(info?.isDirectory() && !info.isSymbolicLink());
}
async function writableDirectory(directory: string): Promise<boolean> {
  const probe = path.join(directory, `.ready-${randomUUID()}`);
  try {
    await writeFile(probe, "", { flag: "wx", mode: 0o600 });
    await rm(probe);
    return true;
  } catch {
    await rm(probe, { force: true }).catch(() => undefined);
    return false;
  }
}
async function executableAvailable(
  executable: string,
  args: string[],
  cwd: string,
  env: NodeJS.ProcessEnv,
): Promise<boolean> {
  if (path.isAbsolute(executable)) {
    try {
      await access(executable, constants.X_OK);
    } catch {
      return false;
    }
  }
  const result = await runCommand(executable, args, { cwd, env }, 5_000);
  return result.code === 0 && !result.timedOut && !result.outputExceeded;
}
function parsePort(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const port = Number(value);
  return Number.isSafeInteger(port) && port > 0 && port <= 65535
    ? port
    : undefined;
}
