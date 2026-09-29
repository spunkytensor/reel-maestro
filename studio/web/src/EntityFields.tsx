import type { Entity } from "../../shared";
import { approvalKey } from "./api";

export function EntityFields({
  kind,
  value,
  onChange,
}: {
  kind: "character" | "location";
  value: Entity[];
  onChange: (value: Entity[]) => void;
}) {
  return (
    <details className="chapter-editor">
      <summary className="label">
        {kind === "character" ? "Characters" : "Locations"}
      </summary>
      <p className="caption">
        Changes update only the images and clips that use this description,
        unless you explicitly keep their existing media.
      </p>
      {value.map((entity, index) => (
        <div key={entity.id} className="advanced-field">
          <label className="label">
            {kind === "character" ? "Character" : "Location"} {index + 1}
            <textarea
              className="field"
              value={entity.description}
              onChange={(event) =>
                onChange(
                  value.map((candidate) =>
                    candidate.id === entity.id
                      ? { ...candidate, description: event.target.value }
                      : candidate,
                  ),
                )
              }
            />
          </label>
          <button
            type="button"
            className="btn quiet"
            onClick={() =>
              onChange(value.filter((candidate) => candidate.id !== entity.id))
            }
          >
            Remove {kind} {index + 1}
          </button>
        </div>
      ))}
      <button
        type="button"
        className="btn quiet"
        onClick={() =>
          onChange([
            ...value,
            { id: `${kind}-${approvalKey().slice(0, 16)}`, description: "" },
          ])
        }
      >
        Add {kind}
      </button>
    </details>
  );
}
