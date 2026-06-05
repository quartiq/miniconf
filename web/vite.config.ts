import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

export default defineConfig(({ command }) => ({
  base: process.env.BASE_PATH ?? "/",
  build: {
    modulePreload: { polyfill: false },
    target: "baseline-widely-available",
  },
  server: { allowedHosts: true },
  plugins: [svelte(), ...(command === "build" ? [viteSingleFile()] : [])],
}));
