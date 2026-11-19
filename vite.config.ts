import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

interface PagePackage {
  name: string;
  version: string;
  // Null for a package that is not installed: one a build tool carries inside itself.
  directory: string | null;
}

function readPackage(directory: string): PagePackage {
  const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8")) as { name: string; version: string };
  return { name: manifest.name, version: manifest.version, directory };
}

// Where an installed package lives: resolved from the project or, for the build tools'
// own dependencies such as Rolldown, from Vite.
function installedPackage(name: string): string | null {
  const viteManifest = createRequire(import.meta.url).resolve("vite/package.json");
  for (const from of [import.meta.url, viteManifest]) {
    try {
      return dirname(createRequire(from).resolve(`${name}/package.json`));
    } catch {
      // Not resolvable from here; try the next place.
    }
  }
  return null;
}

// The package a module of the bundle comes from, or null for the client's own code.
//
// Installed code is whatever follows the last `node_modules/` in the path: a name, or a
// scope and a name. The build tools also write code of their own into the bundle, as
// virtual modules: Vite's preload helpers ("\0vite/preload-helper.js") and Rolldown's
// runtime ("\0rolldown/runtime.js") come from those packages, and the Oxc helpers that
// Rolldown carries inside its native binding name their package and version, which is
// not installed ("\0@oxc-project+runtime@0.155.0/helpers/esm/typeof.js").
function modulePackage(id: string): PagePackage | null {
  if (id.startsWith("\0")) {
    const tool = id.slice(1).split("/")[0];
    const carried = /^(@[^+/]+)\+([^@]+)@(.+)$/.exec(tool);
    if (carried) return { name: `${carried[1]}/${carried[2]}`, version: carried[3], directory: null };
    const directory = installedPackage(tool);
    if (directory === null) {
      throw new Error(`The bundle has code from ${JSON.stringify(id)}, and no package ${tool} is installed to name it in the licence notice.`);
    }
    return readPackage(directory);
  }
  const path = id.split("?")[0].split("\\").join("/");
  const directory = /^(.*\/node_modules\/(?:@[^/]+\/)?[^/]+)\//.exec(path)?.[1];
  return directory === undefined ? null : readPackage(directory);
}

// The npm packages whose code the bundle ships, for the Android app's licence notice
// (scripts/android-notices.mjs): what the chunks are made of, not what package.json
// lists. Off unless FAF_PAGE_PACKAGES names the file to write, as the Android build's
// page build does (scripts/android-page-build.mjs).
function pagePackages(output: string | undefined): Plugin | null {
  if (!output) return null;
  return {
    name: "faf-page-packages",
    apply: "build",
    buildStart() {
      // A build that fails must not leave the list of the one before it behind.
      rmSync(output, { force: true });
    },
    // After the files are written, so the list is never older than the page it describes.
    writeBundle(_options, bundle) {
      const packages = new Map<string, PagePackage>();
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== "chunk") continue;
        for (const [id, module] of Object.entries(chunk.modules)) {
          // A module the tree shaking emptied ships nothing, but a style sheet's rendered
          // JavaScript is always empty: Vite moves it into a CSS file of its own.
          if (module.renderedLength === 0 && !/\.(?:css|less|sass|scss|styl|stylus|pcss|postcss|sss)(?:$|\?)/.test(id)) {
            continue;
          }
          const found = modulePackage(id);
          if (found) packages.set(`${found.name}@${found.version}`, found);
        }
      }
      const list = [...packages].sort(([a], [b]) => (a < b ? -1 : 1)).map(([, found]) => found);
      mkdirSync(dirname(output), { recursive: true });
      writeFileSync(output, `${JSON.stringify(list, null, 2)}\n`);
    },
  };
}

// Frontend source lives in `ui/`. Output goes to `ui/dist`, which Tauri serves
// (see src-tauri/tauri.conf.json `frontendDist`).
export default defineConfig({
  root: "ui",
  plugins: [react(), pagePackages(process.env.FAF_PAGE_PACKAGES)],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2021",
    // Licence comments (`/*! ... */`, `@license`) stay in the shipped code.
    // The minifier dropped them all by default, which took the notices of
    // the libraries and icon drawings the client ships out of the copy that
    // ships, where their licences ask for them to be.
    rolldownOptions: {
      input: { main: "ui/index.html", notifications: "ui/notifications.html" },
      output: {
        comments: { legal: true },
      },
    },
  },
});
