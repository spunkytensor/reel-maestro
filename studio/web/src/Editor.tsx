import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  Job,
  RevisionAction,
  RevisionScene,
  RunDetail,
} from "../../shared";
import {
  captionStyles,
  formats,
  Icon,
  Item,
  money,
  Notice,
  Overlay,
  qualities,
  Segmented,
  timecode,
  Toggle,
} from "./components";
import { errorMessage } from "./api";
import { AdvancedControls } from "./AdvancedControls";
import { UploadField } from "./UploadField";
import { Versions } from "./Versions";
import { EntityFields } from "./EntityFields";
import { useRevision } from "./useRevision";
import { inProgress } from "./Activity";

const revisionReasons: Record<string, string> = {
  dependencies_match: "Creation settings match",
  explicit_keep: "Keep the existing media as requested",
  explicit_regenerate: "Create new media as requested",
  explicit_remove: "Remove as requested",
  not_selected: "Use a still instead of animation",
  source_missing: "No existing media is available",
  edited_revision: "Save the edited script in a new version",
  spoken_inputs_changed: "Words or voice settings changed",
  narration_unchanged: "Narration is unchanged",
  narration_changed: "Update timing for the revised narration",
  image_inputs_changed: "Image description or creation settings changed",
  video_inputs_changed: "Motion or animation settings changed",
  poster_inputs_changed: "Cover description or image settings changed",
  entity_inputs_match: "Character or location settings match",
  entity_inputs_changed: "Character or location settings changed",
  primary_entity_matches: "The main character is unchanged",
  reference_input_changed: "Reference image settings changed",
  revision_render: "Assemble and encode the new video locally",
};
function revisionActionLabel(item: RevisionAction, scenes: RevisionScene[]) {
  const match = /^scene:([^:]+):(still|video)$/.exec(item.artifact);
  const index = match ? scenes.findIndex((scene) => scene.id === match[1]) : -1;
  const label =
    match && index >= 0
      ? `Scene ${index + 1} ${match[2] === "still" ? "image" : "animation"}`
      : item.artifact.startsWith("reference:")
        ? "Reference image"
        : item.artifact;
  return `${label.charAt(0).toUpperCase()}${label.slice(1)} — ${revisionReasons[item.reason] ?? item.reason.replaceAll("_", " ")}`;
}

type Panel = "scenes" | "narration" | "music";
export function Editor({
  run,
  jobs,
  onJob,
}: {
  run: RunDetail;
  jobs: Job[];
  onJob: (job: Job) => void;
}) {
  const [downloads, setDownloads] = useState(false);
  const [versions, setVersions] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [exportPreset, setExportPreset] = useState<"web" | "social" | "pro">(
    "web",
  );
  const [finishing, setFinishing] = useState(false);
  const [removeMode, setRemoveMode] = useState(false);
  const [uploading, setUploading] = useState(false);
  const pendingKey = `reelmaestro.pending.${run.id}`;
  const [jobId, setJobId] = useState<string | undefined>(() => {
    try {
      return localStorage.getItem(pendingKey) || undefined;
    } catch {
      return undefined;
    }
  });
  const revision = useRevision(run, (job) => {
    setJobId(job.id);
    try {
      localStorage.setItem(pendingKey, job.id);
    } catch {
      /* server activity still records the job */
    }
    setReviewOpen(false);
    setExportOpen(false);
    onJob(job);
  });
  const { draft } = revision;
  const submittedJob =
    jobs.find((job) => job.sourceRunId === run.id && inProgress(job)) ??
    jobs.find((job) => job.id === jobId);
  const applying = !!submittedJob && inProgress(submittedJob);
  const locked = revision.busy || !!revision.plan || applying || uploading;
  const actionsElement = document.getElementById("editor-actions");
  const [panel, setPanel] = useState<Panel>("scenes");
  const [selected, setSelected] = useState(0);
  const [scenePreview, setScenePreview] = useState(false);
  const [customize, setCustomize] = useState(false);
  const [mobileScenes, setMobileScenes] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(false);
  const [duration, setDuration] = useState(0);
  const [current, setCurrent] = useState(0);
  const [error, setError] = useState("");
  const video = useRef<HTMLVideoElement>(null);
  const sceneList = useRef<HTMLDivElement>(null);
  const scenes = draft.scenes.map((scene, index) => ({
    ...run.scenes.find((original) => original.id === scene.id),
    ...scene,
    index,
  }));
  const scene = scenes[Math.min(selected, scenes.length - 1)];
  const audio = run.artifacts.find((artifact) => artifact.name === "audio.mp3");
  const music = run.artifacts.find(
    (artifact) => artifact.kind === "audio" && artifact.name !== "audio.mp3",
  );

  function select(index: number) {
    setSelected(index);
    setScenePreview(true);
    video.current?.pause();
  }
  function editScene(changes: Partial<RevisionScene>) {
    if (!scene || locked) return;
    revision.editScenes(
      draft.scenes.map((candidate) =>
        candidate.id === scene.id ? { ...candidate, ...changes } : candidate,
      ),
    );
  }
  function moveScene(delta: number) {
    if (locked || !scene) return;
    const from = draft.scenes.findIndex(
      (candidate) => candidate.id === scene.id,
    );
    const to = from + delta;
    if (to < 0 || to >= draft.scenes.length) return;
    const next = [...draft.scenes];
    [next[from], next[to]] = [next[to], next[from]];
    revision.editScenes(next);
    setSelected(to);
  }
  function removeScene(keepSpeech: boolean) {
    if (!scene || scenes.length < 2 || locked) return;
    const next = draft.scenes.filter((candidate) => candidate.id !== scene.id);
    if (keepSpeech) {
      const target = Math.min(selected, next.length - 1);
      next[target] = {
        ...next[target],
        line:
          selected < next.length
            ? `${scene.line} ${next[target].line}`.trim()
            : `${next[target].line} ${scene.line}`.trim(),
      };
    }
    revision.editScenes(next);
    setSelected(Math.min(selected, next.length - 1));
    setRemoveMode(false);
  }
  async function play() {
    const element = video.current;
    if (!element) return;
    setScenePreview(false);
    if (element.paused) {
      try {
        await element.play();
      } catch {
        setError(
          "This video cannot play in your browser. Try downloading the MP4.",
        );
      }
    } else element.pause();
  }
  useEffect(() => {
    if (downloads) video.current?.pause();
  }, [downloads]);
  useEffect(() => {
    if (!applying || !submittedJob || submittedJob.id === jobId) return;
    setJobId(submittedJob.id);
    try {
      localStorage.setItem(pendingKey, submittedJob.id);
    } catch {
      /* current-tab tracking still works without storage */
    }
  }, [applying, submittedJob?.id, jobId, pendingKey]);
  useEffect(() => {
    if (submittedJob?.status === "succeeded" && submittedJob.runId) {
      try {
        localStorage.removeItem(pendingKey);
      } catch {
        /* navigation does not depend on storage */
      }
      revision.reset();
      location.hash = `video/${submittedJob.runId}`;
    }
  }, [submittedJob?.status, submittedJob?.runId]);

  return (
    <main
      id="main"
      className={`editor-workspace ${mobileScenes ? "show-scenes" : ""}`}
      onKeyDown={(event) => {
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "z" &&
          !(
            event.target instanceof HTMLElement &&
            event.target.closest("input,textarea")
          )
        ) {
          event.preventDefault();
          if (event.shiftKey) revision.redo();
          else revision.undo();
        }
        if (
          event.key === " " &&
          event.target instanceof HTMLElement &&
          !event.target.closest(
            "button, input, textarea, select, a, audio, video",
          )
        ) {
          event.preventDefault();
          void play();
        }
      }}
    >
      {actionsElement &&
        createPortal(
          <>
            <button
              className="btn quiet undo-button"
              disabled={!revision.canUndo || locked}
              onClick={revision.undo}
            >
              Undo
            </button>
            <button
              className="btn quiet undo-button"
              disabled={!revision.canRedo || locked}
              onClick={revision.redo}
            >
              Redo
            </button>
            <button
              className="btn quiet"
              onClick={() => {
                setVersions(true);
                setCustomize(false);
              }}
            >
              Versions
            </button>
            <button
              className={`btn ${downloads || reviewOpen || exportOpen || revision.plan ? "" : "primary"} mobile-primary`}
              disabled={
                locked ||
                downloads ||
                reviewOpen ||
                exportOpen ||
                revision.stale
              }
              onClick={() => {
                setCustomize(false);
                setVersions(false);
                setFinishing(false);
                if (revision.changeCount) setReviewOpen(true);
                else setExportOpen(true);
              }}
            >
              {applying
                ? "Applying changes…"
                : revision.busy
                  ? "Reviewing changes…"
                  : revision.changeCount
                    ? `Apply ${revision.changeCount} ${revision.changeCount === 1 ? "change" : "changes"}`
                    : "Export MP4"}
            </button>
          </>,
          actionsElement,
        )}
      <aside className="glass side" aria-label="Video details">
        <div className="row between mobile-scene-heading">
          <h2 className="section">Video details</h2>
          <button
            className="btn quiet icon"
            aria-label="Close video details"
            onClick={() => setMobileScenes(false)}
          >
            <Icon name="close" />
          </button>
        </div>
        <Segmented
          label="Video details"
          className="tabs"
          value={panel}
          options={[
            { value: "scenes", label: "Scenes" },
            { value: "narration", label: "Narration" },
            { value: "music", label: "Music" },
          ]}
          onChange={setPanel}
        />
        {panel === "scenes" ? (
          <>
            <div
              className="scenes"
              ref={sceneList}
              role="listbox"
              aria-label="Scenes"
              onKeyDown={(event) => {
                if (event.key === "Delete" && !locked && scenes.length > 1) {
                  event.preventDefault();
                  setCustomize(true);
                  setRemoveMode(true);
                  return;
                }
                if (
                  (event.metaKey || event.ctrlKey || event.altKey) &&
                  ["ArrowUp", "ArrowDown"].includes(event.key)
                ) {
                  event.preventDefault();
                  moveScene(event.key === "ArrowUp" ? -1 : 1);
                  return;
                }
                if (
                  !["ArrowUp", "ArrowDown", "Home", "End"].includes(
                    event.key,
                  ) ||
                  !scenes.length
                )
                  return;
                event.preventDefault();
                const next =
                  event.key === "Home"
                    ? 0
                    : event.key === "End"
                      ? scenes.length - 1
                      : Math.max(
                          0,
                          Math.min(
                            scenes.length - 1,
                            selected + (event.key === "ArrowDown" ? 1 : -1),
                          ),
                        );
                select(next);
                sceneList.current
                  ?.querySelectorAll<HTMLButtonElement>('[role="option"]')
                  [next]?.focus();
              }}
            >
              {scenes.map((scene, index) => (
                <button
                  key={scene.id}
                  className={`scene ${selected === index ? "on" : ""}`}
                  role="option"
                  aria-selected={selected === index}
                  tabIndex={selected === index ? 0 : -1}
                  onClick={() => select(index)}
                >
                  <span className="th">
                    {scene.imageUrl ? (
                      <img src={scene.imageUrl} alt="" loading="lazy" />
                    ) : (
                      <Icon name="film" />
                    )}
                  </span>
                  <span className="scene-copy">
                    <span className="t">
                      Scene {index + 1}
                      {revision.changedScenes.includes(scene.id) && (
                        <span
                          className="edited-dot"
                          aria-label="Unapplied change"
                        />
                      )}
                    </span>
                    <span className="c trunc">
                      {scene.line || "No spoken words"}
                    </span>
                  </span>
                </button>
              ))}
            </div>
            <div className="side-footer">
              <p className="caption">
                {revision.changeCount
                  ? `${revision.changeCount} unapplied changes · draft kept in this browser`
                  : "Completed video stays preserved"}
                <br />
                Preview shows the current completed version.
              </p>
              <button
                className="btn"
                disabled={!scene || locked}
                aria-expanded={customize}
                onClick={() => setCustomize(true)}
              >
                Customize scene
              </button>
              <button
                className="btn quiet"
                disabled={locked}
                onClick={() => {
                  setFinishing(true);
                  setCustomize(false);
                }}
              >
                Customize video
              </button>
              <div className="row mobile-undo">
                <button
                  className="btn quiet"
                  disabled={!revision.canUndo || locked}
                  onClick={revision.undo}
                >
                  Undo
                </button>
                <button
                  className="btn quiet"
                  disabled={!revision.canRedo || locked}
                  onClick={revision.redo}
                >
                  Redo
                </button>
              </div>
            </div>
          </>
        ) : panel === "narration" ? (
          <div className="detail-copy">
            <h2 className="section">Narration</h2>
            {audio && (
              <audio
                aria-label="Narration audio"
                controls
                preload="none"
                src={audio.url}
              />
            )}
            <fieldset className="generation-controls" disabled={locked}>
              <label className="label">
                Narration
                <select
                  className="field"
                  value={
                    draft.narrationEnabled === undefined
                      ? "keep"
                      : String(draft.narrationEnabled)
                  }
                  onChange={(event) =>
                    revision.update(
                      "narrationEnabled",
                      event.target.value === "keep"
                        ? undefined
                        : event.target.value === "true",
                    )
                  }
                >
                  <option value="keep">Keep current</option>
                  <option value="true">On</option>
                  <option value="false">Silent timeline</option>
                </select>
              </label>
              {draft.scenes.map((scene, index) => (
                <label className="advanced-field label" key={scene.id}>
                  Scene {index + 1} words
                  <textarea
                    className="field"
                    value={scene.line}
                    onChange={(event) =>
                      revision.editScenes(
                        draft.scenes.map((candidate) =>
                          candidate.id === scene.id
                            ? { ...candidate, line: event.target.value }
                            : candidate,
                        ),
                      )
                    }
                  />
                </label>
              ))}
              <label className="advanced-field label">
                Voice
                <input
                  className="field"
                  placeholder="Keep current"
                  value={draft.voice ?? ""}
                  onChange={(event) =>
                    revision.update("voice", event.target.value || undefined)
                  }
                />
              </label>
              <label className="advanced-field label">
                Pace
                <input
                  className="field"
                  type="number"
                  min="0.5"
                  max="2"
                  step="0.1"
                  placeholder="Keep current"
                  value={draft.speed ?? ""}
                  onChange={(event) =>
                    revision.update(
                      "speed",
                      Number.isFinite(event.target.valueAsNumber)
                        ? event.target.valueAsNumber
                        : undefined,
                    )
                  }
                />
              </label>
            </fieldset>
            <p className="caption">
              Changing spoken words or their order re-records narration and
              timing. You’ll review the cost before anything starts.
            </p>
          </div>
        ) : (
          <div className="detail-copy">
            <h2 className="section">Music</h2>
            {music ? (
              <audio
                aria-label="Music audio"
                controls
                preload="none"
                src={music.url}
              />
            ) : (
              <p className="caption">No separate soundtrack is available.</p>
            )}
            <fieldset className="generation-controls" disabled={locked}>
              <Segmented
                label="Soundtrack"
                value={draft.musicAction}
                options={[
                  { value: "keep", label: "Keep" },
                  { value: "remove", label: "Remove" },
                  { value: "regenerate", label: "Generate" },
                ]}
                onChange={(value) => revision.update("musicAction", value)}
              />
              <label className="advanced-field label">
                Music description
                <textarea
                  className="field"
                  value={draft.musicPrompt ?? ""}
                  onChange={(event) =>
                    revision.update("musicPrompt", event.target.value)
                  }
                />
              </label>
              <UploadField
                label="Use your track"
                kind="audio"
                value={draft.advanced?.musicAssetId}
                disabled={locked}
                onBusyChange={setUploading}
                onChange={(asset) =>
                  revision.change({
                    ...draft,
                    musicAction: "keep",
                    advanced: { ...draft.advanced, musicAssetId: asset?.id },
                  })
                }
              />
            </fieldset>
            <p className="caption">
              Removing music does not call an AI service. Earlier versions keep
              their soundtrack.
            </p>
          </div>
        )}
      </aside>

      <div
        className="stage"
        tabIndex={0}
        aria-label="Video preview and timeline"
      >
        <div className={`preview ${run.format === "reel" ? "vertical" : ""}`}>
          {run.videoUrl && (
            <video
              ref={video}
              src={run.videoUrl}
              poster={run.posterUrl}
              preload="metadata"
              playsInline
              muted={muted}
              className={scenePreview ? "media-hidden" : ""}
              aria-label={run.title}
              onLoadedMetadata={(event) =>
                setDuration(
                  Number.isFinite(event.currentTarget.duration)
                    ? event.currentTarget.duration
                    : 0,
                )
              }
              onTimeUpdate={(event) =>
                setCurrent(event.currentTarget.currentTime)
              }
              onPlay={() => setPlaying(true)}
              onPause={() => setPlaying(false)}
              onError={() =>
                setError(
                  "The video could not be loaded. It may be incomplete or unavailable on this computer.",
                )
              }
            />
          )}
          {(scenePreview || !run.videoUrl) &&
            (scene?.imageUrl || run.posterUrl ? (
              <img
                src={
                  scenePreview
                    ? (scene?.imageUrl ?? run.posterUrl)
                    : (run.posterUrl ?? scene?.imageUrl)
                }
                alt={scenePreview ? `Scene ${selected + 1}` : run.title}
              />
            ) : (
              <div className="placeholder">
                <Icon name="film" />
                <p>No preview is available yet</p>
              </div>
            ))}
          {scenePreview && (
            <div className="scene-preview-label glass elevated">
              <span className="caption">
                Scene {selected + 1} · image preview
              </span>
              {run.videoUrl && (
                <button
                  className="btn sm"
                  onClick={() => setScenePreview(false)}
                >
                  Show full video
                </button>
              )}
            </div>
          )}
        </div>
        {error && <Notice error>{error}</Notice>}
        {run.status !== "complete" && (
          <Notice>
            {run.status === "damaged"
              ? "This video could not be read. Restore its original files and rescan in Settings."
              : "This video is not finished. Available scene images can still be inspected."}
          </Notice>
        )}
        <section className="glass timeline" aria-label="Playback">
          <div className="transport">
            <button
              className="btn icon"
              aria-label={playing ? "Pause video" : "Play video"}
              disabled={!run.videoUrl}
              onClick={() => void play()}
            >
              <Icon name={playing ? "pause" : "play"} />
            </button>
            <span className="tc">
              {timecode(current)}{" "}
              <span className="ink-2">/ {timecode(duration)}</span>
            </span>
            <span className="caption keyboard-hint">
              Space to play · arrows to explore scenes
            </span>
            <span className="grow" />
            <button
              className="btn quiet mobile-scenes-button"
              onClick={() => setMobileScenes(true)}
            >
              Scenes
            </button>
            <button
              className="btn quiet icon"
              aria-label={muted ? "Unmute video" : "Mute video"}
              disabled={!run.videoUrl}
              onClick={() => setMuted(!muted)}
            >
              <Icon name={muted ? "muted" : "volume"} />
            </button>
            <button
              className="btn quiet icon fullscreen-button"
              aria-label="Full screen"
              disabled={!run.videoUrl}
              onClick={() => {
                setScenePreview(false);
                void video.current
                  ?.requestFullscreen()
                  .catch((error) => setError(errorMessage(error)));
              }}
            >
              <Icon name="full" />
            </button>
          </div>
          <input
            className="scrubber"
            aria-label="Playback position"
            aria-valuetext={`${timecode(current)} of ${timecode(duration)}`}
            type="range"
            min="0"
            max={duration || 1}
            step="0.1"
            value={current}
            disabled={!duration}
            onChange={(event) => {
              if (video.current) {
                const time = Number(event.target.value);
                video.current.currentTime = time;
                setCurrent(time);
                setScenePreview(false);
              }
            }}
          />
          <div className="track" aria-label="Scene images">
            {scenes.map((scene, index) => (
              <button
                key={scene.id}
                className={`seg-clip ${selected === index ? "on" : ""}`}
                aria-label={`Preview scene ${index + 1}`}
                aria-pressed={selected === index && scenePreview}
                onClick={() => select(index)}
              >
                {scene.imageUrl && (
                  <img src={scene.imageUrl} alt="" loading="lazy" />
                )}
                <span className="n">{index + 1}</span>
              </button>
            ))}
          </div>
          <p className="caption timeline-caption">
            {scenePreview
              ? scene?.line
              : `${scenes.length} scenes · ${run.format === "youtube" ? "Widescreen" : "Vertical"}`}
          </p>
          {!!draft.chapters?.length && (
            <div className="chapter-navigation" aria-label="Chapters">
              {draft.chapters.map((chapter, index) => (
                <button
                  key={index}
                  className="btn quiet sm"
                  onClick={() => select(chapter.sceneStart)}
                >
                  {chapter.title}
                </button>
              ))}
            </div>
          )}
        </section>
        {revision.error && <Notice error>{revision.error}</Notice>}
        {revision.stale && (
          <Notice error>
            This video changed outside this tab. Your draft has not been
            applied.{" "}
            <button className="btn quiet" onClick={revision.reset}>
              Discard stale draft
            </button>
          </Notice>
        )}
        {submittedJob?.error && (
          <Notice error>
            {submittedJob.error} Open Activity to review recovery options.
          </Notice>
        )}
      </div>

      {customize && scene && (
        <Overlay
          title={`Scene ${selected + 1}`}
          onClose={() => setCustomize(false)}
        >
          <fieldset
            className="scene-inspector generation-controls"
            disabled={locked}
          >
            <label className="label">
              Words
              <textarea
                className="field"
                aria-label="Words"
                value={scene.line}
                onChange={(event) => editScene({ line: event.target.value })}
              />
            </label>
            <p className="caption">
              Changing the words re-records the whole narration.
            </p>
            {!!draft.characters?.length && (
              <details>
                <summary className="label">Characters in this scene</summary>
                {draft.characters.map((character, index) => (
                  <label className="trust-reuse label" key={character.id}>
                    <input
                      type="checkbox"
                      checked={scene.castIds?.includes(character.id) ?? false}
                      onChange={(event) =>
                        editScene({
                          castIds: event.target.checked
                            ? [...(scene.castIds ?? []), character.id]
                            : scene.castIds?.filter(
                                (id) => id !== character.id,
                              ),
                        })
                      }
                    />
                    Character {index + 1}: {character.description.slice(0, 80)}
                  </label>
                ))}
              </details>
            )}
            {!!draft.locations?.length && (
              <label className="label">
                Scene location
                <select
                  className="field"
                  value={scene.locationId ?? ""}
                  onChange={(event) =>
                    editScene({ locationId: event.target.value })
                  }
                >
                  <option value="">No shared location</option>
                  {draft.locations.map((location, index) => (
                    <option key={location.id} value={location.id}>
                      Location {index + 1}: {location.description.slice(0, 60)}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="label">
              Image
              <textarea
                className="field"
                aria-label="Image"
                value={scene.imagePrompt}
                onChange={(event) =>
                  editScene({ imagePrompt: event.target.value })
                }
              />
            </label>
            <label className="label">
              Motion
              <textarea
                className="field"
                aria-label="Motion"
                value={scene.motionPrompt}
                placeholder="Describe the camera or subject’s movement"
                onChange={(event) =>
                  editScene({ motionPrompt: event.target.value })
                }
              />
            </label>
            <Item
              label="Current image"
              caption="Keep retains its original creation settings, even when your description changes."
            >
              <select
                className="field"
                aria-label="Current image"
                value={scene.imageAction ?? "auto"}
                onChange={(event) => {
                  const value = event.target.value;
                  editScene({
                    imageAction:
                      value === "keep" || value === "regenerate"
                        ? value
                        : undefined,
                  });
                }}
              >
                <option value="auto">Update when needed</option>
                <option value="keep">Keep</option>
                <option value="regenerate">Regenerate</option>
              </select>
            </Item>
            <Item label="Animate this scene">
              <Toggle
                label="Animate this scene"
                checked={draft.videoSceneIds.includes(scene.id)}
                onChange={(enabled) =>
                  revision.update(
                    "videoSceneIds",
                    enabled
                      ? [...draft.videoSceneIds, scene.id]
                      : draft.videoSceneIds.filter((id) => id !== scene.id),
                  )
                }
              />
            </Item>
            {draft.videoSceneIds.includes(scene.id) && (
              <Item label="Current motion">
                <select
                  className="field"
                  aria-label="Current motion"
                  value={scene.motionAction ?? "auto"}
                  onChange={(event) => {
                    const value = event.target.value;
                    editScene({
                      motionAction:
                        value === "keep" || value === "regenerate"
                          ? value
                          : undefined,
                    });
                  }}
                >
                  <option value="auto">Update when needed</option>
                  <option value="keep">Keep</option>
                  <option value="regenerate">Regenerate</option>
                </select>
              </Item>
            )}
            {scene.videoUrl && (
              <video
                className="scene-motion-preview"
                controls
                playsInline
                preload="none"
                src={scene.videoUrl}
                aria-label="Current scene motion"
              />
            )}
            <div className="row">
              <button
                className="btn quiet"
                disabled={selected === 0}
                onClick={() => moveScene(-1)}
              >
                Move earlier
              </button>
              <button
                className="btn quiet"
                disabled={selected >= scenes.length - 1}
                onClick={() => moveScene(1)}
              >
                Move later
              </button>
            </div>
            <button
              className="btn quiet"
              disabled={scenes.length < 2}
              onClick={() => setRemoveMode(!removeMode)}
            >
              Remove scene
            </button>
            {removeMode && (
              <div className="notice">
                <p>
                  Keep its spoken words in the neighboring scene, or remove them
                  too? Both can be undone before applying.
                </p>
                <button className="btn quiet" onClick={() => removeScene(true)}>
                  Keep spoken words
                </button>
                <button
                  className="btn quiet"
                  onClick={() => removeScene(false)}
                >
                  Remove scene and words
                </button>
              </div>
            )}
          </fieldset>
        </Overlay>
      )}

      {versions && <Versions run={run} onClose={() => setVersions(false)} />}

      {finishing && (
        <Overlay title="Customize video" onClose={() => setFinishing(false)}>
          <fieldset className="generation-controls" disabled={locked}>
            <button
              className="btn quiet"
              disabled={!revision.changeCount}
              onClick={revision.reset}
            >
              Discard unapplied changes
            </button>
            <div className="list">
              <Item
                label="Format"
                caption="A new shape may require new images and clips."
              >
                <select
                  className="field"
                  aria-label="Version format"
                  value={draft.format ?? ""}
                  onChange={(event) => {
                    const value = event.target.value;
                    revision.update(
                      "format",
                      value === "reel" || value === "youtube"
                        ? value
                        : undefined,
                    );
                  }}
                >
                  <option value="">Keep current</option>
                  {formats.map((format) => (
                    <option key={format.value} value={format.value}>
                      {format.label}
                    </option>
                  ))}
                </select>
              </Item>
              <Item
                label="Quality"
                caption="Review which assets change before approving."
              >
                <select
                  className="field"
                  aria-label="Version quality"
                  value={draft.quality ?? ""}
                  onChange={(event) => {
                    const value = event.target.value;
                    revision.update(
                      "quality",
                      value === "draft" ||
                        value === "standard" ||
                        value === "premium"
                        ? value
                        : undefined,
                    );
                  }}
                >
                  <option value="">Keep current</option>
                  {qualities.map((quality) => (
                    <option key={quality.value} value={quality.value}>
                      {quality.label}
                    </option>
                  ))}
                </select>
              </Item>
              <Item label="Captions">
                <select
                  className="field"
                  aria-label="Version captions"
                  value={
                    draft.captions === undefined
                      ? "keep"
                      : String(draft.captions)
                  }
                  onChange={(event) =>
                    revision.update(
                      "captions",
                      event.target.value === "keep"
                        ? undefined
                        : event.target.value === "true",
                    )
                  }
                >
                  <option value="keep">Keep current</option>
                  <option value="true">On</option>
                  <option value="false">Off</option>
                </select>
              </Item>
              <Item label="Caption style">
                <select
                  className="field"
                  aria-label="Version caption style"
                  value={draft.captionStyle ?? ""}
                  onChange={(event) => {
                    const value = event.target.value;
                    revision.update(
                      "captionStyle",
                      value === "burst" ||
                        value === "karaoke" ||
                        value === "boxed" ||
                        value === "minimal"
                        ? value
                        : undefined,
                    );
                  }}
                >
                  <option value="">Keep current</option>
                  {captionStyles.map((style) => (
                    <option key={style.value} value={style.value}>
                      {style.label}
                    </option>
                  ))}
                </select>
              </Item>
              <Item label="Scene animation">
                <button
                  className="btn quiet"
                  onClick={() => revision.update("videoSceneIds", [])}
                >
                  Use still images
                </button>
                <button
                  className="btn quiet"
                  onClick={() =>
                    revision.update(
                      "videoSceneIds",
                      draft.scenes.map((scene) => scene.id),
                    )
                  }
                >
                  Animate all
                </button>
              </Item>
            </div>
            <AdvancedControls
              revision
              value={draft.advanced}
              onBusyChange={setUploading}
              onChange={(value) => revision.update("advanced", value)}
            />
            <label className="advanced-field label">
              Poster concept
              <textarea
                className="field"
                value={draft.posterPrompt ?? ""}
                onChange={(event) =>
                  revision.update("posterPrompt", event.target.value)
                }
              />
            </label>
            <EntityFields
              kind="character"
              value={draft.characters ?? []}
              onChange={(characters) =>
                revision.change({
                  ...draft,
                  characters,
                  scenes: draft.scenes.map((scene) => ({
                    ...scene,
                    castIds: scene.castIds?.filter((id) =>
                      characters.some((character) => character.id === id),
                    ),
                  })),
                })
              }
            />
            <EntityFields
              kind="location"
              value={draft.locations ?? []}
              onChange={(locations) =>
                revision.change({
                  ...draft,
                  locations,
                  scenes: draft.scenes.map((scene) => ({
                    ...scene,
                    locationId: locations.some(
                      (location) => location.id === scene.locationId,
                    )
                      ? scene.locationId
                      : "",
                  })),
                })
              }
            />
            {!!draft.chapters?.length && (
              <details className="chapter-editor">
                <summary className="label">Chapters</summary>
                {draft.chapters.map((chapter, index) => (
                  <label key={index} className="advanced-field label">
                    Chapter {index + 1} title
                    <input
                      className="field"
                      value={chapter.title}
                      onChange={(event) =>
                        revision.update(
                          "chapters",
                          draft.chapters?.map((candidate, position) =>
                            position === index
                              ? { ...candidate, title: event.target.value }
                              : candidate,
                          ),
                        )
                      }
                    />
                  </label>
                ))}
              </details>
            )}
          </fieldset>
        </Overlay>
      )}

      {(reviewOpen || exportOpen || revision.plan) && (
        <Overlay
          title={
            exportOpen
              ? "Export MP4"
              : `Apply ${revision.changeCount} ${revision.changeCount === 1 ? "change" : "changes"}`
          }
          kind="sheet"
          onClose={() => {
            if (!revision.busy) {
              revision.closeReview();
              setReviewOpen(false);
              setExportOpen(false);
            }
          }}
        >
          {!revision.plan ? (
            <>
              <p className="body ink-2">
                This completed video stays exactly as it is. Review the work and
                cost before creating a new version.
              </p>
              <fieldset className="generation-controls" disabled={locked}>
                {exportOpen && (
                  <>
                    <Segmented
                      label="Export preset"
                      value={exportPreset}
                      options={[
                        { value: "web", label: "Web" },
                        { value: "social", label: "Social" },
                        { value: "pro", label: "Pro" },
                      ]}
                      onChange={setExportPreset}
                    />
                    <p className="caption overlay-note">
                      H.264 MP4 at the current video dimensions. Web balances
                      size and quality; Social and Pro retain more detail.
                      Changing the shape requires a new version.
                    </p>
                  </>
                )}
                <label className="trust-reuse label">
                  <input
                    type="checkbox"
                    checked={draft.legacyReuse === "trust"}
                    onChange={(event) =>
                      revision.update(
                        "legacyReuse",
                        event.target.checked ? "trust" : undefined,
                      )
                    }
                  />
                  Keep existing media when its original creation settings are
                  unknown. I have reviewed these assets.
                </label>
                <Item label="Spending limit">
                  <input
                    className="field compact tc"
                    aria-label="Revision spending limit"
                    type="number"
                    min="0"
                    max="1000"
                    step="0.5"
                    value={Number.isFinite(draft.maxCost) ? draft.maxCost : ""}
                    onChange={(event) =>
                      revision.update("maxCost", event.target.valueAsNumber)
                    }
                  />
                </Item>
              </fieldset>
              {revision.error && <Notice error>{revision.error}</Notice>}
              <div className="overlay-footer">
                <button
                  className="btn quiet"
                  disabled={locked || !run.videoUrl}
                  onClick={() => {
                    setExportOpen(false);
                    setReviewOpen(false);
                    setDownloads(true);
                  }}
                >
                  Download current MP4
                </button>
                <button
                  className="btn primary"
                  disabled={
                    locked ||
                    !Number.isFinite(draft.maxCost) ||
                    draft.maxCost < 0 ||
                    draft.maxCost > 1000 ||
                    revision.stale
                  }
                  onClick={() =>
                    void revision.review(
                      exportOpen ? { ...draft, exportPreset } : draft,
                    )
                  }
                >
                  {revision.busy ? "Reviewing work…" : "Review changes"}
                </button>
              </div>
            </>
          ) : (
            <>
              <p className="body ink-2">
                About{" "}
                <span className="tc">
                  {money(revision.plan.estimate.total_usd)}
                </span>
                . Your earlier version stays unchanged.
              </p>
              {(
                [
                  ["generate", "Regenerate"],
                  ["reuse", "Keep"],
                  ["exclude", "Remove"],
                  ["derive", "Restyle"],
                ] as const
              ).map(([action, label]) => {
                const items =
                  revision.plan?.actions.filter(
                    (item) => item.action === action,
                  ) ?? [];
                return items.length ? (
                  <section key={action}>
                    <h3 className="label">{label}</h3>
                    <ul className="review-actions">
                      {items.map((item, index) => (
                        <li key={index}>
                          {revisionActionLabel(item, draft.scenes)}
                        </li>
                      ))}
                    </ul>
                  </section>
                ) : null;
              })}
              <p className="caption">
                {revision.plan.estimate.warning} This is an estimate guard, not
                a guaranteed billing cap.
              </p>
              {revision.plan.warnings.map((warning, index) => (
                <Notice key={index}>{warning}</Notice>
              ))}
              {revision.overBudget && (
                <Notice error>
                  The estimate exceeds your {money(draft.maxCost)} spending
                  limit. Close this review to change it.
                </Notice>
              )}
              {revision.expired && (
                <Notice error>
                  This review expired. Close it and request a new estimate.
                </Notice>
              )}
              {revision.error && <Notice error>{revision.error}</Notice>}
              <div className="overlay-footer">
                <button
                  className="btn quiet"
                  disabled={revision.busy}
                  onClick={revision.closeReview}
                >
                  Back
                </button>
                <button
                  className="btn primary"
                  disabled={
                    revision.busy ||
                    revision.overBudget ||
                    revision.expired ||
                    revision.stale
                  }
                  onClick={() => void revision.apply()}
                >
                  {revision.busy
                    ? "Starting work…"
                    : exportOpen
                      ? "Export MP4"
                      : "Apply changes"}
                </button>
              </div>
            </>
          )}
        </Overlay>
      )}

      {downloads && (
        <Overlay
          title="Ready to download"
          kind="success"
          onClose={() => setDownloads(false)}
        >
          {run.posterUrl && (
            <img
              className="download-poster"
              src={run.posterUrl}
              alt={`Poster for ${run.title}`}
            />
          )}
          <h2 className="section">{run.title}</h2>
          <p className="caption">
            Current completed version · downloads do not modify your video.
          </p>
          <div className="list companion-downloads">
            {run.artifacts
              .filter(
                (artifact) =>
                  artifact.kind !== "video" &&
                  (artifact.kind !== "image" ||
                    artifact.name === "poster.jpg") &&
                  artifact.kind !== "audio",
              )
              .map((artifact) => (
                <Item
                  key={artifact.name}
                  label={
                    artifact.kind === "image"
                      ? "Poster"
                      : artifact.kind === "captions"
                        ? "Captions"
                        : "Description and chapters"
                  }
                >
                  <a
                    className="btn quiet"
                    href={artifact.url}
                    download={artifact.name}
                  >
                    <Icon name="download" />
                    <span className="sr-only">Download {artifact.name}</span>
                  </a>
                </Item>
              ))}
          </div>
          <p className="caption">
            Saved on this computer. Share the downloaded file; this address is
            not a public sharing link.
          </p>
          {run.videoUrl ? (
            <a
              className="btn primary lg download-primary"
              href={run.videoUrl}
              download={`${run.title.replace(/[^\p{L}\p{N} _-]/gu, "").slice(0, 100) || "video"}.mp4`}
            >
              <Icon name="download" />
              Download MP4
            </a>
          ) : (
            <Notice>No completed video is available to download.</Notice>
          )}
        </Overlay>
      )}
    </main>
  );
}
