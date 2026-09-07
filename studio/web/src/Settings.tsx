import { useState } from "react";
import type { Settings as SettingsData } from "../../shared";
import { api, errorMessage } from "./api";
import {
  formats,
  Item,
  Notice,
  Overlay,
  qualities,
  Segmented,
  Toggle,
} from "./components";
import {
  persist,
  readDefaults,
  type Appearance,
  type Defaults,
} from "./preferences";

export function Settings({
  settings,
  appearance,
  onAppearance,
  onRefresh,
  scanning,
}: {
  settings: SettingsData | undefined;
  appearance: Appearance;
  onAppearance: (value: Appearance) => boolean;
  onRefresh: () => Promise<void>;
  scanning: boolean;
}) {
  const [defaults, setDefaults] = useState<Defaults>(readDefaults);
  const [keyOpen, setKeyOpen] = useState(false);
  const [info, setInfo] = useState<"help" | "licenses">();
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  function update(value: Partial<Defaults>) {
    const next = { ...defaults, ...value };
    setDefaults(next);
    if (
      Number.isFinite(next.maxCost) &&
      next.maxCost >= 0 &&
      next.maxCost <= 1000
    ) {
      if (!persist("reelmaestro.defaults", next))
        setError(
          "Your browser could not remember these preferences. Allow local storage to keep them after closing Studio.",
        );
    }
  }
  function appearanceChange(value: Partial<Appearance>) {
    if (!onAppearance({ ...appearance, ...value }))
      setError(
        "Appearance changed for this visit, but your browser could not remember it.",
      );
  }
  async function connect() {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await api("/settings", { apiKey: apiKey.trim() });
      setApiKey("");
      setKeyOpen(false);
      await onRefresh();
      setMessage(
        "Your key was stored privately. No paid connection test was made.",
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <main id="main" className="page settings-page">
      <h1 className="title">Settings</h1>
      {error && <Notice error>{error}</Notice>}
      {message && <Notice>{message}</Notice>}
      <div className="settings-grid">
        <section className="glass group">
          <h2 className="section">Appearance</h2>
          <div className="list">
            <Item label="Theme" caption="System follows your device">
              <Segmented
                label="Theme"
                value={appearance.theme}
                options={[
                  { value: "system", label: "System" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ]}
                onChange={(theme) => appearanceChange({ theme })}
              />
            </Item>
            <Item label="Reduce transparency" caption="Solid panels, no blur">
              <Toggle
                label="Reduce transparency"
                checked={appearance.solid}
                onChange={(solid) => appearanceChange({ solid })}
              />
            </Item>
            <Item
              label="Reduce motion"
              caption="Your device preference is always respected"
            >
              <Toggle
                label="Reduce motion"
                checked={appearance.motion}
                onChange={(motion) => appearanceChange({ motion })}
              />
            </Item>
          </div>
        </section>
        <section className="glass group">
          <h2 className="section">Generation</h2>
          <div className="list">
            <Item
              label="AI provider"
              caption="Keys are write-only. Status indicates configuration, not a connection test."
            >
              <div className="key-status">
                <span
                  className={`pill ${settings?.providerConnected ? "ok" : "warn"}`}
                >
                  <span className="dot" />
                  {settings?.providerConnected
                    ? "Key configured"
                    : "Not connected"}
                </span>
                <button className="btn quiet" onClick={() => setKeyOpen(true)}>
                  {settings?.providerConnected ? "Change key" : "Connect"}
                </button>
              </div>
            </Item>
            <Item label="Default format">
              <Segmented
                label="Default format"
                value={defaults.format}
                options={formats}
                onChange={(format) => update({ format })}
              />
            </Item>
            <Item label="Default quality">
              <Segmented
                label="Default quality"
                value={defaults.quality}
                options={qualities}
                onChange={(quality) => update({ quality })}
              />
            </Item>
            <Item
              label="Spending limit"
              caption="Estimate guard per video, not a guaranteed billing cap"
            >
              <input
                aria-label="Default spending limit in dollars"
                className="field compact tc"
                type="number"
                min="0"
                max="1000"
                step="0.5"
                value={
                  Number.isFinite(defaults.maxCost) ? defaults.maxCost : ""
                }
                onChange={(event) =>
                  update({ maxCost: event.target.valueAsNumber })
                }
              />
            </Item>
            <p className="caption overlay-note">
              Defaults are remembered in this browser. Narration and captions
              start on; music and video clips start off.
            </p>
          </div>
        </section>
        <section className="glass group">
          <h2 className="section">Videos</h2>
          <div className="list">
            <Item
              label="Saved to"
              caption={settings?.outputLocation ?? "Loading saved location…"}
            >
              <button
                className="btn quiet"
                disabled={scanning}
                onClick={() => {
                  setMessage("");
                  void onRefresh()
                    .then(() =>
                      setMessage("Your saved videos have been rescanned."),
                    )
                    .catch((error) => setError(errorMessage(error)));
                }}
              >
                {scanning ? "Rescanning…" : "Rescan"}
              </button>
            </Item>
            <Item
              label="Existing videos"
              caption="Imported read-only. Originals are never modified."
            />
            <Item
              label="Export watermark"
              caption="Downloads keep the original video unchanged. Studio adds no branding."
            />
            <Item
              label="Versions"
              caption="Safe editing and version history are not available yet."
            />
          </div>
        </section>
        <section className="glass group">
          <h2 className="section">About</h2>
          <div className="about-copy">
            <p>
              Reel Maestro Studio <span className="tc">0.1</span>
            </p>
            <p className="caption">by Spunky Tensor</p>
          </div>
          <div className="list">
            <Item
              label="Generation tools"
              caption={
                settings?.generationAvailable
                  ? "Reel Maestro is installed"
                  : "Build the Reel Maestro CLI before generating videos"
              }
            />
            <Item label="Licenses">
              <button className="btn quiet" onClick={() => setInfo("licenses")}>
                View
              </button>
            </Item>
            <Item label="Help">
              <button className="btn quiet" onClick={() => setInfo("help")}>
                Open
              </button>
            </Item>
          </div>
        </section>
      </div>
      {keyOpen && (
        <Overlay
          title="Connect AI provider"
          onClose={() => {
            if (!busy) {
              setKeyOpen(false);
              setApiKey("");
            }
          }}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void connect();
            }}
          >
            <label className="label" htmlFor="api-key">
              OpenRouter API key
            </label>
            <input
              id="api-key"
              className="field key-field"
              type="password"
              autoComplete="new-password"
              spellCheck={false}
              maxLength={4096}
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
            />
            <p className="caption">
              Stored privately on this computer, never returned to the browser.
              Generation uses paid APIs only after you approve an estimate.
            </p>
            {error && <Notice error>{error}</Notice>}
            <div className="overlay-footer">
              <button
                className="btn quiet"
                type="button"
                disabled={busy}
                onClick={() => {
                  setKeyOpen(false);
                  setApiKey("");
                }}
              >
                Cancel
              </button>
              <button
                className="btn"
                type="submit"
                disabled={!apiKey.trim() || busy}
              >
                {busy ? "Connecting…" : "Connect"}
              </button>
            </div>
          </form>
        </Overlay>
      )}
      {info && (
        <Overlay
          title={info === "help" ? "About this release" : "Licenses"}
          onClose={() => setInfo(undefined)}
        >
          {info === "help" ? (
            <div className="help-copy">
              <p>
                Create a video from a topic, brief, or script, review the cost,
                then approve generation. Activity keeps track of the work if you
                close the page.
              </p>
              <p>
                Explore existing videos, inspect scenes, and download completed
                videos. Scene changes, new export presets, uploads, and version
                history are still being built.
              </p>
              <p>
                Interrupted work never retries automatically. Check the original
                output with the native CLI before requesting paid work again.
              </p>
              <a
                className="btn quiet"
                href="https://github.com/spunkytensor/reel-maestro"
                target="_blank"
                rel="noreferrer"
              >
                Project documentation
              </a>
            </div>
          ) : (
            <div className="help-copy">
              <p>
                Reel Maestro and Studio: Apache License 2.0, copyright 2026
                Spunky Tensor.
              </p>
              <p>
                React, Vite, and Fastify: MIT License. Inter and JetBrains Mono:
                SIL Open Font License 1.1.
              </p>
              <p>
                Design tokens and the compact reel mark are adapted from this
                repository’s approved Studio mockups. No mascot or watermark
                asset is bundled.
              </p>
              <a
                className="btn quiet"
                href="/licenses/Inter.txt"
                target="_blank"
                rel="noreferrer"
              >
                Inter license
              </a>
              <a
                className="btn quiet"
                href="/licenses/JetBrainsMono.txt"
                target="_blank"
                rel="noreferrer"
              >
                JetBrains Mono license
              </a>
            </div>
          )}
        </Overlay>
      )}
    </main>
  );
}
