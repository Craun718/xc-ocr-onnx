import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";

export default defineConfig({
  plugins: [pluginReact()],
  html: {
    template: "./index.html",
  },
  source: {
    entry: {
      index: "./src/main.tsx",
    },
  },
  tools: {
    rspack: {
      watchOptions: {
        ignored: ["**/target/**"],
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
