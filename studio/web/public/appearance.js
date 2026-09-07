// Apply the persisted appearance before styles load; no inline script is needed under CSP.
try {
  const p = JSON.parse(localStorage.getItem("reelmaestro.appearance") || "{}");
  document.documentElement.dataset.theme =
    p.theme === "dark" ||
    (p.theme !== "light" && matchMedia("(prefers-color-scheme: dark)").matches)
      ? "dark"
      : "light";
  document.documentElement.dataset.solid = matchMedia(
    "(prefers-reduced-transparency: reduce)",
  ).matches
    ? "1"
    : "0";
  document.documentElement.dataset.motion = matchMedia(
    "(prefers-reduced-motion: reduce)",
  ).matches
    ? "reduce"
    : "normal";
} catch {
  /* Unavailable storage falls back to the system theme in CSS. */
}
