import { useCallback, useEffect, useRef, useState } from "react";
import type {
  Job,
  RunDetail,
  RunSummary,
  Settings as SettingsData,
} from "../../shared";
import { Activity, inProgress, jobLabels } from "./Activity";
import { api, errorMessage } from "./api";
import { Brand, Icon, Notice } from "./components";
import { Editor } from "./Editor";
import { useAppearance } from "./preferences";
import { Projects } from "./Projects";
import { Settings } from "./Settings";
import { Start } from "./Start";

export function App() {
  const [route, setRoute] = useState(location.hash.slice(1));
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [settings, setSettings] = useState<SettingsData>();
  const [detail, setDetail] = useState<RunDetail>();
  const [error, setError] = useState("");
  const [detailError, setDetailError] = useState("");
  const [ready, setReady] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [activity, setActivity] = useState(false);
  const [appearance, setAppearance] = useAppearance();
  const newJob = useRef<string | undefined>(undefined);
  const scanActive = useRef(false);

  const refreshJobs = useCallback(async () => {
    const result = await api<{ jobs: Job[] }>("/jobs");
    setJobs(result.jobs);
  }, []);
  const refresh = useCallback(async () => {
    if (scanActive.current) return;
    scanActive.current = true;
    setScanning(true);
    try {
      const [library, config, activity] = await Promise.all([
        api<{ runs: RunSummary[] }>("/runs"),
        api<SettingsData>("/settings"),
        api<{ jobs: Job[] }>("/jobs"),
      ]);
      setRuns(library.runs);
      setSettings(config);
      setJobs(activity.jobs);
      setError("");
      setReady(true);
    } finally {
      scanActive.current = false;
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    void refresh().catch((error) => setError(errorMessage(error)));
  }, [refresh]);
  useEffect(() => {
    const navigate = () => {
      setRoute(location.hash.slice(1));
      setActivity(false);
      window.scrollTo(0, 0);
    };
    window.addEventListener("hashchange", navigate);
    return () => window.removeEventListener("hashchange", navigate);
  }, []);
  useEffect(() => {
    if (!ready || route) return;
    location.hash = runs.length ? "projects" : "new";
  }, [ready, route, runs.length]);
  const videoId = route.startsWith("video/") ? route.slice(6) : undefined;
  useEffect(() => {
    let cancelled = false;
    setDetail(undefined);
    setDetailError("");
    if (videoId && ready)
      void api<RunDetail>(`/runs/${encodeURIComponent(videoId)}`)
        .then((run) => {
          if (!cancelled) setDetail(run);
        })
        .catch((error) => {
          if (!cancelled) setDetailError(errorMessage(error));
        });
    return () => {
      cancelled = true;
    };
  }, [videoId, ready]);

  const activeIds = jobs
    .filter(inProgress)
    .map((job) => job.id)
    .sort()
    .join(",");
  useEffect(() => {
    if (!ready) return;
    const streams = activeIds
      ? activeIds.split(",").map((id) => {
          const stream = new EventSource(
            `/api/jobs/${encodeURIComponent(id)}/events`,
          );
          stream.addEventListener("job", () => {
            void refreshJobs().catch((error) => setError(errorMessage(error)));
          });
          return stream;
        })
      : [];
    // Refresh discovers work submitted in another tab; SSE carries active-job transitions.
    const timer = setInterval(() => {
      if (!document.hidden)
        void refreshJobs().catch((error) => setError(errorMessage(error)));
    }, 15000);
    return () => {
      streams.forEach((stream) => stream.close());
      clearInterval(timer);
    };
  }, [ready, activeIds, refreshJobs]);
  useEffect(() => {
    const job = jobs.find((job) => job.id === newJob.current);
    if (job?.status === "succeeded") {
      newJob.current = undefined;
      void refresh().catch((error) => setError(errorMessage(error)));
      if (job.runId && route === "new") location.hash = `video/${job.runId}`;
    }
  }, [jobs, route, refresh]);
  useEffect(() => {
    document.title = `${detail?.title ?? (route === "settings" ? "Settings" : route === "projects" ? "Your videos" : "Create a video")} — Reel Maestro Studio`;
  }, [detail?.title, route]);

  const activeCount = jobs.filter(inProgress).length;
  return (
    <>
      <a
        className="skip-link btn"
        href="#main"
        onClick={(event) => {
          event.preventDefault();
          const main = document.getElementById("main");
          main?.setAttribute("tabindex", "-1");
          main?.focus();
        }}
      >
        Skip to content
      </a>
      <header className="bar glass">
        {videoId ? (
          <>
            <a
              href="#projects"
              className="btn quiet icon"
              aria-label="Back to projects"
            >
              <Icon name="back" />
            </a>
            <div className="editor-title grow">
              <h1 className="label trunc">
                {detail?.title ?? "Opening video…"}
              </h1>
              <p className="caption">
                {detail?.parentRunId ? "Completed version" : "Original"} ·
                preserved
              </p>
            </div>
          </>
        ) : (
          <Brand />
        )}
        <span className="spacer" />
        <nav className="nav row" aria-label="Main navigation">
          {!videoId && (
            <a
              href="#projects"
              className={route === "projects" ? "on" : ""}
              aria-current={route === "projects" ? "page" : undefined}
            >
              Projects
            </a>
          )}
          {!videoId && (
            <a
              href="#settings"
              className={route === "settings" ? "on" : ""}
              aria-current={route === "settings" ? "page" : undefined}
            >
              Settings
            </a>
          )}
          <button
            className="btn quiet activity-button"
            aria-expanded={activity}
            onClick={() => setActivity(!activity)}
          >
            {activeCount ? `${activeCount} in progress` : "Activity"}
          </button>
        </nav>
        {route === "projects" && (
          <a className="btn primary mobile-primary" href="#new">
            New video
          </a>
        )}
        {videoId && <div id="editor-actions" className="row editor-actions" />}
      </header>
      {error && (
        <div className="global-error">
          <Notice error>
            {error}{" "}
            <button
              className="btn quiet"
              onClick={() =>
                void refresh().catch((error) => setError(errorMessage(error)))
              }
            >
              Reconnect
            </button>
          </Notice>
        </div>
      )}
      {!ready ? (
        <main id="main" className="page">
          <Notice>
            {error ? "Studio is not connected." : "Opening Studio…"}
          </Notice>
        </main>
      ) : videoId ? (
        detail ? (
          <Editor
            key={detail.id}
            run={detail}
            jobs={jobs}
            onJob={(job) => {
              setJobs((previous) => [
                job,
                ...previous.filter((old) => old.id !== job.id),
              ]);
              void refreshJobs().catch((error) =>
                setError(errorMessage(error)),
              );
            }}
          />
        ) : (
          <main id="main" className="page">
            <Notice error={!!detailError}>
              {detailError || "Opening your video…"}
            </Notice>
            <a className="btn quiet" href="#projects">
              Back to projects
            </a>
          </main>
        )
      ) : route === "settings" ? (
        <Settings
          settings={settings}
          appearance={appearance}
          onAppearance={setAppearance}
          onRefresh={refresh}
          scanning={scanning}
        />
      ) : route === "projects" ? (
        <Projects
          runs={runs}
          scanning={scanning}
          onRescan={() =>
            void refresh().catch((error) => setError(errorMessage(error)))
          }
        />
      ) : (
        <Start
          settings={settings}
          onJob={(job) => {
            newJob.current = job.id;
            setJobs((previous) => [
              job,
              ...previous.filter((old) => old.id !== job.id),
            ]);
            setActivity(true);
          }}
        />
      )}
      {activity && (
        <Activity
          jobs={jobs}
          onClose={() => setActivity(false)}
          onRefresh={refreshJobs}
        />
      )}
      <img
        className="corner-logo"
        src="/logo.png"
        alt="Spunky Tensor"
        width="44"
        height="44"
      />
      <div className="sr-only" role="status" aria-live="polite">
        {jobs
          .filter((job) => job.id === newJob.current)
          .map((job) => jobLabels[job.status])
          .join(". ")}
      </div>
    </>
  );
}
