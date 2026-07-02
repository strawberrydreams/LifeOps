import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  build: { outDir: "dist", assetsDir: "assets" },
  server: {
    port: 5173,
    proxy: { "/api": "http://localhost:3000" },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
  },
});
