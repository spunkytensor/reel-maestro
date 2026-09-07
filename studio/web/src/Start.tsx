import { useEffect, useRef, useState } from "react";
import type { Job, Plan, PlanInput, Settings, UrlSource } from "../../shared";
import { api, approvalKey as createApprovalKey, errorMessage } from "./api";
import { AdvancedControls } from "./AdvancedControls";
import { UploadField } from "./UploadField";
import {
  captionStyles,
  formats,
  Item,
  money,
  Notice,
  Overlay,
  qualities,
  Segmented,
  Toggle,
} from "./components";
import { readDefaults } from "./preferences";

export function Start({
  settings,
  onJob,
}: {
  settings: Settings | undefined;
  onJob: (job: Job) => void;
}) {
  const [input, setInput] = useState<PlanInput>(() => ({
    ...readDefaults(),
    prompt: "",
    source: "topic",
    minutes: 1,
    narration: true,
    music: false,
    video: "off",
    captions: true,
    captionStyle: "burst",
    speed: 1,
  }));
  const [customize, setCustomize] = useState(false);
  const [plan, setPlan] = useState<Plan>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [now, setNow] = useState(Date.now());
  const approvalKey = useRef("");
  const submitting = useRef(false);
  useEffect(() => {
    if (!plan) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [plan]);
  function change<K extends keyof PlanInput>(key: K, value: PlanInput[K]) {
    setInput((previous) => ({
      ...previous,
      [key]: value,
      ...(key === "prompt" ? { sourceAssetId: undefined } : {}),
    }));
    setPlan(undefined);
    setError("");
  }
  const invalid =
    !input.prompt.trim() ||
    !Number.isFinite(input.maxCost) ||
    input.maxCost < 0 ||
    input.maxCost > 1000;
  const overBudget = plan && plan.estimate.total_usd > input.maxCost;
  const expired = plan && now >= Date.parse(plan.expiresAt);

  async function estimate() {
    if (
      submitting.current ||
      uploading ||
      invalid ||
      plan ||
      !settings?.generationAvailable ||
      !settings.providerConnected
    )
      return;
    submitting.current = true;
    setBusy(true);
    setError("");
    setCustomize(false);
    try {
      let request = input;
      if (
        input.source === "url" ||
        /^https?:\/\/\S+$/i.test(input.prompt.trim())
      ) {
        const article = await api<UrlSource>("/sources/url", {
          url: input.prompt.trim(),
        });
        request = {
          ...input,
          prompt: article.text,
          source: "brief",
          sourceAssetId: undefined,
        };
        setInput(request);
      }
      const result = await api<Plan>("/plans", request);
      approvalKey.current = createApprovalKey();
      setPlan(result);
      setNow(Date.now());
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
      submitting.current = false;
    }
  }
  async function generate() {
    if (!plan || expired || overBudget || submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setError("");
    try {
      const job = await api<Job>("/jobs", {
        planId: plan.planId,
        planHash: plan.planHash,
        idempotencyKey: approvalKey.current,
      });
      setPlan(undefined);
      onJob(job);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
      submitting.current = false;
    }
  }

  return (
    <main
      id="main"
      className={`start-page ${customize || plan ? "customizing" : ""}`}
    >
      <section className="glass start-card">
        <h1 className="title">Describe the video you want</h1>
        <p className="body ink-2 lead">
          A topic is enough. You can also paste an article link, a brief, or a
          script you have already written.
        </p>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void estimate();
          }}
        >
          <fieldset
            className="generation-controls"
            disabled={busy || uploading || !!plan}
          >
            <label className="sr-only" htmlFor="prompt">
              Describe your video
            </label>
            <textarea
              id="prompt"
              className="field big prompt"
              placeholder="The history of espresso — how a rushed Italian invention changed our mornings"
              maxLength={100000}
              value={input.prompt}
              disabled={busy || !!plan}
              onChange={(event) => change("prompt", event.target.value)}
            />
            <details className="source-upload">
              <summary className="caption">Use a text file</summary>
              <UploadField
                label="Script or brief"
                kind="text"
                value={input.sourceAssetId}
                disabled={busy || !!plan}
                onBusyChange={setUploading}
                onChange={(asset) => {
                  setInput((previous) => ({
                    ...previous,
                    sourceAssetId: asset?.id,
                    ...(asset?.text
                      ? { prompt: asset.text, source: "script" }
                      : {}),
                  }));
                  setPlan(undefined);
                }}
              />
            </details>
            <div className="choices">
              <Segmented
                label="Video format"
                value={input.format}
                options={formats}
                onChange={(value) => change("format", value)}
              />
              {input.format === "youtube" && (
                <Segmented
                  label="Video length"
                  value={String(input.minutes)}
                  options={[
                    { value: "1", label: "About 1 minute" },
                    { value: "3", label: "3 minutes" },
                    { value: "6", label: "6 minutes" },
                  ]}
                  onChange={(value) => change("minutes", Number(value))}
                />
              )}
              <Segmented
                label="Video quality"
                value={input.quality}
                options={qualities}
                onChange={(value) => {
                  change("quality", value);
                  if (value === "draft") {
                    change("music", false);
                    change("video", "off");
                  }
                }}
              />
            </div>
            <p className="caption summary">
              Narration {input.narration ? "on" : "off"} · captions{" "}
              {input.captions ? "on" : "off"} · music{" "}
              {input.music ? "on" : "off"} · video clips{" "}
              {input.video === "all" ? "on" : "off"} ·{" "}
              <button
                type="button"
                className="text-button"
                aria-expanded={customize}
                onClick={() => {
                  setCustomize(!customize);
                  setPlan(undefined);
                }}
              >
                Customize
              </button>
            </p>
          </fieldset>
          <div className="start-footer">
            <p className="caption" aria-live="polite">
              {!settings?.generationAvailable ? (
                <>
                  Generation is unavailable.{" "}
                  <a className="text-button" href="#settings">
                    Open Settings
                  </a>
                </>
              ) : !settings.providerConnected ? (
                <>
                  Connect your AI service to generate.{" "}
                  <a className="text-button" href="#settings">
                    Connect
                  </a>
                </>
              ) : (
                <>
                  Review the cost before anything starts.
                  <br />
                  Spending limit{" "}
                  <span className="tc">{money(input.maxCost)}</span>
                </>
              )}
            </p>
            <button
              type="submit"
              className={`btn lg ${plan ? "" : "primary"} mobile-primary`}
              aria-hidden={!!plan || undefined}
              disabled={
                invalid ||
                busy ||
                uploading ||
                !!plan ||
                !settings?.generationAvailable ||
                !settings.providerConnected
              }
            >
              {busy && !plan ? "Estimating cost…" : "Generate video"}
            </button>
          </div>
        </form>
        {error && !plan && <Notice error>{error}</Notice>}
      </section>

      {customize && (
        <Overlay title="Customize" onClose={() => setCustomize(false)}>
          <fieldset
            className="generation-controls"
            disabled={busy || uploading || !!plan}
          >
            <div className="list">
              <Item
                label="Your words"
                caption="A script is narrated verbatim; a brief guides the writing."
              >
                <Segmented
                  label="Source type"
                  value={input.source}
                  options={[
                    { value: "topic", label: "Topic" },
                    { value: "brief", label: "Brief" },
                    { value: "script", label: "Script" },
                    { value: "url", label: "Link" },
                  ]}
                  onChange={(value) => change("source", value)}
                />
              </Item>
              <Item label="Narration" caption="Automatic voice · normal pace">
                <Toggle
                  label="Narration"
                  checked={input.narration}
                  onChange={(value) => change("narration", value)}
                />
              </Item>
              {input.narration && (
                <>
                  <Item
                    label="Voice"
                    caption="Leave blank to use this computer’s default voice"
                  >
                    <input
                      className="field compact"
                      aria-label="Voice"
                      value={input.voice ?? ""}
                      maxLength={100}
                      onChange={(event) => change("voice", event.target.value)}
                      placeholder="Automatic"
                    />
                  </Item>
                  <Item label="Pace">
                    <select
                      className="field compact"
                      aria-label="Narration pace"
                      value={input.speed}
                      onChange={(event) =>
                        change("speed", Number(event.target.value))
                      }
                    >
                      <option value={0.8}>Slower</option>
                      <option value={1}>Normal</option>
                      <option value={1.2}>Faster</option>
                    </select>
                  </Item>
                </>
              )}
              <Item
                label="Music"
                caption="Generated music ducks under the narration"
              >
                <Segmented
                  label="Music"
                  value={
                    input.advanced?.musicAssetId
                      ? "uploaded"
                      : input.music
                        ? "generated"
                        : "none"
                  }
                  options={[
                    { value: "generated", label: "Generated" },
                    { value: "none", label: "None" },
                    { value: "uploaded", label: "Your track" },
                  ]}
                  onChange={(value) => {
                    change("music", value === "generated");
                    if (value !== "uploaded")
                      change("advanced", {
                        ...input.advanced,
                        musicAssetId: undefined,
                      });
                    else
                      document
                        .getElementById("music-upload")
                        ?.scrollIntoView({ block: "nearest" });
                  }}
                />
              </Item>
              <div id="music-upload">
                <UploadField
                  label="Your music track"
                  kind="audio"
                  value={input.advanced?.musicAssetId}
                  onBusyChange={setUploading}
                  onChange={(asset) => {
                    change("advanced", {
                      ...input.advanced,
                      musicAssetId: asset?.id,
                    });
                    if (asset) change("music", false);
                  }}
                />
              </div>
              <Item
                label="Video clips"
                caption="Animated scenes cost more and take longer"
              >
                <Segmented
                  label="Video clips"
                  value={input.video}
                  options={[
                    { value: "off", label: "Off" },
                    { value: "all", label: "All scenes" },
                  ]}
                  onChange={(value) => change("video", value)}
                />
              </Item>
              <Item label="Captions" caption="Burned into the video">
                <Toggle
                  label="Captions"
                  checked={input.captions}
                  onChange={(value) => change("captions", value)}
                />
              </Item>
              {input.captions && (
                <Segmented
                  label="Caption style"
                  value={input.captionStyle}
                  options={captionStyles}
                  onChange={(value) => change("captionStyle", value)}
                />
              )}
              <Item
                label="Spending limit"
                caption="Stops before starting if the estimate is higher"
              >
                <input
                  className="field compact tc"
                  aria-label="Spending limit in dollars"
                  type="number"
                  min="0"
                  max="1000"
                  step="0.5"
                  value={Number.isFinite(input.maxCost) ? input.maxCost : ""}
                  onChange={(event) =>
                    change("maxCost", event.target.valueAsNumber)
                  }
                />
              </Item>
              <Item
                label="Models"
                caption={`${qualities.find((q) => q.value === input.quality)?.label} preset. Customize models and finishing below.`}
              />
            </div>
            <AdvancedControls
              value={input.advanced}
              onBusyChange={setUploading}
              onChange={(value) => change("advanced", value)}
            />
          </fieldset>
        </Overlay>
      )}

      {plan && (
        <Overlay
          title="Generate video"
          kind="sheet"
          onClose={() => {
            if (!busy) setPlan(undefined);
          }}
        >
          <p className="body ink-2">
            About <span className="tc">{money(plan.estimate.total_usd)}</span>{" "}
            for a new video. Nothing starts until you approve.
          </p>
          <div className="list cost-list">
            {Object.entries({
              script_usd: "Words",
              narration_usd: "Narration",
              images_usd: "Images and poster",
              video_usd: "Video clips",
              music_usd: "Music",
            }).map(([key, label]) => {
              const amount = plan.estimate[key];
              return typeof amount === "number" && amount > 0 ? (
                <Item key={key} label={label}>
                  <span className="tc">{money(amount)}</span>
                </Item>
              ) : null;
            })}
          </div>
          <p className="caption">
            {plan.estimate.warning} The spending limit checks the estimate; it
            is not a guaranteed billing cap. Completion time depends on the
            requested work.
          </p>
          {overBudget && (
            <Notice error>
              This video is estimated at {money(plan.estimate.total_usd)}, above
              your {money(input.maxCost)} spending limit. Choose Quick draft or
              raise the limit in Customize.
            </Notice>
          )}
          {expired && (
            <Notice error>
              This estimate has expired. Close this review and request a new
              estimate.
            </Notice>
          )}
          {error && <Notice error>{error}</Notice>}
          <div className="overlay-footer">
            <button
              className="btn quiet"
              disabled={busy}
              onClick={() => setPlan(undefined)}
            >
              Cancel
            </button>
            <button
              className="btn primary"
              disabled={busy || !!overBudget || !!expired}
              onClick={() => void generate()}
            >
              {busy ? "Starting video…" : "Generate video"}
            </button>
          </div>
        </Overlay>
      )}
    </main>
  );
}
