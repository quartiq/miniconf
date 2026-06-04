import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  base: process.env.BASE_PATH ?? "/",
  build: {
    modulePreload: { polyfill: false },
    target: "baseline-widely-available",
  },
  server: { allowedHosts: true },
  plugins: [svelte()],
});
