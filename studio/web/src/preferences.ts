import { useEffect, useState } from "react";
import type { PlanInput } from "../../shared";

export type Appearance = {
  theme: "system" | "light" | "dark";
};
export type Defaults = Pick<PlanInput, "format" | "quality" | "maxCost">;

function stored(key: string): Record<string, unknown> {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
    return data && typeof data === "object" && !Array.isArray(data)
      ? (data as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

export function readDefaults(): Defaults {
  const value = stored("reelmaestro.defaults");
  return {
    format: value.format === "youtube" ? "youtube" : "reel",
    quality:
      value.quality === "draft" || value.quality === "premium"
        ? value.quality
        : "standard",
    maxCost:
      typeof value.maxCost === "number" &&
      Number.isFinite(value.maxCost) &&
      value.maxCost >= 0 &&
      value.maxCost <= 1000
        ? value.maxCost
        : 5,
  };
}

export function persist(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export function useAppearance() {
  const [appearance, setAppearance] = useState<Appearance>(() => {
    const value = stored("reelmaestro.appearance");
    return {
      theme:
        value.theme === "light" || value.theme === "dark"
          ? value.theme
          : "system",
    };
  });
  useEffect(() => {
    const dark = matchMedia("(prefers-color-scheme: dark)");
    const solid = matchMedia("(prefers-reduced-transparency: reduce)");
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const apply = () => {
      document.documentElement.dataset.theme =
        appearance.theme === "system"
          ? dark.matches
            ? "dark"
            : "light"
          : appearance.theme;
      document.documentElement.dataset.solid = solid.matches ? "1" : "0";
      document.documentElement.dataset.motion = motion.matches
        ? "reduce"
        : "normal";
    };
    apply();
    for (const query of [dark, solid, motion])
      query.addEventListener("change", apply);
    return () => {
      for (const query of [dark, solid, motion])
        query.removeEventListener("change", apply);
    };
  }, [appearance]);
  return [
    appearance,
    (next: Appearance) => {
      setAppearance(next);
      return persist("reelmaestro.appearance", next);
    },
  ] as const;
}
