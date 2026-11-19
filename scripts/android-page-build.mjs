// The Android build's page build (`build.beforeBuildCommand` in
// src-tauri/tauri.android.conf.json): the usual `pnpm build`, with vite.config.ts
// listing the npm packages the bundle ships, for the licence notice the Gradle build
// puts into the APK (scripts/android-notices.mjs). A script rather than
// `FAF_PAGE_PACKAGES=... pnpm build`, which a Windows shell cannot run.
import { spawnSync } from "node:child_process";

import { PAGE_PACKAGES } from "./android-notices.mjs";

// One command string through the shell: on Windows pnpm is a .cmd script, which
// nothing else starts.
const { status, error } = spawnSync("pnpm build", {
  env: { ...process.env, FAF_PAGE_PACKAGES: PAGE_PACKAGES },
  shell: true,
  stdio: "inherit",
});
if (error) throw error;
process.exit(status ?? 1);
