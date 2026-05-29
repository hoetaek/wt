import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const studioApiTarget = process.env.WT_STUDIO_API_TARGET || "http://127.0.0.1:8424";

export default defineConfig({
  plugins: [tailwindcss()],
  resolve: {
    alias: {
      react: "preact/compat",
      "react-dom": "preact/compat"
    }
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": {
        target: studioApiTarget,
        changeOrigin: false
      },
      "/auth": {
        target: studioApiTarget,
        changeOrigin: false
      },
      "/favicon.ico": {
        target: studioApiTarget,
        changeOrigin: false
      }
    }
  },
  build: {
    emptyOutDir: true
  }
});
