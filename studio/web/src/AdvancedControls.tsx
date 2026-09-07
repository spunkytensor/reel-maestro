import type { AdvancedOptions } from "../../shared";
import { Item } from "./components";
import { UploadField } from "./UploadField";

const modelFields = [
  ["textModel", "Writing model"],
  ["imageModel", "Image model"],
  ["judgeModel", "Image review model"],
  ["ttsModel", "Narration model"],
  ["musicModel", "Music model"],
  ["videoModel", "Video model"],
] as const;
const numberFields = [
  ["sceneSeconds", "Silent scene length (seconds)", 0.1, 120, 0.1],
  ["videoSeed", "Video seed", 0, Number.MAX_SAFE_INTEGER, 1],
  ["videoSteps", "Video sampling steps", 1, 1000, 1],
  ["videoWaitTimeout", "Video wait limit (minutes)", 1, 1440, 1],
  ["videoScenes", "Animate first scenes (new videos only)", 0, 500, 1],
  ["musicVolume", "Music volume", 0, 2, 0.05],
  ["dissolveSeconds", "Transition length (seconds)", 0, 10, 0.05],
  ["posterScene", "Cover fallback scene (zero-based)", 0, 500, 1],
] as const;

export function AdvancedControls({
  value = {},
  onChange,
  onBusyChange,
  revision = false,
}: {
  value?: AdvancedOptions;
  onChange: (value: AdvancedOptions) => void;
  onBusyChange?: (busy: boolean) => void;
  revision?: boolean;
}) {
  function change<K extends keyof AdvancedOptions>(
    key: K,
    next: AdvancedOptions[K],
  ) {
    const result = { ...value, [key]: next };
    if (next === undefined) delete result[key];
    onChange(result);
  }
  return (
    <div className="advanced-controls">
      <p className="caption">
        Blank values use this computer’s configuration or the quality preset.
        Explicit choices appear in the cost review before work starts.
      </p>
      <details>
        <summary className="label">Models</summary>
        <div className="list">
          {modelFields.map(([key, label]) => (
            <label className="advanced-field label" key={key}>
              {label}
              <input
                className="field"
                maxLength={200}
                placeholder="Preset default"
                value={value[key] ?? ""}
                onChange={(event) =>
                  change(key, event.target.value || undefined)
                }
              />
            </label>
          ))}
          <Item
            label="Video service"
            caption="Local video uses this computer’s operator-configured endpoint."
          >
            <select
              className="field"
              aria-label="Video service"
              value={value.videoProvider ?? ""}
              onChange={(event) => {
                const next = event.target.value;
                if (next === "openrouter" || next === "local")
                  change("videoProvider", next);
                else change("videoProvider", undefined);
              }}
            >
              <option value="">Computer default</option>
              <option value="openrouter">OpenRouter</option>
              <option value="local">Local video</option>
            </select>
          </Item>
        </div>
      </details>
      <details>
        <summary className="label">Images and motion</summary>
        <div className="list">
          <Item label="Image candidates per scene">
            <select
              className="field"
              aria-label="Image candidates per scene"
              value={value.validateScene ?? ""}
              onChange={(event) =>
                change(
                  "validateScene",
                  event.target.value === ""
                    ? undefined
                    : Number(event.target.value),
                )
              }
            >
              <option value="">Keep default</option>
              <option value="0">One image, no review</option>
              <option value="2">Up to two</option>
              <option value="3">Up to three</option>
            </select>
          </Item>
          <Item label="Consistent characters and locations">
            <BooleanChoice
              label="Consistent characters and locations"
              value={value.consistency}
              onChange={(next) => change("consistency", next)}
            />
          </Item>
          <UploadField
            label="Character reference"
            kind="image"
            value={value.characterReferenceAssetId}
            onBusyChange={onBusyChange}
            onChange={(asset) => change("characterReferenceAssetId", asset?.id)}
          />
          <Item label="Clip resolution">
            <select
              className="field compact"
              aria-label="Clip resolution"
              value={value.videoResolution ?? ""}
              onChange={(event) => {
                const next = event.target.value;
                change(
                  "videoResolution",
                  next === "720p" || next === "1080p" ? next : undefined,
                );
              }}
            >
              <option value="">Default</option>
              <option value="720p">720p</option>
              <option value="1080p">1080p</option>
            </select>
          </Item>
          <label className="advanced-field label">
            Local clip size
            <input
              className="field"
              placeholder="Computer default, e.g. 768x432"
              value={value.videoSize ?? ""}
              maxLength={30}
              onChange={(event) =>
                change("videoSize", event.target.value || undefined)
              }
            />
          </label>
          <Item label="Clip input">
            <select
              className="field"
              aria-label="Clip input"
              value={value.videoInputMode ?? ""}
              onChange={(event) => {
                const next = event.target.value;
                change(
                  "videoInputMode",
                  next === "text" || next === "first-frame" ? next : undefined,
                );
              }}
            >
              <option value="">Computer default</option>
              <option value="text">Motion description</option>
              <option value="first-frame">Scene image</option>
            </select>
          </Item>
        </div>
      </details>
      <details>
        <summary className="label">Timing and finishing</summary>
        <div className="list">
          {numberFields
            .filter(([key]) => !revision || key !== "videoScenes")
            .map(([key, label, min, max, step]) => (
              <Item label={label} key={key}>
                <input
                  className="field compact tc"
                  aria-label={label}
                  type="number"
                  min={min}
                  max={max}
                  step={step}
                  placeholder="Default"
                  value={value[key] ?? ""}
                  onChange={(event) =>
                    change(
                      key,
                      Number.isFinite(event.target.valueAsNumber)
                        ? event.target.valueAsNumber
                        : undefined,
                    )
                  }
                />
              </Item>
            ))}
          {(
            [
              ["dissolve", "Dissolve between scenes"],
              ["grade", "Color finishing"],
              ["loudnorm", "Normalize loudness"],
              ["embedPoster", "Poster as cover"],
            ] as const
          ).map(([key, label]) => (
            <Item label={label} key={key}>
              <BooleanChoice
                label={label}
                value={value[key]}
                onChange={(next) => change(key, next)}
              />
            </Item>
          ))}
          <Item label="Music under narration">
            <select
              className="field"
              aria-label="Music under narration"
              value={value.mix === undefined ? "" : String(value.mix)}
              onChange={(event) =>
                change(
                  "mix",
                  event.target.value === ""
                    ? undefined
                    : event.target.value === "true",
                )
              }
            >
              <option value="">Keep default</option>
              <option value="true">Ducks automatically</option>
              <option value="false">Constant volume</option>
            </select>
          </Item>
          <label className="advanced-field label">
            Caption font
            <input
              className="field"
              placeholder="Computer default"
              value={value.captionFont ?? ""}
              maxLength={100}
              onChange={(event) =>
                change("captionFont", event.target.value || undefined)
              }
            />
          </label>
          <label className="advanced-field label">
            Whisper model
            <input
              className="field"
              placeholder="Installed default"
              value={value.whisperModel ?? ""}
              maxLength={100}
              onChange={(event) =>
                change("whisperModel", event.target.value || undefined)
              }
            />
          </label>
          <UploadField
            label="Watermark (optional)"
            kind="image"
            value={value.watermarkAssetId}
            onBusyChange={onBusyChange}
            onChange={(asset) => change("watermarkAssetId", asset?.id)}
          />
        </div>
      </details>
      <details>
        <summary className="label">Diagnostics</summary>
        {!revision && (
          <>
            <p className="caption">
              Timing-only stops before images and does not produce a playable
              video. Use this only to inspect narration timing.
            </p>
            <Item label="Timing-only diagnostic">
              <BooleanChoice
                label="Timing-only diagnostic"
                value={value.noImages}
                onChange={(next) => change("noImages", next)}
              />
            </Item>
          </>
        )}
        <Item label="Detailed diagnostics">
          <BooleanChoice
            label="Detailed diagnostics"
            value={value.verbose}
            onChange={(next) => change("verbose", next)}
          />
        </Item>
        <Item label="Timing executable">
          <select
            className="field"
            aria-label="Timing executable"
            value={value.whisperExecutable ?? ""}
            onChange={(event) =>
              change(
                "whisperExecutable",
                event.target.value === "whisper_timestamped"
                  ? "whisper_timestamped"
                  : undefined,
              )
            }
          >
            <option value="">Computer default</option>
            <option value="whisper_timestamped">
              Installed Whisper Timestamped
            </option>
          </select>
        </Item>
      </details>
      <button type="button" className="btn quiet" onClick={() => onChange({})}>
        Use computer defaults
      </button>
    </div>
  );
}

function BooleanChoice({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean | undefined;
  onChange: (value: boolean | undefined) => void;
}) {
  return (
    <select
      className="field compact"
      aria-label={label}
      value={value === undefined ? "" : String(value)}
      onChange={(event) =>
        onChange(
          event.target.value === "" ? undefined : event.target.value === "true",
        )
      }
    >
      <option value="">Keep default</option>
      <option value="true">On</option>
      <option value="false">Off</option>
    </select>
  );
}
