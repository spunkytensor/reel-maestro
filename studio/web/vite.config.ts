import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [react()],
  // Preserve both backdrop-filter overrides; Lightning CSS drops the mobile reset.
  build: { cssMinify: "esbuild" },
});
