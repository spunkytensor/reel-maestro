export type RunSummary = {
  id: string;
  title: string;
  format: "reel" | "youtube";
  status: "complete" | "incomplete" | "damaged";
  posterUrl?: string;
  videoUrl?: string;
  updatedAt: string;
  sceneCount: number;
};

export type Scene = {
  id: string;
  index: number;
  line: string;
  imagePrompt: string;
  motionPrompt: string;
  castIds: string[];
  locationId: string;
  transition: "cut" | "dissolve";
  imageUrl?: string;
  videoUrl?: string;
};

export type Chapter = {
  title: string;
  summary: string;
  narration: string;
  sceneStart: number;
  sceneCount: number;
};

export type Entity = { id: string; description: string };

export type Artifact = {
  name: string;
  url: string;
  kind: "video" | "image" | "audio" | "captions" | "document";
};
export type RunDetail = RunSummary & {
  scenes: Scene[];
  narration: string;
  musicPrompt: string;
  artifacts: Artifact[];
  chapters: Chapter[];
  characters: Entity[];
  locations: Entity[];
  posterPrompt: string;
  parentRunId?: string;
  revisions?: RunSummary[];
  sourceFingerprint: string;
  error?: string;
};

export type PlanInput = {
  prompt: string;
  source: "topic" | "script" | "brief" | "url";
  sourceAssetId?: string;
  format: "reel" | "youtube";
  quality: "draft" | "standard" | "premium";
  minutes?: number;
  narration: boolean;
  music: boolean;
  video: "off" | "all";
  captions: boolean;
  captionStyle: "burst" | "karaoke" | "boxed" | "minimal";
  maxCost: number;
  speed?: number;
  voice?: string;
  advanced?: AdvancedOptions;
};

export type AdvancedOptions = {
  textModel?: string;
  imageModel?: string;
  judgeModel?: string;
  ttsModel?: string;
  musicModel?: string;
  captionFont?: string;
  whisperModel?: string;
  sceneSeconds?: number;
  validateScene?: number;
  consistency?: boolean;
  characterReferenceAssetId?: string;
  videoProvider?: "openrouter" | "local";
  videoModel?: string;
  videoResolution?: "720p" | "1080p";
  videoSize?: string;
  videoInputMode?: "text" | "first-frame";
  videoSeed?: number;
  videoSteps?: number;
  videoWaitTimeout?: number;
  videoScenes?: number;
  mix?: boolean;
  musicVolume?: number;
  musicAssetId?: string;
  dissolve?: boolean;
  dissolveSeconds?: number;
  grade?: boolean;
  loudnorm?: boolean;
  posterScene?: number;
  embedPoster?: boolean;
  watermarkAssetId?: string;
  noImages?: boolean;
  verbose?: boolean;
  whisperExecutable?: "whisper_timestamped";
  exportPreset?: "web" | "social" | "pro";
};

export type RevisionScene = {
  id: string;
  line: string;
  imagePrompt: string;
  motionPrompt: string;
  castIds?: string[];
  locationId?: string;
  transition?: "cut" | "dissolve";
  imageAction?: "keep" | "regenerate";
  motionAction?: "keep" | "regenerate";
};

export type RevisionInput = {
  scenes: RevisionScene[];
  chapters?: Chapter[];
  characters?: Entity[];
  locations?: Entity[];
  posterPrompt?: string;
  narration?: string;
  narrationEnabled?: boolean;
  musicPrompt?: string;
  advanced?: AdvancedOptions;
  musicAction: "keep" | "remove" | "regenerate";
  videoSceneIds: string[];
  maxCost: number;
  captions?: boolean;
  captionStyle?: "burst" | "karaoke" | "boxed" | "minimal";
  voice?: string;
  speed?: number;
  quality?: "draft" | "standard" | "premium";
  exportPreset?: "web" | "social" | "pro";
  legacyReuse?: "trust";
  format?: "reel" | "youtube";
  expectedSourceFingerprint: string;
};

export type RevisionAction = {
  artifact: string;
  action: "reuse" | "generate" | "derive" | "exclude";
  reason: string;
  source?: string;
  destination?: string;
};

export type CostEstimate = {
  [key: string]: unknown;
  version: 1;
  total_usd: number;
  script_usd: number;
  narration_usd: number;
  images_usd: number;
  video_usd: number;
  music_usd: number;
  warning: string;
};
export type Plan = {
  planId: string;
  planHash: string;
  expiresAt: string;
  estimate: CostEstimate;
};
export type RevisionPlan = Plan & {
  revisionId: string;
  parentRunId: string;
  actions: RevisionAction[];
  localWork: boolean;
  warnings: string[];
};
export type JobStatus =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "interrupted"
  | "cancelled"
  | "cancelling";
export type Job = {
  id: string;
  status: JobStatus;
  createdAt: string;
  updatedAt: string;
  runId?: string;
  sourceRunId?: string;
  error?: string;
  stage?: string;
  progress?: number;
  warnings?: string[];
  recoverable?: boolean;
  requiresOperatorAction?: boolean;
};
export type JobEvent = {
  id: number;
  jobId: string;
  status: JobStatus;
  at: string;
  message: string;
  stage?: string;
  sceneId?: string;
  progress?: number;
  kind?: "progress" | "warning";
};
export type Settings = {
  providerConnected: boolean;
  outputLocation: string;
  generationAvailable: boolean;
};
export type Capabilities = {
  available: boolean;
  schema?: unknown;
  error?: string;
};

export type UploadAsset = {
  id: string;
  name: string;
  kind: "text" | "image" | "audio";
  size: number;
  mime: string;
  text?: string;
};

export type UrlSource = { text: string; title?: string; url: string };

export type Readiness = {
  ready: boolean;
  components: {
    output: boolean;
    state: boolean;
    sqlite: boolean;
    reelmaestro: boolean;
    ffmpeg: boolean;
    ffprobe: boolean;
    whisper: boolean;
  };
  providerConfigured: boolean;
};
