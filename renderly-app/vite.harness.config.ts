// Browser dev harness config: renders the real app in a plain browser by swapping the
// four @tauri-apps modules for src/dev/tauriMock.ts. Run with:
//   npx vite --config vite.harness.config.ts --port 5188
// Never used by the production build (npm run build uses vite.config.ts).
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

const mock = fileURLToPath(new URL("./src/dev/tauriMock.ts", import.meta.url));

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: [
      { find: "@tauri-apps/api/core", replacement: mock },
      { find: "@tauri-apps/api/event", replacement: mock },
      { find: "@tauri-apps/api/window", replacement: mock },
      { find: "@tauri-apps/plugin-dialog", replacement: mock },
    ],
  },
  server: { port: 5188, strictPort: true },
});
