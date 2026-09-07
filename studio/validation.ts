import type {
  AdvancedOptions,
  Chapter,
  RevisionInput,
  RevisionScene,
} from "./shared.js";

type JsonObject = Record<string, unknown>;

export function validateRevision(value: unknown): RevisionInput | string {
  const input = object(value);
  if (!input) return "Invalid revision";
  const allowed = new Set([
    "scenes",
    "chapters",
    "characters",
    "locations",
    "posterPrompt",
    "narration",
    "narrationEnabled",
    "musicPrompt",
    "advanced",
    "musicAction",
    "videoSceneIds",
    "maxCost",
    "captions",
    "captionStyle",
    "voice",
    "speed",
    "quality",
    "exportPreset",
    "legacyReuse",
    "format",
    "expectedSourceFingerprint",
  ]);
  if (Object.keys(input).some((key) => !allowed.has(key)))
    return "Unknown revision field";
  if (
    !Array.isArray(input.scenes) ||
    input.scenes.length < 1 ||
    input.scenes.length > 500
  )
    return "scenes must contain 1-500 entries";
  const scenes: RevisionScene[] = [];
  const ids = new Set<string>();
  for (const value of input.scenes) {
    const result = revisionScene(value);
    if (typeof result === "string") return result;
    if (ids.has(result.id)) return "Scene ids must be unique";
    ids.add(result.id);
    scenes.push(result);
  }
  if (!Array.isArray(input.videoSceneIds) || input.videoSceneIds.length > 500)
    return "videoSceneIds must be an array";
  const videoSceneIds: string[] = [];
  for (const id of input.videoSceneIds) {
    if (typeof id !== "string" || !ids.has(id) || videoSceneIds.includes(id))
      return "videoSceneIds must contain unique scene ids";
    videoSceneIds.push(id);
  }
  if (!oneOf(input.musicAction, ["keep", "remove", "regenerate"]))
    return "Invalid musicAction";
  if (!finite(input.maxCost, 0, 1000))
    return "maxCost must be between 0 and 1000";
  if (
    typeof input.expectedSourceFingerprint !== "string" ||
    !/^sha256:[a-f0-9]{64}$/.test(input.expectedSourceFingerprint)
  )
    return "Invalid expectedSourceFingerprint";
  const advanced =
    input.advanced === undefined ? undefined : validateAdvanced(input.advanced);
  if (typeof advanced === "string") return advanced;
  if (input.exportPreset !== undefined && advanced?.exportPreset !== undefined)
    return "exportPreset must be specified once";
  const chapters =
    input.chapters === undefined
      ? undefined
      : parseChapters(input.chapters, scenes.length);
  if (typeof chapters === "string") return chapters;
  const characters =
    input.characters === undefined
      ? undefined
      : parseEntities(input.characters);
  if (typeof characters === "string") return characters;
  const locations =
    input.locations === undefined ? undefined : parseEntities(input.locations);
  if (typeof locations === "string") return locations;
  if (
    input.posterPrompt !== undefined &&
    !boundedString(input.posterPrompt, 20_000)
  )
    return "Invalid posterPrompt";
  if (input.narration !== undefined && !boundedString(input.narration, 200_000))
    return "Invalid narration";
  if (
    input.musicPrompt !== undefined &&
    !boundedString(input.musicPrompt, 20_000)
  )
    return "Invalid musicPrompt";
  if (
    input.voice !== undefined &&
    (!boundedString(input.voice, 100) || !/^[\w .-]+$/.test(input.voice))
  )
    return "Invalid voice";
  if (
    input.narrationEnabled !== undefined &&
    typeof input.narrationEnabled !== "boolean"
  )
    return "Invalid narrationEnabled";
  if (input.captions !== undefined && typeof input.captions !== "boolean")
    return "Invalid captions";
  if (
    input.captionStyle !== undefined &&
    !oneOf(input.captionStyle, ["burst", "karaoke", "boxed", "minimal"])
  )
    return "Invalid captionStyle";
  if (input.speed !== undefined && !finite(input.speed, 0.5, 2))
    return "Invalid speed";
  if (
    input.quality !== undefined &&
    !oneOf(input.quality, ["draft", "standard", "premium"])
  )
    return "Invalid quality";
  if (
    input.exportPreset !== undefined &&
    !oneOf(input.exportPreset, ["web", "social", "pro"])
  )
    return "Invalid exportPreset";
  if (input.legacyReuse !== undefined && input.legacyReuse !== "trust")
    return "Invalid legacyReuse";
  if (input.format !== undefined && !oneOf(input.format, ["reel", "youtube"]))
    return "Invalid format";
  return {
    scenes,
    ...(chapters ? { chapters } : {}),
    ...(characters ? { characters } : {}),
    ...(locations ? { locations } : {}),
    ...(typeof input.posterPrompt === "string"
      ? { posterPrompt: input.posterPrompt }
      : {}),
    ...(typeof input.narration === "string"
      ? { narration: input.narration }
      : {}),
    ...(typeof input.narrationEnabled === "boolean"
      ? { narrationEnabled: input.narrationEnabled }
      : {}),
    ...(typeof input.musicPrompt === "string"
      ? { musicPrompt: input.musicPrompt }
      : {}),
    ...(advanced ? { advanced } : {}),
    musicAction: input.musicAction,
    videoSceneIds,
    maxCost: input.maxCost,
    ...(typeof input.captions === "boolean"
      ? { captions: input.captions }
      : {}),
    ...(typeof input.captionStyle === "string"
      ? { captionStyle: input.captionStyle }
      : {}),
    ...(typeof input.voice === "string" ? { voice: input.voice } : {}),
    ...(typeof input.speed === "number" ? { speed: input.speed } : {}),
    ...(typeof input.quality === "string" ? { quality: input.quality } : {}),
    ...(typeof input.exportPreset === "string"
      ? { exportPreset: input.exportPreset }
      : {}),
    ...(input.legacyReuse === "trust" ? { legacyReuse: "trust" as const } : {}),
    ...(typeof input.format === "string" ? { format: input.format } : {}),
    expectedSourceFingerprint: input.expectedSourceFingerprint,
  };
}

export function validateAdvanced(value: unknown): AdvancedOptions | string {
  const input = object(value);
  if (!input) return "Invalid advanced options";
  const allowed = new Set([
    "textModel",
    "imageModel",
    "judgeModel",
    "ttsModel",
    "musicModel",
    "captionFont",
    "whisperModel",
    "sceneSeconds",
    "validateScene",
    "consistency",
    "characterReferenceAssetId",
    "videoProvider",
    "videoModel",
    "videoResolution",
    "videoSize",
    "videoInputMode",
    "videoSeed",
    "videoSteps",
    "videoWaitTimeout",
    "videoScenes",
    "mix",
    "musicVolume",
    "musicAssetId",
    "dissolve",
    "dissolveSeconds",
    "grade",
    "loudnorm",
    "posterScene",
    "embedPoster",
    "watermarkAssetId",
    "noImages",
    "verbose",
    "whisperExecutable",
    "exportPreset",
  ]);
  if (Object.keys(input).some((key) => !allowed.has(key)))
    return "Unknown advanced option";
  const result: AdvancedOptions = {};
  for (const name of [
    "textModel",
    "imageModel",
    "judgeModel",
    "ttsModel",
    "musicModel",
    "captionFont",
    "whisperModel",
    "videoModel",
    "videoSize",
  ] as const) {
    const value = input[name];
    if (value !== undefined) {
      if (!boundedString(value, 200) || /[\u0000\r\n]/.test(value))
        return `Invalid ${name}`;
      result[name] = value;
    }
  }
  for (const name of [
    "characterReferenceAssetId",
    "musicAssetId",
    "watermarkAssetId",
  ] as const) {
    const value = input[name];
    if (value !== undefined) {
      if (
        typeof value !== "string" ||
        !/^[A-Za-z0-9_-]{20,200}\.[A-Za-z0-9_-]{20,100}$/.test(value)
      )
        return `Invalid ${name}`;
      result[name] = value;
    }
  }
  for (const name of [
    "consistency",
    "mix",
    "dissolve",
    "grade",
    "loudnorm",
    "embedPoster",
    "noImages",
    "verbose",
  ] as const) {
    const value = input[name];
    if (value !== undefined) {
      if (typeof value !== "boolean") return `Invalid ${name}`;
      result[name] = value;
    }
  }
  const numeric: ReadonlyArray<
    [keyof AdvancedOptions, number, number, boolean]
  > = [
    ["sceneSeconds", 0.1, 120, false],
    ["validateScene", 0, 500, true],
    ["videoSeed", 0, Number.MAX_SAFE_INTEGER, true],
    ["videoSteps", 1, 1000, true],
    ["videoWaitTimeout", 1, 1440, true],
    ["videoScenes", 0, 500, true],
    ["musicVolume", 0, 2, false],
    ["dissolveSeconds", 0, 10, false],
    ["posterScene", 0, 500, true],
  ];
  for (const [name, minimum, maximum, integer] of numeric) {
    const value = input[name];
    if (value !== undefined) {
      if (
        !finite(value, minimum, maximum) ||
        (integer && !Number.isSafeInteger(value))
      )
        return `Invalid ${name}`;
      if (name === "validateScene" && value !== 0 && value !== 2 && value !== 3)
        return "validateScene must be 0, 2, or 3";
      setNumeric(result, name, value);
    }
  }
  if (
    input.videoProvider !== undefined &&
    !oneOf(input.videoProvider, ["openrouter", "local"])
  )
    return "Invalid videoProvider";
  if (
    input.videoResolution !== undefined &&
    !oneOf(input.videoResolution, ["720p", "1080p"])
  )
    return "Invalid videoResolution";
  if (
    input.videoInputMode !== undefined &&
    !oneOf(input.videoInputMode, ["text", "first-frame"])
  )
    return "Invalid videoInputMode";
  if (
    input.whisperExecutable !== undefined &&
    input.whisperExecutable !== "whisper_timestamped"
  )
    return "Invalid whisperExecutable";
  if (typeof input.videoProvider === "string")
    result.videoProvider = input.videoProvider;
  if (typeof input.videoResolution === "string")
    result.videoResolution = input.videoResolution;
  if (typeof input.videoInputMode === "string")
    result.videoInputMode = input.videoInputMode;
  if (input.whisperExecutable === "whisper_timestamped")
    result.whisperExecutable = input.whisperExecutable;
  if (
    input.exportPreset !== undefined &&
    !oneOf(input.exportPreset, ["web", "social", "pro"])
  )
    return "Invalid exportPreset";
  if (typeof input.exportPreset === "string")
    result.exportPreset = input.exportPreset;
  return result;
}

function revisionScene(value: unknown): RevisionScene | string {
  const scene = object(value);
  if (!scene) return "Invalid scene";
  const allowed = new Set([
    "id",
    "line",
    "imagePrompt",
    "motionPrompt",
    "castIds",
    "locationId",
    "transition",
    "imageAction",
    "motionAction",
  ]);
  if (Object.keys(scene).some((key) => !allowed.has(key)))
    return "Unknown scene field";
  if (typeof scene.id !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(scene.id))
    return "Invalid scene id";
  if (
    !boundedString(scene.line, 100_000) ||
    !boundedString(scene.imagePrompt, 20_000) ||
    !boundedString(scene.motionPrompt, 20_000)
  )
    return "Invalid scene text";
  if (
    scene.castIds !== undefined &&
    (!Array.isArray(scene.castIds) ||
      scene.castIds.some(
        (id) => typeof id !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(id),
      ))
  )
    return "Invalid scene castIds";
  if (scene.locationId !== undefined && !boundedString(scene.locationId, 128))
    return "Invalid scene locationId";
  if (
    scene.transition !== undefined &&
    !oneOf(scene.transition, ["cut", "dissolve"])
  )
    return "Invalid scene transition";
  if (
    scene.imageAction !== undefined &&
    !oneOf(scene.imageAction, ["keep", "regenerate"])
  )
    return "Invalid scene imageAction";
  if (
    scene.motionAction !== undefined &&
    !oneOf(scene.motionAction, ["keep", "regenerate"])
  )
    return "Invalid scene motionAction";
  return {
    id: scene.id,
    line: scene.line,
    imagePrompt: scene.imagePrompt,
    motionPrompt: scene.motionPrompt,
    ...(Array.isArray(scene.castIds)
      ? {
          castIds: scene.castIds.filter(
            (id): id is string => typeof id === "string",
          ),
        }
      : {}),
    ...(typeof scene.locationId === "string"
      ? { locationId: scene.locationId }
      : {}),
    ...(typeof scene.transition === "string"
      ? { transition: scene.transition }
      : {}),
    ...(typeof scene.imageAction === "string"
      ? { imageAction: scene.imageAction }
      : {}),
    ...(typeof scene.motionAction === "string"
      ? { motionAction: scene.motionAction }
      : {}),
  };
}

function parseChapters(
  value: unknown,
  sceneLength: number,
): Chapter[] | string {
  if (!Array.isArray(value) || value.length > 500) return "Invalid chapters";
  const result: Chapter[] = [];
  for (const item of value) {
    const chapter = object(item);
    if (
      !chapter ||
      !boundedString(chapter.title, 500) ||
      !boundedString(chapter.summary, 5000) ||
      !boundedString(chapter.narration, 200_000) ||
      !finite(chapter.sceneStart, 0, sceneLength) ||
      !Number.isSafeInteger(chapter.sceneStart) ||
      !finite(chapter.sceneCount, 1, sceneLength) ||
      !Number.isSafeInteger(chapter.sceneCount) ||
      chapter.sceneStart + chapter.sceneCount > sceneLength
    )
      return "Invalid chapter";
    result.push({
      title: chapter.title,
      summary: chapter.summary,
      narration: chapter.narration,
      sceneStart: chapter.sceneStart,
      sceneCount: chapter.sceneCount,
    });
  }
  return result;
}

function parseEntities(
  value: unknown,
): Array<{ id: string; description: string }> | string {
  if (!Array.isArray(value) || value.length > 500) return "Invalid entities";
  const result: Array<{ id: string; description: string }> = [];
  for (const item of value) {
    const entity = object(item);
    if (
      !entity ||
      Object.keys(entity).some(
        (key) => key !== "id" && key !== "description",
      ) ||
      typeof entity.id !== "string" ||
      !/^[A-Za-z0-9_-]{1,128}$/.test(entity.id) ||
      !boundedString(entity.description, 20_000) ||
      result.some((known) => known.id === entity.id)
    )
      return "Invalid entity";
    result.push({ id: entity.id, description: entity.description });
  }
  return result;
}

function object(value: unknown): JsonObject | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : undefined;
}
function boundedString(value: unknown, maximum: number): value is string {
  return (
    typeof value === "string" &&
    value.length <= maximum &&
    !value.includes("\u0000")
  );
}
function finite(
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
function oneOf<const T extends string>(
  value: unknown,
  choices: readonly T[],
): value is T {
  return (
    typeof value === "string" && choices.some((choice) => choice === value)
  );
}
function setNumeric(
  target: AdvancedOptions,
  name: keyof AdvancedOptions,
  value: number,
): void {
  Object.assign(target, { [name]: value });
}
