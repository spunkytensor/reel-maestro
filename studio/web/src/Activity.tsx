import { useEffect, useState } from "react";
import type { Job, JobStatus } from "../../shared";
import { api, errorMessage } from "./api";
import { Notice, Overlay, timecode } from "./components";

export const jobLabels: Record<JobStatus, string> = {
  queued: "Waiting",
  running: "Generating",
  succeeded: "Ready",
  failed: "Needs attention",
  interrupted: "Interrupted",
  cancelled: "Stopped",
  cancelling: "Stopping",
};
export function inProgress(job: Job) {
  return ["queued", "running", "cancelling"].includes(job.status);
}

export function jobLabel(job: Job) {
  if (job.status === "running")
    return job.operation === "export"
      ? "Exporting"
      : job.operation === "apply"
        ? "Applying changes"
        : "Generating";
  if (job.status === "succeeded")
    return job.operation === "export"
      ? "Exported"
      : job.operation === "apply"
        ? "Changes applied"
        : "Ready";
  return jobLabels[job.status];
}

const stageLabels: Record<string, string> = {
  script: "Writing your story",
  narration: "Recording narration",
  timing: "Aligning words and scenes",
  images: "Creating scene images",
  video: "Animating scenes",
  music: "Creating the soundtrack",
  assemble: "Putting your video together",
  assembly: "Putting your video together",
  poster: "Creating the cover",
  export: "Encoding your video",
  publish: "Checking and completing your video",
  planning: "Checking requested work",
  prepare: "Preparing a new version",
};

export function JobProgress({ job }: { job: Job }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!inProgress(job)) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [job.status]);
  return (
    <div className="job-progress">
      <p className="label" role="status">
        {jobLabel(job)}
        {job.stage && inProgress(job)
          ? ` · ${stageLabels[job.stage] ?? "Working on your video"}`
          : ""}
      </p>
      {inProgress(job) && (
        <p className="caption">
          Since requested{" "}
          <span className="tc">
            {timecode(Math.max(0, (now - Date.parse(job.createdAt)) / 1000))}
          </span>{" "}
          · You can leave this page; work continues.
        </p>
      )}
    </div>
  );
}

export function Activity({
  jobs,
  onClose,
  onRefresh,
}: {
  jobs: Job[];
  onClose: () => void;
  onRefresh: () => Promise<void>;
}) {
  const [error, setError] = useState("");
  const [cancelling, setCancelling] = useState<string>();
  const [resumeId, setResumeId] = useState<string>();
  const [resuming, setResuming] = useState(false);
  async function cancel(id: string) {
    setCancelling(id);
    setError("");
    try {
      await api(`/jobs/${id}/cancel`, {});
      await onRefresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setCancelling(undefined);
    }
  }
  async function resume(id: string) {
    setResuming(true);
    setError("");
    try {
      await api<Job>(`/jobs/${id}/resume`, {});
      setResumeId(undefined);
      await onRefresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setResuming(false);
    }
  }
  return (
    <Overlay title="Activity" onClose={onClose}>
      {!jobs.length && <p className="ink-2">No work has been requested yet.</p>}
      <div className="activity-list">
        {jobs.map((job) => (
          <article className="activity-job" key={job.id}>
            <div className="row between">
              <h3 className="label">{job.title ?? "Video"}</h3>
              <span
                className={`pill ${job.status === "failed" || job.status === "interrupted" ? "warn" : job.status === "succeeded" ? "ok" : ""}`}
              >
                <span className="dot" />
                {jobLabel(job)}
              </span>
            </div>
            <p className="caption">
              Requested{" "}
              {new Date(job.createdAt).toLocaleString(undefined, {
                dateStyle: "short",
                timeStyle: "short",
              })}
            </p>
            {inProgress(job) && <JobProgress job={job} />}
            {!!job.warnings?.length && (
              <details>
                <summary className="caption">Ready, with a note</summary>
                {job.warnings.map((warning, index) => (
                  <p key={index} className="caption">
                    {warning}
                  </p>
                ))}
              </details>
            )}
            {job.error && <p className="caption">{job.error}</p>}
            {(job.status === "interrupted" || job.status === "failed") && (
              <p className="caption">
                Nothing restarts automatically. Work already accepted by an AI
                service may still complete. Recovery never silently starts a new
                paid request.
              </p>
            )}
            <div className="row">
              {job.status === "interrupted" &&
                job.recoverable &&
                resumeId !== job.id && (
                  <button
                    className="btn quiet"
                    disabled={resuming}
                    onClick={() => setResumeId(job.id)}
                  >
                    Review recovery
                  </button>
                )}
              {job.runId && (
                <a
                  className="btn quiet"
                  href={`#video/${job.runId}${job.operation === "export" ? "?download" : ""}`}
                  onClick={onClose}
                >
                  {job.operation === "export"
                    ? "Download exported video"
                    : "Open video"}
                </a>
              )}
              {!job.runId && (
                <a
                  className="btn quiet"
                  href={
                    job.sourceRunId
                      ? `#video/${job.sourceRunId}`
                      : `#new/${job.id}`
                  }
                  onClick={onClose}
                >
                  View progress
                </a>
              )}
              {["queued", "running"].includes(job.status) && (
                <button
                  className="btn quiet"
                  disabled={!!cancelling}
                  onClick={() => void cancel(job.id)}
                >
                  {cancelling === job.id ? "Stopping…" : "Cancel"}
                </button>
              )}
            </div>
            {resumeId === job.id &&
              job.status === "interrupted" &&
              job.recoverable && (
                <div className="overlay-note">
                  <p className="caption">
                    Resume only the exact work you already approved. Accepted
                    clips are checked again; ambiguous paid requests will not be
                    retried. Source or configuration changes require a new
                    review.
                  </p>
                  <div className="row">
                    <button
                      className="btn quiet"
                      disabled={resuming}
                      onClick={() => setResumeId(undefined)}
                    >
                      Not now
                    </button>
                    <button
                      className="btn primary"
                      disabled={resuming}
                      onClick={() => void resume(job.id)}
                    >
                      {resuming ? "Resuming…" : "Resume approved work"}
                    </button>
                  </div>
                </div>
              )}
            {job.requiresOperatorAction && (
              <p className="caption">
                An operator must check the provider’s job status before further
                work. Automatic recovery is unavailable because it could
                duplicate charges.
              </p>
            )}
          </article>
        ))}
      </div>
      {error && <Notice error>{error}</Notice>}
      <p className="caption overlay-note">
        You can close this and keep exploring. Cancelling stops local work;
        already requested clips or audio may still arrive and incur charges.
      </p>
    </Overlay>
  );
}
