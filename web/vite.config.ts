import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig, loadEnv } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

export default defineConfig(({ command, mode }) => ({
  define: {
    __BUILD_COMMIT__: JSON.stringify(
      loadEnv(mode, ".", "").MINICONF_WEB_BUILD_COMMIT ||
        process.env.GITHUB_SHA ||
        "local",
    ),
  },
  base: process.env.BASE_PATH ?? "/",
  build: {
    modulePreload: { polyfill: false },
    target: "baseline-widely-available",
  },
  plugins: [svelte(), ...(command === "build" ? [viteSingleFile()] : [])],
}));
