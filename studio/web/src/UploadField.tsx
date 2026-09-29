import { useEffect, useRef, useState } from "react";
import type { UploadAsset } from "../../shared";
import { api, errorMessage } from "./api";
import { Notice } from "./components";

export function UploadField({
  label,
  kind,
  value,
  onChange,
  disabled = false,
  onBusyChange,
}: {
  label: string;
  kind: UploadAsset["kind"];
  value?: string;
  onChange: (asset: UploadAsset | undefined) => void;
  disabled?: boolean;
  onBusyChange?: (busy: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const mounted = useRef(true);
  const busyCallback = useRef(onBusyChange);
  busyCallback.current = onBusyChange;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      busyCallback.current?.(false);
    };
  }, []);
  async function upload(file: File) {
    setError("");
    const maximum = kind === "text" ? 400_000 : 20_000_000;
    if (!file.size || file.size > maximum) {
      setError(
        `Choose a non-empty file smaller than ${maximum / 1_000_000} MB.`,
      );
      return;
    }
    setBusy(true);
    busyCallback.current?.(true);
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onerror = () =>
          reject(new Error("This file could not be read."));
        reader.onload = () => {
          if (typeof reader.result !== "string")
            reject(new Error("Invalid file."));
          else resolve(reader.result.slice(reader.result.indexOf(",") + 1));
        };
        reader.readAsDataURL(file);
      });
      const asset = await api<UploadAsset>("/uploads", {
        name: file.name,
        kind,
        data,
      });
      if (!mounted.current) return;
      setName(asset.name);
      onChange(asset);
    } catch (error) {
      if (mounted.current) setError(errorMessage(error));
    } finally {
      if (mounted.current) {
        setBusy(false);
        busyCallback.current?.(false);
      }
      if (input.current) input.current.value = "";
    }
  }
  return (
    <div className="upload-field">
      <label className="label">
        {label}
        <input
          ref={input}
          className="field"
          type="file"
          accept={
            kind === "text"
              ? ".txt,.md,text/plain,text/markdown"
              : kind === "image"
                ? "image/png,image/jpeg"
                : "audio/mpeg,audio/wav,audio/mp4,audio/x-m4a"
          }
          disabled={disabled || busy}
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void upload(file);
          }}
        />
      </label>
      <p className="caption" aria-live="polite">
        {busy
          ? "Checking and uploading…"
          : value
            ? name || "Your uploaded file is selected."
            : "Only files you have the rights to use."}
      </p>
      {value && (
        <button
          type="button"
          className="btn quiet"
          disabled={disabled || busy}
          onClick={() => {
            onChange(undefined);
            setName("");
          }}
        >
          Remove selection
        </button>
      )}
      {error && <Notice error>{error}</Notice>}
    </div>
  );
}
