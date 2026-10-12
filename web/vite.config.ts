import { dirname, resolve } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import { defineConfig, loadEnv, type Plugin, type ServerOptions } from "vite";
import react from "@vitejs/plugin-react";

const rootDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function compressedAssets(): Plugin {
  return {
    name: "compressed-assets",
    apply: "build",
    writeBundle(options, bundle) {
      for (const entry of Object.values(bundle)) {
        if (!/\.(js|css)$/.test(entry.fileName)) continue;
        // Vite rewrites dynamic import/preload maps after generateBundle hooks.
        // Compress the final files so every encoding has identical contents.
        const file = resolve(options.dir || resolve(rootDir, "web/dist"), entry.fileName);
        const bytes = readFileSync(file);
        if (bytes.length < 1024) continue;
        writeFileSync(`${file}.gz`, gzipSync(bytes, { level: 9 }));
        writeFileSync(`${file}.br`, brotliCompressSync(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }));
      }
    },
  };
}

function modernFontSources(): Plugin {
  return {
    name: "huninn-woff2",
    enforce: "pre",
    transform(code, id) {
      if (!id.includes("/@fontsource/huninn/") || !id.endsWith(".css")) return;
      // All supported browsers use WOFF2. Keep every unicode subset and glyph,
      // without shipping a second unused copy of each font in the build.
      return code.replace(/,\s*url\([^)]*\.woff\)\s*format\(['"]woff['"]\)/g, "");
    },
  };
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, rootDir, "");
  // Prefer .env / loadEnv over stale shell process.env (ai often pins LAZYBOY_VERSION).
  const version = (env.LAZYBOY_VERSION || process.env.LAZYBOY_VERSION || "0.1.0").trim() || "0.1.0";
  const build = (env.LAZYBOY_BUILD || process.env.LAZYBOY_BUILD || "1").trim() || "1";
  // The API accepts HTTP even when its optional TLS listener is enabled.
  const backendPort = process.env.LAZYHERMES_PORT || env.LAZYHERMES_PORT || "8790";
  const backend = `http://127.0.0.1:${backendPort}`;
  const server: ServerOptions = {
    port: 5174,
    host: "0.0.0.0",
    strictPort: true,
    proxy: {
      "/api": { target: backend, secure: false },
      "/novnc": { target: backend, ws: true, secure: false },
      "/lazyboy-ca.crt": { target: backend, secure: false },
    },
  };
  return {
    plugins: [modernFontSources(), react(), compressedAssets()],
    resolve: { dedupe: ["react", "react-dom"] },
    define: {
      "import.meta.env.LAZYBOY_VERSION": JSON.stringify(version),
      "import.meta.env.LAZYBOY_BUILD": JSON.stringify(build),
    },
    server,
    preview: {
      port: 4174,
      host: "0.0.0.0",
      strictPort: true,
      proxy: server.proxy,
    },
  };
});
