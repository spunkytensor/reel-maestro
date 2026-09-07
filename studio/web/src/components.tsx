import { useEffect, useId, useRef, type ReactNode } from "react";

export function Brand() {
  return (
    <a className="brand" href="#new" aria-label="Reel Maestro home">
      <svg className="mark" viewBox="0 0 28 28" aria-hidden="true">
        <circle
          cx="14"
          cy="14"
          r="9"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
        />
        {[
          [14, 14, 2],
          [14, 8, 1.5],
          [14, 20, 1.5],
          [8, 14, 1.5],
          [20, 14, 1.5],
        ].map(([cx, cy, r]) => (
          <circle
            key={`${cx}-${cy}`}
            cx={cx}
            cy={cy}
            r={r}
            fill="currentColor"
          />
        ))}
      </svg>
      <span className="name">Reel Maestro</span>
      <span className="by">by Spunky Tensor</span>
    </a>
  );
}

type IconName =
  | "play"
  | "pause"
  | "back"
  | "close"
  | "film"
  | "download"
  | "volume"
  | "muted"
  | "full"
  | "search";
const paths: Record<IconName, ReactNode> = {
  play: <path d="M8 5v14l11-7z" />,
  pause: <path d="M8 5v14M16 5v14" />,
  back: <path d="m14 6-6 6 6 6" />,
  close: <path d="m6 6 12 12M18 6 6 18" />,
  film: (
    <>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="M8 4v16M16 4v16M4 9h4M4 15h4M16 9h4M16 15h4" />
    </>
  ),
  download: <path d="M12 4v11m-4-4 4 4 4-4M5 20h14" />,
  volume: <path d="M4 10v4h4l5 4V6L8 10zM17 8a6 6 0 0 1 0 8" />,
  muted: <path d="M4 10v4h4l5 4V6L8 10zM17 10l4 4m0-4-4 4" />,
  full: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />,
  search: (
    <>
      <circle cx="10" cy="10" r="6" />
      <path d="m15 15 5 5" />
    </>
  ),
};
export function Icon({ name }: { name: IconName }) {
  return (
    <svg className="i" viewBox="0 0 24 24" aria-hidden="true">
      {paths[name]}
    </svg>
  );
}

export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  className = "",
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  className?: string;
}) {
  return (
    <div className={`seg ${className}`} role="radiogroup" aria-label={label}>
      {options.map((option, index) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          tabIndex={value === option.value ? 0 : -1}
          className={value === option.value ? "on" : ""}
          onClick={() => onChange(option.value)}
          onKeyDown={(event) => {
            const delta = ["ArrowRight", "ArrowDown"].includes(event.key)
              ? 1
              : ["ArrowLeft", "ArrowUp"].includes(event.key)
                ? -1
                : 0;
            if (!delta && event.key !== "Home" && event.key !== "End") return;
            event.preventDefault();
            const next =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? options.length - 1
                  : (index + delta + options.length) % options.length;
            onChange(options[next].value);
            event.currentTarget.parentElement
              ?.querySelectorAll("button")
              [next]?.focus();
          }}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      className="switch-target"
      role="switch"
      aria-label={label}
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span className={`toggle ${checked ? "on" : ""}`} />
    </button>
  );
}

export function Item({
  label,
  caption,
  children,
}: {
  label: string;
  caption?: string;
  children?: ReactNode;
}) {
  return (
    <div className="item">
      <div className="item-copy">
        <div className="l">{label}</div>
        {caption && <div className="c">{caption}</div>}
      </div>
      {children}
    </div>
  );
}

export function Notice({
  children,
  error = false,
}: {
  children: ReactNode;
  error?: boolean;
}) {
  return (
    <div className="notice" role={error ? "alert" : "status"}>
      {children}
    </div>
  );
}

// Modeless dialogs keep the video visible and usable. Escape returns to the opener.
export function Overlay({
  title,
  kind = "popover",
  children,
  onClose,
}: {
  title: string;
  kind?: "popover" | "sheet" | "success";
  children: ReactNode;
  onClose: () => void;
}) {
  const id = useId();
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const opener =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
      }
    };
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("keydown", key, true);
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  return (
    <div
      ref={ref}
      role="dialog"
      aria-labelledby={id}
      className={`glass elevated overlay ${kind}`}
    >
      <div className="row between overlay-header">
        <h2 id={id} className="section">
          {title}
        </h2>
        <button
          className="btn quiet icon"
          aria-label={`Close ${title.toLowerCase()}`}
          onClick={onClose}
        >
          <Icon name="close" />
        </button>
      </div>
      {children}
    </div>
  );
}

export const formats = [
  { value: "youtube", label: "Widescreen" },
  { value: "reel", label: "Vertical" },
] as const;
export const qualities = [
  { value: "draft", label: "Quick draft" },
  { value: "standard", label: "Balanced" },
  { value: "premium", label: "Best quality" },
] as const;
export const captionStyles = [
  { value: "burst", label: "Burst" },
  { value: "karaoke", label: "Karaoke" },
  { value: "boxed", label: "Boxed" },
  { value: "minimal", label: "Minimal" },
] as const;
export function timecode(seconds: number) {
  const value = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(value / 60)}:${String(value % 60).padStart(2, "0")}`;
}
export function money(amount: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 2,
  }).format(amount);
}
