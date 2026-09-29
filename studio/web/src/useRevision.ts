import { useEffect, useRef, useState } from "react";
import type {
  Chapter,
  Job,
  RevisionInput,
  RevisionPlan,
  RevisionScene,
  RunDetail,
} from "../../shared";
import { validateRevision } from "../../validation";
import { api, approvalKey, errorMessage } from "./api";
import { readDefaults } from "./preferences";

export function initialRevision(run: RunDetail): RevisionInput {
  return {
    scenes: run.scenes.map(
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
    chapters: run.chapters,
    characters: run.characters,
    locations: run.locations,
    posterPrompt: run.posterPrompt,
    narration: run.narration,
    musicPrompt: run.musicPrompt,
    musicAction: "keep",
    videoSceneIds: run.scenes
      .filter((scene) => scene.videoUrl)
      .map((scene) => scene.id),
    maxCost: readDefaults().maxCost,
    expectedSourceFingerprint: run.sourceFingerprint,
  };
}

export function useRevision(run: RunDetail, onJob: (job: Job) => void) {
  const initial = useRef(initialRevision(run)).current;
  const storageKey = `reelmaestro.draft.${run.id}`;
  const [restored] = useState(() => {
    try {
      const source = localStorage.getItem(storageKey);
      if (!source) return { draft: initial, error: "" };
      const parsed = validateRevision(JSON.parse(source));
      return typeof parsed === "string"
        ? {
            draft: initial,
            error:
              "A saved draft could not be restored. Its stored copy has not been replaced.",
          }
        : { draft: parsed, error: "" };
    } catch {
      return {
        draft: initial,
        error:
          "Draft storage is unavailable. Keep this tab open until your changes are applied.",
      };
    }
  });
  const [history, setHistory] = useState<{
    past: RevisionInput[];
    present: RevisionInput;
    future: RevisionInput[];
  }>({ past: [], present: restored.draft, future: [] });
  const [error, setError] = useState(restored.error);
  const [plan, setPlan] = useState<RevisionPlan>();
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  const key = useRef("");
  const inFlight = useRef(false);
  const draft = history.present;
  const stale = draft.expectedSourceFingerprint !== run.sourceFingerprint;
  const changedScenes = draft.scenes
    .filter((scene, index) => {
      const original = initial.scenes.find(
        (candidate) => candidate.id === scene.id,
      );
      return (
        !original ||
        JSON.stringify(scene) !== JSON.stringify(original) ||
        initial.scenes[index]?.id !== scene.id
      );
    })
    .map((scene) => scene.id);
  const removed = initial.scenes.filter(
    (scene) => !draft.scenes.some((candidate) => candidate.id === scene.id),
  ).length;
  const originalFields = new Map<string, unknown>(Object.entries(initial));
  const otherChanges = Object.entries(draft).filter(
    ([name, value]) =>
      ![
        "scenes",
        "narration",
        "expectedSourceFingerprint",
        "legacyReuse",
        "maxCost",
      ].includes(name) &&
      JSON.stringify(value) !== JSON.stringify(originalFields.get(name)),
  ).length;
  const changeCount = changedScenes.length + removed + otherChanges;

  useEffect(() => {
    if (!plan) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [plan]);
  const expired = !!plan && now >= Date.parse(plan.expiresAt);
  const overBudget = !!plan && plan.estimate.total_usd > draft.maxCost;

  function persist(next: RevisionInput) {
    try {
      localStorage.setItem(storageKey, JSON.stringify(next));
    } catch {
      setError(
        "Draft storage is full or unavailable. Keep this tab open until your changes are applied.",
      );
    }
  }
  function change(next: RevisionInput) {
    if (inFlight.current || plan) return;
    setHistory((previous) => ({
      past: [...previous.past, previous.present].slice(-100),
      present: next,
      future: [],
    }));
    setError("");
    persist(next);
  }
  function update<K extends keyof RevisionInput>(
    name: K,
    value: RevisionInput[K],
  ) {
    change({ ...draft, [name]: value });
  }
  function undo() {
    if (inFlight.current || plan || !history.past.length) return;
    const previous = history.past[history.past.length - 1];
    setHistory({
      past: history.past.slice(0, -1),
      present: previous,
      future: [draft, ...history.future],
    });
    persist(previous);
  }
  function redo() {
    if (inFlight.current || plan || !history.future.length) return;
    const next = history.future[0];
    setHistory({
      past: [...history.past, draft],
      present: next,
      future: history.future.slice(1),
    });
    persist(next);
  }
  function reset() {
    if (inFlight.current) return;
    setPlan(undefined);
    setHistory({ past: [], present: initial, future: [] });
    setError("");
    persist(initial);
  }
  function editScenes(scenes: RevisionScene[]) {
    const speechChanged =
      scenes.length !== draft.scenes.length ||
      scenes.some(
        (scene, index) =>
          scene.id !== draft.scenes[index]?.id ||
          scene.line !== draft.scenes[index]?.line,
      );
    if (!speechChanged) return update("scenes", scenes);
    const chapters: Chapter[] = [];
    let previousChapter: Chapter | undefined;
    for (const [index, scene] of scenes.entries()) {
      const oldIndex = draft.scenes.findIndex(
        (candidate) => candidate.id === scene.id,
      );
      const chapter = draft.chapters?.find(
        (candidate) =>
          oldIndex >= candidate.sceneStart &&
          oldIndex < candidate.sceneStart + candidate.sceneCount,
      );
      if (!chapter) continue;
      if (chapter !== previousChapter)
        chapters.push({
          ...chapter,
          sceneStart: index,
          sceneCount: 0,
          narration: "",
        });
      const current = chapters[chapters.length - 1];
      current.sceneCount++;
      current.narration = [current.narration, scene.line]
        .filter(Boolean)
        .join(" ");
      previousChapter = chapter;
    }
    change({
      ...draft,
      scenes,
      chapters,
      narration: scenes
        .map((scene) => scene.line)
        .filter(Boolean)
        .join(" "),
      videoSceneIds: draft.videoSceneIds.filter((id) =>
        scenes.some((scene) => scene.id === id),
      ),
    });
  }
  async function review(request: RevisionInput = draft) {
    if (inFlight.current || stale || !draft.scenes.length) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      const next = await api<RevisionPlan>(
        `/runs/${encodeURIComponent(run.id)}/plans`,
        request,
      );
      key.current = approvalKey();
      setPlan(next);
      setNow(Date.now());
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  async function apply() {
    if (inFlight.current || !plan || expired || overBudget || stale) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      const job = await api<Job>("/jobs", {
        planId: plan.planId,
        planHash: plan.planHash,
        idempotencyKey: key.current,
      });
      setPlan(undefined);
      onJob(job);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  return {
    draft,
    update,
    change,
    editScenes,
    undo,
    redo,
    reset,
    canUndo: !!history.past.length,
    canRedo: !!history.future.length,
    changedScenes,
    changeCount,
    stale,
    busy,
    error,
    plan,
    expired,
    overBudget,
    review,
    apply,
    closeReview: () => {
      if (!inFlight.current) setPlan(undefined);
    },
  };
}
