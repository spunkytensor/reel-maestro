import { createHash } from "node:crypto";
import { lstat, readdir, readFile, realpath, stat } from "node:fs/promises";
import path from "node:path";
import { createReadStream } from "node:fs";
import type {
  Artifact,
  Chapter,
  RunDetail,
  RunSummary,
  Scene,
} from "../shared.js";

const MAX_JSON = 2 * 1024 * 1024;
const MAX_SCAN_DEPTH = 6;
const MAX_SCAN_DIRECTORIES = 5_000;
const MAX_DIRECTORY_ENTRIES = 10_000;
const SAFE_FILE =
  /^(?:poster\.jpg|reel(?:-video)?\.mp4|youtube\.mp4|audio\.mp3|mix\.m4a|music\.(?:wav|mp3)|captions\.(?:ass|srt|vtt)|reel\.ass|words\.json|(?:metadata|youtube)\.md|scene-\d{2,}\.(?:jpg|png|mp4)|segment-\d{2,}\.(?:ass|mp4)|chapter-\d{2,}\.mp3)$/;
const FINAL_VIDEOS = ["reel-video.mp4", "youtube.mp4", "reel.mp4"];
export const MIME: Readonly<Record<string, string>> = {
  ".ass": "text/x-ssa; charset=utf-8",
  ".jpg": "image/jpeg",
  ".png": "image/png",
  ".mp4": "video/mp4",
  ".mp3": "audio/mpeg",
  ".m4a": "audio/mp4",
  ".wav": "audio/wav",
  ".srt": "application/x-subrip",
  ".vtt": "text/vtt",
  ".md": "text/markdown; charset=utf-8",
  ".json": "application/json; charset=utf-8",
};

type JsonObject = { readonly [key: string]: unknown };

function object(value: unknown): JsonObject | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return undefined;
  return Object.fromEntries(Object.entries(value));
}
function idFor(value: string): string {
  return createHash("sha256").update(value).digest("base64url").slice(0, 24);
}
async function jsonFile(file: string): Promise<JsonObject> {
  const info = await lstat(file);
  if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_JSON)
    throw new Error("invalid script");
  const parsed: unknown = JSON.parse(await readFile(file, "utf8"));
  const result = object(parsed);
  if (!result) throw new Error("invalid script");
  return result;
}
async function isRegularRootedFile(
  directory: string,
  name: string,
): Promise<boolean> {
  if (path.basename(name) !== name) return false;
  const candidate = path.join(directory, name);
  const info = await lstat(candidate).catch(() => undefined);
  if (!info?.isFile() || info.isSymbolicLink()) return false;
  const [resolvedDirectory, resolvedCandidate] = await Promise.all([
    realpath(directory),
    realpath(candidate),
  ]);
  return resolvedCandidate.startsWith(`${resolvedDirectory}${path.sep}`);
}

export class Library {
  constructor(
    readonly root: string,
    private readonly validateVideo: (file: string) => Promise<boolean>,
    private readonly inspectRun?: (directory: string) => Promise<unknown>,
  ) {}

  private async dirs(): Promise<string[]> {
    const root = await realpath(this.root).catch(() => path.resolve(this.root));
    const found: string[] = [];
    let visited = 0;
    const scan = async (
      base: string,
      depth: number,
      allowStudioJobs: boolean,
    ): Promise<void> => {
      if (visited >= MAX_SCAN_DIRECTORIES) return;
      visited += 1;
      const entries = (
        await readdir(base, { withFileTypes: true }).catch(() => [])
      ).slice(0, MAX_DIRECTORY_ENTRIES);
      for (const entry of entries) {
        if (visited >= MAX_SCAN_DIRECTORIES) return;
        if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
        if (
          entry.name.startsWith(".") &&
          !(allowStudioJobs && entry.name === ".studio-jobs")
        )
          continue;
        const candidate = path.join(base, entry.name);
        const resolved = await realpath(candidate).catch(() => "");
        if (!resolved || !isWithin(root, resolved)) continue;
        if (await isRegularRootedFile(resolved, "script.json"))
          found.push(resolved);
        if (depth > 0) await scan(resolved, depth - 1, false);
      }
    };
    await scan(root, MAX_SCAN_DEPTH, true);
    return found;
  }

  async locate(id: string): Promise<string | undefined> {
    if (!/^[A-Za-z0-9_-]{24}$/.test(id)) return undefined;
    return (await this.dirs()).find((directory) => idFor(directory) === id);
  }

  async runIdForDirectory(directory: string): Promise<string | undefined> {
    const resolvedRoot = await realpath(this.root).catch(() =>
      path.resolve(this.root),
    );
    const resolved = await realpath(directory).catch(() => "");
    if (
      !resolved ||
      !isWithin(resolvedRoot, resolved) ||
      !(await isRegularRootedFile(resolved, "script.json"))
    )
      return undefined;
    return idFor(resolved);
  }

  async runIdForRevision(
    revisionId: string,
    sourceRunId?: string,
  ): Promise<string | undefined> {
    if (!/^[A-Za-z0-9_-]{1,96}$/.test(revisionId)) return undefined;
    const source = sourceRunId ? await this.locate(sourceRunId) : undefined;
    const familyRoot = source ? familyRootFor(source) : undefined;
    const directory = (await this.dirs()).find(
      (candidate) =>
        path.basename(candidate) === revisionId &&
        (!familyRoot || familyRootFor(candidate) === familyRoot),
    );
    return directory ? idFor(directory) : undefined;
  }

  async summaries(): Promise<RunSummary[]> {
    const rows = await Promise.all(
      (await this.dirs()).map((directory) => this.detailAt(directory, false)),
    );
    return rows.sort((left, right) =>
      right.updatedAt.localeCompare(left.updatedAt),
    );
  }

  async detail(id: string): Promise<RunDetail | undefined> {
    const directory = await this.locate(id);
    if (!directory) return undefined;
    let detail = await this.detailAt(directory, true);
    const familyRoot = familyRootFor(directory);
    const inRevisions = familyRoot !== directory;
    const family = await this.familySummaries(familyRoot);
    if (inRevisions) {
      detail = {
        ...detail,
        parentRunId: idFor(familyRoot),
        revisions: family,
      };
    } else {
      detail = {
        ...detail,
        revisions: family,
      };
    }
    if (!this.inspectRun) return detail;
    const inspection = object(await this.inspectRun(directory));
    const script = object(inspection?.script);
    const sourceFingerprint = inspection?.source_fingerprint;
    if (
      inspection?.version !== 1 ||
      !script ||
      typeof sourceFingerprint !== "string" ||
      !/^sha256:[a-f0-9]{64}$/.test(sourceFingerprint)
    )
      return detail;
    const inspectedScenes = Array.isArray(script.scenes) ? script.scenes : [];
    if (inspectedScenes.length !== detail.scenes.length) return detail;
    return {
      ...detail,
      ...(typeof script.title === "string" && script.title.trim()
        ? { title: script.title }
        : {}),
      format: script.format === "youtube" ? "youtube" : "reel",
      sourceFingerprint,
      scenes: detail.scenes.map((scene, index) => {
        const inspected = object(inspectedScenes[index]);
        if (
          typeof inspected?.id !== "string" ||
          !/^[A-Za-z0-9_-]{1,128}$/.test(inspected.id)
        )
          return scene;
        return {
          ...scene,
          id: inspected.id,
          line: text(inspected.line),
          imagePrompt: text(inspected.image_prompt),
          motionPrompt: text(inspected.motion_prompt),
          castIds: stringArray(inspected.cast_ids),
          locationId: text(inspected.location_id),
          transition: inspected.transition === "dissolve" ? "dissolve" : "cut",
        };
      }),
      narration: text(script.narration),
      musicPrompt: text(script.music_prompt),
      chapters: parseChapters(script.chapters),
      characters: parseEntities(script.characters),
      locations: parseEntities(script.locations),
      posterPrompt: text(script.poster_prompt),
    };
  }

  private async familySummaries(familyRoot: string): Promise<RunSummary[]> {
    const directories = (await this.dirs()).filter(
      (candidate) => familyRootFor(candidate) === familyRoot,
    );
    const summaries = await Promise.all(
      directories.map((candidate) => this.detailAt(candidate, false)),
    );
    return summaries.sort((left, right) =>
      right.updatedAt.localeCompare(left.updatedAt),
    );
  }

  async source(
    id: string,
  ): Promise<
    { directory: string; script: JsonObject; fingerprint: string } | undefined
  > {
    const directory = await this.locate(id);
    if (!directory) return undefined;
    const inspected = object(await this.inspectRun?.(directory));
    const script = object(inspected?.script);
    const fingerprint = inspected?.source_fingerprint;
    return script &&
      typeof fingerprint === "string" &&
      /^sha256:[a-f0-9]{64}$/.test(fingerprint)
      ? { directory, script, fingerprint }
      : undefined;
  }

  private async detailAt(directory: string, full: boolean): Promise<RunDetail> {
    const id = idFor(directory);
    const modified = (await stat(directory)).mtime.toISOString();
    try {
      const script = await jsonFile(path.join(directory, "script.json"));
      const sourceScenes = Array.isArray(script.scenes)
        ? script.scenes.slice(0, 500)
        : [];
      const scenes: Scene[] = await Promise.all(
        sourceScenes.map(async (value, index) => {
          const scene = object(value);
          const number = String(index).padStart(2, "0");
          const imageName = `scene-${number}.jpg`;
          const videoName = `scene-${number}.mp4`;
          return {
            id:
              typeof scene?.id === "string" &&
              /^[A-Za-z0-9_-]{1,128}$/.test(scene.id)
                ? scene.id
                : `legacy-${index}`,
            index,
            line: text(scene?.line),
            imagePrompt: text(scene?.image_prompt),
            motionPrompt: text(scene?.motion_prompt),
            castIds: stringArray(scene?.cast_ids),
            locationId: text(scene?.location_id),
            transition: scene?.transition === "dissolve" ? "dissolve" : "cut",
            ...((await isRegularRootedFile(directory, imageName))
              ? { imageUrl: artifactUrl(id, imageName) }
              : {}),
            ...((await isRegularRootedFile(directory, videoName))
              ? { videoUrl: artifactUrl(id, videoName) }
              : {}),
          };
        }),
      );
      const names = await readdir(directory);
      const artifacts: Artifact[] = [];
      for (const name of names) {
        if (
          SAFE_FILE.test(name) &&
          (await isRegularRootedFile(directory, name))
        ) {
          artifacts.push({
            name,
            url: artifactUrl(id, name),
            kind: kind(name),
          });
        }
      }
      const finalArtifacts = FINAL_VIDEOS.map((name) =>
        artifacts.find((artifact) => artifact.name === name),
      ).filter((artifact) => artifact !== undefined);
      let video: Artifact | undefined;
      for (const artifact of finalArtifacts) {
        if (await this.validateVideo(path.join(directory, artifact.name))) {
          video = artifact;
          break;
        }
      }
      const poster = artifacts.find(
        (artifact) => artifact.name === "poster.jpg",
      );
      const title =
        typeof script.title === "string" && script.title.trim()
          ? script.title
          : path.basename(directory);
      const chapters: Chapter[] = full ? parseChapters(script.chapters) : [];
      const characters = full ? parseEntities(script.characters) : [];
      const locations = full ? parseEntities(script.locations) : [];
      const sourceFingerprint = full
        ? await directoryFingerprint(directory, names)
        : "";
      return {
        id,
        title,
        format: script.format === "youtube" ? "youtube" : "reel",
        status: video
          ? "complete"
          : finalArtifacts.length
            ? "damaged"
            : "incomplete",
        updatedAt: modified,
        sceneCount: scenes.length,
        ...(poster ? { posterUrl: poster.url } : {}),
        ...(video ? { videoUrl: video.url } : {}),
        scenes: full ? scenes : [],
        narration: full ? text(script.narration) : "",
        musicPrompt: full ? text(script.music_prompt) : "",
        artifacts: full ? artifacts : [],
        chapters,
        characters,
        locations,
        posterPrompt: full ? text(script.poster_prompt) : "",
        sourceFingerprint,
        ...(!video && finalArtifacts.length
          ? { error: "The final video could not be validated" }
          : {}),
      };
    } catch {
      return {
        id,
        title: path.basename(directory),
        format: "reel",
        status: "damaged",
        updatedAt: modified,
        sceneCount: 0,
        scenes: [],
        narration: "",
        musicPrompt: "",
        artifacts: [],
        chapters: [],
        characters: [],
        locations: [],
        posterPrompt: "",
        sourceFingerprint: "",
        error: "This project could not be read safely",
      };
    }
  }

  async artifact(
    id: string,
    name: string,
  ): Promise<
    | { path: string; stat: Awaited<ReturnType<typeof stat>>; mime: string }
    | undefined
  > {
    if (!SAFE_FILE.test(name) || path.basename(name) !== name) return undefined;
    const directory = await this.locate(id);
    if (!directory || !(await isRegularRootedFile(directory, name)))
      return undefined;
    const candidate = await realpath(path.join(directory, name));
    const mime = MIME[path.extname(name)];
    if (!mime) return undefined;
    return { path: candidate, stat: await stat(candidate), mime };
  }
}

function isWithin(root: string, candidate: string): boolean {
  return candidate === root || candidate.startsWith(`${root}${path.sep}`);
}
function familyRootFor(directory: string): string {
  return path.basename(path.dirname(directory)) === "revisions"
    ? path.dirname(path.dirname(directory))
    : directory;
}
function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}
function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value
        .filter((item): item is string => typeof item === "string")
        .slice(0, 100)
    : [];
}
function parseChapters(value: unknown): Chapter[] {
  if (!Array.isArray(value)) return [];
  const chapters: Chapter[] = [];
  for (const item of value.slice(0, 500)) {
    const chapter = object(item);
    if (
      !chapter ||
      typeof chapter.title !== "string" ||
      typeof chapter.narration !== "string" ||
      typeof chapter.scene_start !== "number" ||
      !Number.isSafeInteger(chapter.scene_start) ||
      chapter.scene_start < 0 ||
      typeof chapter.scene_count !== "number" ||
      !Number.isSafeInteger(chapter.scene_count) ||
      chapter.scene_count < 1
    )
      continue;
    chapters.push({
      title: chapter.title,
      summary: text(chapter.summary),
      narration: chapter.narration,
      sceneStart: chapter.scene_start,
      sceneCount: chapter.scene_count,
    });
  }
  return chapters;
}
function parseEntities(
  value: unknown,
): Array<{ id: string; description: string }> {
  if (!Array.isArray(value)) return [];
  const entities: Array<{ id: string; description: string }> = [];
  for (const item of value.slice(0, 500)) {
    const entity = object(item);
    if (
      entity &&
      typeof entity.id === "string" &&
      /^[A-Za-z0-9_-]{1,128}$/.test(entity.id) &&
      typeof entity.description === "string"
    )
      entities.push({ id: entity.id, description: entity.description });
  }
  return entities;
}
async function directoryFingerprint(
  directory: string,
  names: string[],
): Promise<string> {
  const hash = createHash("sha256");
  const relevant = names
    .filter((name) => !name.startsWith(".") && name !== "revision-plan.json")
    .sort();
  for (const name of relevant) {
    if (!(await isRegularRootedFile(directory, name))) continue;
    hash.update(`${name}\0`);
    await new Promise<void>((resolve, reject) => {
      const stream = createReadStream(path.join(directory, name));
      stream.on("data", (chunk) => hash.update(chunk));
      stream.once("error", reject);
      stream.once("end", resolve);
    });
    hash.update("\0");
  }
  return `sha256:${hash.digest("hex")}`;
}
function artifactUrl(id: string, name: string): string {
  return `/api/runs/${id}/artifacts/${name}`;
}
function kind(name: string): Artifact["kind"] {
  const extension = path.extname(name);
  if (extension === ".mp4") return "video";
  if (extension === ".jpg" || extension === ".png") return "image";
  if (extension === ".mp3" || extension === ".m4a" || extension === ".wav")
    return "audio";
  if (extension === ".srt" || extension === ".vtt" || extension === ".ass")
    return "captions";
  return "document";
}
