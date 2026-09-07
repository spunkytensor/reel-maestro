import { useState } from "react";
import type { RunSummary } from "../../shared";
import { Icon, Notice, Segmented } from "./components";

export function Projects({
  runs,
  onRescan,
  scanning,
}: {
  runs: RunSummary[];
  onRescan: () => void;
  scanning: boolean;
}) {
  const [search, setSearch] = useState("");
  const [format, setFormat] = useState("all");
  const filtered = runs.filter(
    (run) =>
      (format === "all" || format === run.format) &&
      run.title.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()),
  );
  return (
    <main id="main" className="page projects-page">
      <div className="row between page-heading">
        <div>
          <h1 className="title">Your videos</h1>
          <p className="caption">
            {runs.length} {runs.length === 1 ? "video" : "videos"} · saved on
            this computer
          </p>
        </div>
        <button className="btn quiet" disabled={scanning} onClick={onRescan}>
          {scanning ? "Rescanning…" : "Rescan"}
        </button>
      </div>
      <div className="row between library-tools">
        <Segmented
          label="Filter by format"
          value={format}
          options={[
            { value: "all", label: "All videos" },
            { value: "youtube", label: "Widescreen" },
            { value: "reel", label: "Vertical" },
          ]}
          onChange={setFormat}
        />
        <label className="search field">
          <Icon name="search" />
          <span className="sr-only">Search videos</span>
          <input
            type="search"
            placeholder="Search videos"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>
      </div>
      {!runs.length ? (
        <section className="glass empty-state">
          <Icon name="film" />
          <h2 className="section">Your first video starts with an idea</h2>
          <p className="ink-2">
            Create a video, or add earlier Reel Maestro videos to your saved
            location and rescan.
          </p>
          <a className="btn quiet" href="#new">
            Describe a video
          </a>
        </section>
      ) : !filtered.length ? (
        <Notice>
          No videos match your search. Try another title or format.
        </Notice>
      ) : (
        <div className="project-grid">
          {filtered.map((run) => (
            <a className="project" href={`#video/${run.id}`} key={run.id}>
              <div
                className={`tile ${run.format === "reel" ? "vertical" : ""}`}
              >
                {run.posterUrl ? (
                  <>
                    <img
                      className="poster-backdrop"
                      src={run.posterUrl}
                      alt=""
                      aria-hidden="true"
                    />
                    <img
                      className="poster"
                      src={run.posterUrl}
                      alt=""
                      loading="lazy"
                    />
                  </>
                ) : (
                  <div className="placeholder">
                    <Icon name="film" />
                  </div>
                )}
                {run.status !== "complete" && (
                  <span className="pill warn">
                    <span className="dot" />
                    {run.status === "damaged"
                      ? "Needs attention"
                      : "Not finished"}
                  </span>
                )}
              </div>
              <h2 className="label trunc">{run.title}</h2>
              <p className="caption">
                {run.format === "youtube" ? "Widescreen" : "Vertical"} ·{" "}
                {run.sceneCount} scenes ·{" "}
                {new Date(run.updatedAt).toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                })}
              </p>
            </a>
          ))}
        </div>
      )}
    </main>
  );
}
