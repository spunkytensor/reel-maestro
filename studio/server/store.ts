import { DatabaseSync } from "node:sqlite";
import {
  closeSync,
  mkdirSync,
  openSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import type { Job, JobEvent, JobStatus, PlanInput } from "../shared.js";

type SqlRow = Record<string, unknown>;
export type StoredPlan = {
  id: string;
  hash: string;
  expires: number;
  input: PlanInput;
  args: string[];
  fingerprint: string;
  estimatedTotal: number;
};
export type StoredJob = {
  id: string;
  planId: string;
  status: JobStatus;
  args: string[];
  input: PlanInput;
  fingerprint: string;
  estimatedTotal: number;
  revisionPlanPath?: string;
  revisionId?: string;
  sourceRunId?: string;
  recovery: boolean;
};
export type StoredRevisionPlan = {
  id: string;
  hash: string;
  expires: number;
  requestPath: string;
  planPath: string;
  sourceId: string;
  fingerprint: string;
  estimatedTotal: number;
  localWork: boolean;
  revisionId: string;
  maxCost: number;
  jobId: string;
};

function row(value: unknown): SqlRow | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : undefined;
}
function stringField(value: unknown, name: string): string {
  if (typeof value !== "string") throw new Error(`Invalid database ${name}`);
  return value;
}
function numberField(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isFinite(value))
    throw new Error(`Invalid database ${name}`);
  return value;
}
function statusField(value: unknown): JobStatus {
  if (typeof value !== "string" || !isJobStatus(value))
    throw new Error("Invalid database job status");
  return value;
}

function isJobStatus(value: string): value is JobStatus {
  return (
    value === "queued" ||
    value === "running" ||
    value === "succeeded" ||
    value === "failed" ||
    value === "interrupted" ||
    value === "cancelled" ||
    value === "cancelling"
  );
}
function stringArrayJson(value: unknown): string[] {
  const parsed: unknown = JSON.parse(stringField(value, "args"));
  if (!Array.isArray(parsed) || parsed.some((item) => typeof item !== "string"))
    throw new Error("Invalid database args");
  return parsed;
}

export type StateLock = { release(): void };
export function acquireStateLock(stateDir: string): StateLock {
  mkdirSync(stateDir, { recursive: true, mode: 0o700 });
  const lockPath = path.join(stateDir, "server.lock");
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      const fd = openSync(lockPath, "wx", 0o600);
      writeFileSync(fd, JSON.stringify({ pid: process.pid }));
      closeSync(fd);
      let released = false;
      return {
        release() {
          if (!released) {
            released = true;
            try {
              unlinkSync(lockPath);
            } catch {
              /* already removed */
            }
          }
        },
      };
    } catch (error) {
      const code = errorCode(error);
      if (code !== "EEXIST") throw error;
      let pid: number;
      try {
        const parsed: unknown = JSON.parse(readFileSync(lockPath, "utf8"));
        if (
          parsed !== null &&
          typeof parsed === "object" &&
          !Array.isArray(parsed) &&
          "pid" in parsed &&
          typeof parsed.pid === "number" &&
          Number.isSafeInteger(parsed.pid) &&
          parsed.pid > 0
        )
          pid = parsed.pid;
        else throw new Error("invalid lock owner");
      } catch {
        throw new Error(
          "Studio state lock is malformed; remove it manually if no Studio server is running",
        );
      }
      try {
        process.kill(pid, 0);
        throw new Error("Studio state directory is already in use");
      } catch (ownerError) {
        if (errorCode(ownerError) !== "ESRCH")
          throw new Error("Studio state directory is already in use");
      }
      if (attempt === 1)
        throw new Error("Studio state directory is already in use");
      try {
        unlinkSync(lockPath);
      } catch {
        /* retry reports contention */
      }
    }
  }
  throw new Error("Studio state directory is already in use");
}

function errorCode(error: unknown): unknown {
  return error !== null && typeof error === "object" && "code" in error
    ? error.code
    : undefined;
}

export class Store {
  readonly db: DatabaseSync;
  constructor(stateDir: string) {
    mkdirSync(stateDir, { recursive: true, mode: 0o700 });
    this.db = new DatabaseSync(path.join(stateDir, "studio.sqlite"));
    this.db
      .exec(`PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS plans(id TEXT PRIMARY KEY, hash TEXT, expires INTEGER, input TEXT, args TEXT, fingerprint TEXT, estimated_total REAL NOT NULL);
      CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, plan_id TEXT UNIQUE, idem TEXT UNIQUE, status TEXT, created TEXT, updated TEXT, args TEXT, input TEXT, fingerprint TEXT, estimated_total REAL NOT NULL, run_id TEXT, error TEXT);
      CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY AUTOINCREMENT, job_id TEXT, status TEXT, at TEXT, message TEXT);
      CREATE TABLE IF NOT EXISTS revision_plans(id TEXT PRIMARY KEY, hash TEXT NOT NULL, expires INTEGER NOT NULL, request_path TEXT NOT NULL, plan_path TEXT NOT NULL, source_id TEXT NOT NULL, fingerprint TEXT NOT NULL, estimated_total REAL NOT NULL, local_work INTEGER NOT NULL, revision_id TEXT NOT NULL, max_cost REAL NOT NULL, job_id TEXT NOT NULL);`);
    this.ensureColumn("plans", "estimated_total", "REAL NOT NULL DEFAULT 0");
    this.ensureColumn("jobs", "estimated_total", "REAL NOT NULL DEFAULT 0");
    this.ensureColumn("jobs", "revision_plan", "TEXT");
    this.ensureColumn("jobs", "revision_id", "TEXT");
    this.ensureColumn("jobs", "source_run_id", "TEXT");
    this.ensureColumn("jobs", "recovery", "INTEGER NOT NULL DEFAULT 0");
    this.ensureColumn("jobs", "recovery_allowed", "INTEGER NOT NULL DEFAULT 0");
    this.ensureColumn("jobs", "operator_action", "INTEGER NOT NULL DEFAULT 0");
    this.ensureColumn("events", "stage", "TEXT");
    this.ensureColumn("events", "scene_id", "TEXT");
    this.ensureColumn("events", "progress", "REAL");
    this.ensureColumn("events", "kind", "TEXT");
    this.ensureColumn("revision_plans", "max_cost", "REAL NOT NULL DEFAULT 0");
    this.ensureColumn("revision_plans", "job_id", "TEXT NOT NULL DEFAULT ''");
    const now = new Date().toISOString();
    const running = this.db
      .prepare(
        "SELECT id,revision_plan FROM jobs WHERE status IN ('running','cancelling')",
      )
      .all();
    for (const value of running) {
      const parsed = row(value);
      if (parsed) {
        const id = stringField(parsed.id, "job id");
        if (typeof parsed.revision_plan === "string")
          this.setRevisionInterruption(
            id,
            true,
            false,
            "Server restarted; revision recovery requires an explicit request",
          );
        else
          this.setStatus(
            id,
            "interrupted",
            "Server restarted; paid work was not relaunched",
            now,
          );
      }
    }
  }
  addPlan(p: {
    id: string;
    hash: string;
    expires: number;
    input: PlanInput;
    args: string[];
    fingerprint: string;
    estimatedTotal: number;
  }) {
    this.db
      .prepare("INSERT INTO plans VALUES(?,?,?,?,?,?,?)")
      .run(
        p.id,
        p.hash,
        p.expires,
        JSON.stringify(p.input),
        JSON.stringify(p.args),
        p.fingerprint,
        p.estimatedTotal,
      );
  }
  plan(id: string): StoredPlan | undefined {
    const parsed = row(
      this.db.prepare("SELECT * FROM plans WHERE id=?").get(id),
    );
    if (!parsed) return undefined;
    return {
      id: stringField(parsed.id, "plan id"),
      hash: stringField(parsed.hash, "plan hash"),
      expires: numberField(parsed.expires, "plan expiry"),
      input: parsePlanInput(parsed.input),
      args: stringArrayJson(parsed.args),
      fingerprint: stringField(parsed.fingerprint, "fingerprint"),
      estimatedTotal: numberField(parsed.estimated_total, "estimate"),
    };
  }
  addRevisionPlan(plan: StoredRevisionPlan): void {
    this.db
      .prepare("INSERT INTO revision_plans VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
      .run(
        plan.id,
        plan.hash,
        plan.expires,
        plan.requestPath,
        plan.planPath,
        plan.sourceId,
        plan.fingerprint,
        plan.estimatedTotal,
        plan.localWork ? 1 : 0,
        plan.revisionId,
        plan.maxCost,
        plan.jobId,
      );
  }
  revisionPlan(id: string): StoredRevisionPlan | undefined {
    const parsed = row(
      this.db.prepare("SELECT * FROM revision_plans WHERE id=?").get(id),
    );
    if (!parsed) return undefined;
    return {
      id: stringField(parsed.id, "revision plan id"),
      hash: stringField(parsed.hash, "revision plan hash"),
      expires: numberField(parsed.expires, "revision plan expiry"),
      requestPath: stringField(parsed.request_path, "revision request"),
      planPath: stringField(parsed.plan_path, "revision plan"),
      sourceId: stringField(parsed.source_id, "revision source"),
      fingerprint: stringField(parsed.fingerprint, "fingerprint"),
      estimatedTotal: numberField(parsed.estimated_total, "estimate"),
      localWork: numberField(parsed.local_work, "local work") === 1,
      revisionId: stringField(parsed.revision_id, "revision id"),
      maxCost: numberField(parsed.max_cost, "maximum cost"),
      jobId: stringField(parsed.job_id, "revision job id"),
    };
  }
  createJob(j: {
    id: string;
    planId: string;
    idem: string;
    args: string[];
    input: PlanInput;
    fingerprint: string;
    estimatedTotal: number;
  }) {
    const now = new Date().toISOString();
    this.db
      .prepare(
        "INSERT INTO jobs(id,plan_id,idem,status,created,updated,args,input,fingerprint,estimated_total) VALUES(?,?,?,?,?,?,?,?,?,?)",
      )
      .run(
        j.id,
        j.planId,
        j.idem,
        "queued",
        now,
        now,
        JSON.stringify(j.args),
        JSON.stringify(j.input),
        j.fingerprint,
        j.estimatedTotal,
      );
    this.event(j.id, "queued", "Approved and queued");
    const created = this.job(j.id);
    if (!created) throw new Error("Created job could not be read");
    return created;
  }
  createRevisionJob(j: {
    id: string;
    planId: string;
    idem: string;
    plan: StoredRevisionPlan;
    maxCost: number;
  }) {
    const input: PlanInput = {
      prompt: "revision",
      source: "topic",
      format: "reel",
      quality: "standard",
      narration: true,
      music: false,
      video: "off",
      captions: true,
      captionStyle: "burst",
      maxCost: j.maxCost,
    };
    const now = new Date().toISOString();
    this.db
      .prepare(
        "INSERT INTO jobs(id,plan_id,idem,status,created,updated,args,input,fingerprint,estimated_total,revision_plan,revision_id,source_run_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
      )
      .run(
        j.id,
        j.planId,
        j.idem,
        "queued",
        now,
        now,
        "[]",
        JSON.stringify(input),
        j.plan.fingerprint,
        j.plan.estimatedTotal,
        j.plan.planPath,
        j.plan.revisionId,
        j.plan.sourceId,
      );
    this.event(j.id, "queued", "Approved revision queued");
    const created = this.job(j.id);
    if (!created) throw new Error("Created revision job could not be read");
    return created;
  }
  requeueRevision(id: string): void {
    const job = this.rawJob(id);
    const persisted = row(
      this.db.prepare("SELECT recovery_allowed FROM jobs WHERE id=?").get(id),
    );
    if (
      !job?.revisionPlanPath ||
      job.status !== "interrupted" ||
      persisted?.recovery_allowed !== 1
    )
      throw new Error("Revision is not recoverable");
    const now = new Date().toISOString();
    this.db
      .prepare(
        "UPDATE jobs SET status='queued',updated=?,recovery=1,error=NULL WHERE id=?",
      )
      .run(now, id);
    this.event(id, "queued", "Explicit revision recovery queued", now);
  }
  setRevisionInterruption(
    id: string,
    recoverable: boolean,
    operatorAction: boolean,
    message: string,
  ): void {
    const now = new Date().toISOString();
    this.db
      .prepare(
        "UPDATE jobs SET status='interrupted',updated=?,error=?,recovery_allowed=?,operator_action=? WHERE id=?",
      )
      .run(now, message, recoverable ? 1 : 0, operatorAction ? 1 : 0, id);
    this.event(id, "interrupted", message, now);
  }
  byIdem(id: string): { job: Job; planId: string } | undefined {
    return this.jobLookup("idem", id);
  }
  byPlan(id: string): { job: Job; idem: string } | undefined {
    const parsed = row(
      this.db.prepare("SELECT * FROM jobs WHERE plan_id=?").get(id),
    );
    return parsed
      ? {
          job: this.toJob(parsed),
          idem: stringField(parsed.idem, "idempotency key"),
        }
      : undefined;
  }
  rawJob(id: string): StoredJob | undefined {
    const parsed = row(
      this.db.prepare("SELECT * FROM jobs WHERE id=?").get(id),
    );
    return parsed ? this.toStoredJob(parsed) : undefined;
  }
  job(id: string): Job | undefined {
    const parsed = row(
      this.db.prepare("SELECT * FROM jobs WHERE id=?").get(id),
    );
    return parsed ? this.toJob(parsed) : undefined;
  }
  jobs(): Job[] {
    return this.db
      .prepare("SELECT * FROM jobs ORDER BY created DESC")
      .all()
      .map((value) => this.toJobRequired(value));
  }
  next(): StoredJob | undefined {
    const parsed = row(
      this.db
        .prepare(
          "SELECT * FROM jobs WHERE status='queued' ORDER BY created LIMIT 1",
        )
        .get(),
    );
    return parsed ? this.toStoredJob(parsed) : undefined;
  }
  setStatus(
    id: string,
    status: JobStatus,
    message: string,
    at = new Date().toISOString(),
    extra: { runId?: string; error?: string } = {},
  ) {
    this.db
      .prepare(
        "UPDATE jobs SET status=?,updated=?,run_id=COALESCE(?,run_id),error=? WHERE id=?",
      )
      .run(status, at, extra.runId ?? null, extra.error ?? null, id);
    this.event(id, status, message, at);
  }
  event(
    id: string,
    status: JobStatus,
    message: string,
    at = new Date().toISOString(),
    details: {
      stage?: string;
      sceneId?: string;
      progress?: number;
      kind?: JobEvent["kind"];
    } = {},
  ) {
    this.db
      .prepare(
        "INSERT INTO events(job_id,status,at,message,stage,scene_id,progress,kind) VALUES(?,?,?,?,?,?,?,?)",
      )
      .run(
        id,
        status,
        at,
        message,
        details.stage ?? null,
        details.sceneId ?? null,
        details.progress ?? null,
        details.kind ?? null,
      );
    this.db
      .prepare(
        "DELETE FROM events WHERE job_id=? AND id NOT IN (SELECT id FROM events WHERE job_id=? ORDER BY id DESC LIMIT 1000)",
      )
      .run(id, id);
  }
  events(id: string, after = 0, limit = 1000): JobEvent[] {
    return this.db
      .prepare(
        "SELECT * FROM events WHERE job_id=? AND id>? ORDER BY id LIMIT ?",
      )
      .all(id, after, limit)
      .map((value) => {
        const parsed = row(value);
        if (!parsed) throw new Error("Invalid database event");
        return {
          id: numberField(parsed.id, "event id"),
          jobId: stringField(parsed.job_id, "event job id"),
          status: statusField(parsed.status),
          at: stringField(parsed.at, "event time"),
          message: stringField(parsed.message, "event message"),
          ...(typeof parsed.stage === "string" ? { stage: parsed.stage } : {}),
          ...(typeof parsed.scene_id === "string"
            ? { sceneId: parsed.scene_id }
            : {}),
          ...(typeof parsed.progress === "number" &&
          Number.isFinite(parsed.progress)
            ? { progress: parsed.progress }
            : {}),
          ...(parsed.kind === "progress" || parsed.kind === "warning"
            ? { kind: parsed.kind }
            : {}),
        };
      });
  }
  latestEvent(id: string): JobEvent | undefined {
    const parsed = row(
      this.db
        .prepare("SELECT * FROM events WHERE job_id=? ORDER BY id DESC LIMIT 1")
        .get(id),
    );
    return parsed
      ? {
          id: numberField(parsed.id, "event id"),
          jobId: stringField(parsed.job_id, "event job id"),
          status: statusField(parsed.status),
          at: stringField(parsed.at, "event time"),
          message: stringField(parsed.message, "event message"),
          ...(typeof parsed.stage === "string" ? { stage: parsed.stage } : {}),
          ...(typeof parsed.scene_id === "string"
            ? { sceneId: parsed.scene_id }
            : {}),
          ...(typeof parsed.progress === "number" &&
          Number.isFinite(parsed.progress)
            ? { progress: parsed.progress }
            : {}),
          ...(parsed.kind === "progress" || parsed.kind === "warning"
            ? { kind: parsed.kind }
            : {}),
        }
      : undefined;
  }
  private jobLookup(
    column: "idem",
    value: string,
  ): { job: Job; planId: string } | undefined {
    const parsed = row(
      this.db.prepare(`SELECT * FROM jobs WHERE ${column}=?`).get(value),
    );
    return parsed
      ? {
          job: this.toJob(parsed),
          planId: stringField(parsed.plan_id, "plan id"),
        }
      : undefined;
  }
  private toStoredJob(parsed: SqlRow): StoredJob {
    return {
      id: stringField(parsed.id, "job id"),
      planId: stringField(parsed.plan_id, "plan id"),
      status: statusField(parsed.status),
      args: stringArrayJson(parsed.args),
      input: parsePlanInput(parsed.input),
      fingerprint: stringField(parsed.fingerprint, "fingerprint"),
      estimatedTotal: numberField(parsed.estimated_total, "estimate"),
      ...(typeof parsed.revision_plan === "string"
        ? { revisionPlanPath: parsed.revision_plan }
        : {}),
      ...(typeof parsed.revision_id === "string"
        ? { revisionId: parsed.revision_id }
        : {}),
      ...(typeof parsed.source_run_id === "string"
        ? { sourceRunId: parsed.source_run_id }
        : {}),
      recovery: parsed.recovery === 1,
    };
  }
  private toJobRequired(value: unknown): Job {
    const parsed = row(value);
    if (!parsed) throw new Error("Invalid database job");
    return this.toJob(parsed);
  }
  private toJob(parsed: SqlRow): Job {
    const runId = parsed.run_id;
    const sourceRunId = parsed.source_run_id;
    const error = parsed.error;
    const latest = this.events(stringField(parsed.id, "job id")).findLast(
      (event) => event.stage !== undefined || event.progress !== undefined,
    );
    const warnings = this.events(stringField(parsed.id, "job id"))
      .filter((event) => event.kind === "warning")
      .map((event) => event.message);
    return {
      id: stringField(parsed.id, "job id"),
      status: statusField(parsed.status),
      createdAt: stringField(parsed.created, "created time"),
      updatedAt: stringField(parsed.updated, "updated time"),
      ...(typeof runId === "string" && runId ? { runId } : {}),
      ...(typeof sourceRunId === "string" && sourceRunId
        ? { sourceRunId }
        : {}),
      ...(latest?.stage ? { stage: latest.stage } : {}),
      ...(latest?.progress !== undefined ? { progress: latest.progress } : {}),
      ...(warnings.length ? { warnings } : {}),
      ...(parsed.status === "interrupted" && parsed.recovery_allowed === 1
        ? { recoverable: true }
        : {}),
      ...(parsed.operator_action === 1 ? { requiresOperatorAction: true } : {}),
      ...(typeof error === "string" && error ? { error } : {}),
    };
  }
  private ensureColumn(
    table: "plans" | "jobs" | "revision_plans" | "events",
    column: string,
    definition: string,
  ): void {
    const columns = this.db.prepare(`PRAGMA table_info(${table})`).all();
    if (!columns.some((value) => row(value)?.name === column))
      this.db.exec(`ALTER TABLE ${table} ADD COLUMN ${column} ${definition}`);
  }
}

function parsePlanInput(value: unknown): PlanInput {
  const parsed: unknown = JSON.parse(stringField(value, "plan input"));
  const candidate = row(parsed);
  if (
    !candidate ||
    typeof candidate.prompt !== "string" ||
    !isSource(candidate.source) ||
    !isFormat(candidate.format) ||
    !isQuality(candidate.quality) ||
    typeof candidate.narration !== "boolean" ||
    typeof candidate.music !== "boolean" ||
    !isVideo(candidate.video) ||
    typeof candidate.captions !== "boolean" ||
    !isCaptionStyle(candidate.captionStyle) ||
    typeof candidate.maxCost !== "number" ||
    !Number.isFinite(candidate.maxCost)
  )
    throw new Error("Invalid database plan input");
  const minutes = optionalFiniteNumber(candidate.minutes, "minutes");
  const speed = optionalFiniteNumber(candidate.speed, "speed");
  const voice = optionalString(candidate.voice, "voice");
  return {
    prompt: candidate.prompt,
    source: candidate.source,
    format: candidate.format,
    quality: candidate.quality,
    narration: candidate.narration,
    music: candidate.music,
    video: candidate.video,
    captions: candidate.captions,
    captionStyle: candidate.captionStyle,
    maxCost: candidate.maxCost,
    ...(minutes === undefined ? {} : { minutes }),
    ...(speed === undefined ? {} : { speed }),
    ...(voice === undefined ? {} : { voice }),
  };
}

function isSource(value: unknown): value is PlanInput["source"] {
  return (
    value === "topic" ||
    value === "script" ||
    value === "brief" ||
    value === "url"
  );
}
function isFormat(value: unknown): value is PlanInput["format"] {
  return value === "reel" || value === "youtube";
}
function isQuality(value: unknown): value is PlanInput["quality"] {
  return value === "draft" || value === "standard" || value === "premium";
}
function isVideo(value: unknown): value is PlanInput["video"] {
  return value === "off" || value === "all";
}
function isCaptionStyle(value: unknown): value is PlanInput["captionStyle"] {
  return (
    value === "burst" ||
    value === "karaoke" ||
    value === "boxed" ||
    value === "minimal"
  );
}
function optionalFiniteNumber(
  value: unknown,
  name: string,
): number | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "number" || !Number.isFinite(value))
    throw new Error(`Invalid database ${name}`);
  return value;
}
function optionalString(value: unknown, name: string): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string") throw new Error(`Invalid database ${name}`);
  return value;
}
