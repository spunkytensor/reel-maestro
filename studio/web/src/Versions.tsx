import { useEffect, useState } from "react";
import type { RunDetail } from "../../shared";
import { api, errorMessage } from "./api";
import { Notice, Overlay } from "./components";

export function Versions({
  run,
  onClose,
}: {
  run: RunDetail;
  onClose: () => void;
}) {
  const [selected, setSelected] = useState("");
  const [comparison, setComparison] = useState<RunDetail>();
  const [error, setError] = useState("");
  useEffect(() => {
    let cancelled = false;
    setComparison(undefined);
    setError("");
    if (selected)
      void api<RunDetail>(`/runs/${encodeURIComponent(selected)}`)
        .then((detail) => {
          if (!cancelled) setComparison(detail);
        })
        .catch((error) => {
          if (!cancelled) setError(errorMessage(error));
        });
    return () => {
      cancelled = true;
    };
  }, [selected]);
  const changes = comparison
    ? run.scenes
        .flatMap((scene, index) => {
          const before = comparison.scenes.find(
            (candidate) => candidate.id === scene.id,
          );
          if (!before) return [`Scene ${index + 1}: added`];
          const fields = [
            before.line !== scene.line ? "words" : "",
            before.imagePrompt !== scene.imagePrompt ? "image description" : "",
            before.motionPrompt !== scene.motionPrompt ? "motion" : "",
            comparison.scenes.indexOf(before) !== index ? "position" : "",
          ].filter(Boolean);
          return fields.length
            ? [`Scene ${index + 1}: changed ${fields.join(", ")}`]
            : [];
        })
        .concat(
          comparison.scenes
            .filter(
              (scene) =>
                !run.scenes.some((candidate) => candidate.id === scene.id),
            )
            .map(
              (scene) =>
                `Removed scene: ${scene.line.slice(0, 80) || "No spoken words"}`,
            ),
        )
    : [];
  return (
    <Overlay title="Versions" kind="sheet" onClose={onClose}>
      <p className="caption">
        Completed versions stay exactly as they are. Unapplied changes stay with
        the video you were editing.
      </p>
      <label className="label">
        Compare with
        <select
          className="field"
          value={selected}
          onChange={(event) => setSelected(event.target.value)}
        >
          <option value="">Choose a version</option>
          {(run.revisions ?? [])
            .filter((version) => version.id !== run.id)
            .map((version, index) => (
              <option key={version.id} value={version.id}>
                {version.title} · {new Date(version.updatedAt).toLocaleString()}{" "}
                · {index + 1}
              </option>
            ))}
        </select>
      </label>
      {!run.revisions?.some((version) => version.id !== run.id) && (
        <p className="caption">
          No other versions yet. Applying changes creates a new one without
          replacing this video.
        </p>
      )}
      {selected && !comparison && !error && (
        <Notice>Opening comparison…</Notice>
      )}
      {error && <Notice error>{error}</Notice>}
      {comparison && (
        <>
          <div className="version-previews">
            {[comparison, run].map((version, index) => (
              <figure key={version.id}>
                {version.videoUrl ? (
                  <video
                    controls
                    playsInline
                    preload="metadata"
                    src={version.videoUrl}
                    poster={version.posterUrl}
                    aria-label={
                      index
                        ? "Current version preview"
                        : "Compared version preview"
                    }
                  />
                ) : (
                  <p className="caption">No video preview</p>
                )}
                <figcaption className="caption">
                  {index ? "Current version" : "Compared version"}
                </figcaption>
              </figure>
            ))}
          </div>
          <h3 className="label">Scene differences</h3>
          {changes.length ? (
            <ul className="version-differences">
              {changes.map((change, index) => (
                <li key={index}>{change}</li>
              ))}
            </ul>
          ) : (
            <p className="caption">
              Scene words and descriptions match. Finishing or generated media
              may differ; compare the videos above.
            </p>
          )}
          <a
            className="btn quiet"
            href={`#video/${comparison.id}`}
            onClick={onClose}
          >
            Open compared version
          </a>
        </>
      )}
    </Overlay>
  );
}
