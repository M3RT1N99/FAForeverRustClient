// The Android app's licence notice, assets/NOTICE.txt. The Gradle build
// (src-tauri/gen/android/app/build.gradle.kts) runs this for every APK, on what that
// build compiled, so the notice cannot drift from the app: section 1 is the client's
// own licence, 2 the Rust crates in its native library, 3 the npm packages in its page
// and 4 the Android libraries on its classpath. Each distinct text is printed once,
// after the list of the packages it covers.
//
// A package that ships no licence text of its own fails the build, unless
// scripts/android-notices/ holds a reviewed override for it: a directory
// <crates|npm|maven>/<name>/ with the texts as files beside an override.txt, which
// lists the packages under "Packages:", one "name version" a line, and says under
// "Source:" where the texts come from. An override is never written from memory: it
// is the package's licence file from its own repository at the version shipped, or
// the standard text of its declared licence with its own copyright line.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const root = resolve(import.meta.dirname, "..");
const overrideRoot = join(import.meta.dirname, "android-notices");

// Where the Android build's page build (scripts/android-page-build.mjs) has
// vite.config.ts list the npm packages in the bundle. Beside the Gradle build's output
// rather than in ui/dist, which Tauri embeds into the app whole: the list holds this
// machine's paths.
export const PAGE_PACKAGES = join(root, "src-tauri", "gen", "android", "app", "build", "notices", "page-packages.json");

// A licence file by name: LICENSE-MIT, LICENCE, COPYING, NOTICE.md, UNLICENSE,
// COPYRIGHT, THIRD-PARTY-LICENSE and the like, in any case.
const LICENCE_FILE = /licen[cs]e|copying|copyright|notice/i;

const KINDS = { crates: "Rust crate", npm: "npm package", maven: "Android library" };
const RULE = "=".repeat(78);
const LINE = "-".repeat(78);

// What the build reports as it is, without a stack trace: what is missing, and why.
class NoticeError extends Error {}

const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const byPackage = (a, b) => byText(a.name, b.name) || byText(a.version, b.version);

function repositoryPath(path) {
  return relative(root, path).split(sep).join("/");
}

function wrap(text, width = 78) {
  const lines = [];
  let line = "";
  for (const word of text.split(/\s+/)) {
    if (line && line.length + 1 + word.length > width) {
      lines.push(line);
      line = word;
    } else {
      line = line ? `${line} ${word}` : word;
    }
  }
  return [...lines, line].join("\n");
}

// A licence text as printed: no byte order mark, Unix line ends, no blanks at the ends of
// lines or around it.
function normalise(text) {
  return (text.charCodeAt(0) === 0xfeff ? text.slice(1) : text).replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").replace(/^\n+|\n+$/g, "");
}

function readText(path) {
  const bytes = readFileSync(path);
  try {
    return normalise(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    // An old licence file in Latin-1.
    return normalise(bytes.toString("latin1"));
  }
}

// An SPDX document (LICENSE.spdx) names a licence and its holders but holds no licence
// text, so on its own it is not one.
const isLicenceText = ({ file }) => !file.toLowerCase().endsWith(".spdx");

// The licence files at the top of a package's directory, and in a folder of them
// (LICENSES/). Dot files are tool settings and templates.
function shippedTexts(directory) {
  const files = [];
  for (const name of readdirSync(directory).sort(byText)) {
    if (name.startsWith(".") || !LICENCE_FILE.test(name)) continue;
    const path = join(directory, name);
    if (statSync(path).isDirectory()) {
      for (const inner of readdirSync(path).sort(byText)) {
        if (statSync(join(path, inner)).isFile()) files.push(`${name}/${inner}`);
      }
    } else {
      files.push(name);
    }
  }
  return files.map((file) => ({ file, text: readText(join(directory, file)) }));
}

// The crates compiled into the native library: the normal dependencies of the shell
// crate, followed through the workspace's own crates, which section 1 covers. Build
// dependencies run only on the build machine, dev-dependencies only in tests.
function rustCrates(targets) {
  const args = ["metadata", "--format-version", "1", "--manifest-path", join(root, "Cargo.toml")];
  for (const target of targets) args.push("--filter-platform", target);
  const metadata = JSON.parse(execFileSync("cargo", args, {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  }));
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const members = new Set(metadata.workspace_members);
  const shell = metadata.packages.find((pkg) => pkg.name === "faforever-rust-client" && members.has(pkg.id));
  if (!shell) throw new NoticeError("cargo metadata lists no faforever-rust-client crate in the workspace.");

  // A Set's iteration also visits what is added to it on the way: a breadth-first walk.
  const reached = new Set([shell.id]);
  for (const id of reached) {
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some(({ kind }) => kind === null)) reached.add(dep.pkg);
    }
  }

  return [...reached].filter((id) => !members.has(id)).map((id) => {
    const pkg = packages.get(id);
    const directory = dirname(pkg.manifest_path);
    const texts = shippedTexts(directory);
    if (pkg.license_file) {
      const path = resolve(directory, pkg.license_file);
      if (!existsSync(path)) throw new NoticeError(`${pkg.name} ${pkg.version} names ${pkg.license_file} as its licence file, which it does not ship.`);
      const file = relative(directory, path).split(sep).join("/");
      if (!texts.some((text) => text.file === file)) texts.push({ file, text: readText(path) });
    }
    return { kind: "crates", name: pkg.name, version: pkg.version, licence: pkg.license, directory, texts };
  });
}

// The npm packages the page build found in the bundle, or why there is no list.
function pagePackages(release) {
  const page = join(root, "ui", "dist", "index.html");
  let unlisted = null;
  if (!existsSync(PAGE_PACKAGES)) {
    unlisted = `${repositoryPath(PAGE_PACKAGES)} does not exist`;
  } else if (existsSync(page) && statSync(page).mtimeMs > statSync(PAGE_PACKAGES).mtimeMs) {
    unlisted = `${repositoryPath(PAGE_PACKAGES)} is older than the page in ui/dist`;
  }
  if (unlisted) {
    if (release) {
      throw new NoticeError(`${unlisted}. A release build's page comes from \`pnpm exec tauri android build\`, whose page build (scripts/android-page-build.mjs) lists the npm packages in it.`);
    }
    return { packages: [], unlisted };
  }

  const packages = JSON.parse(readFileSync(PAGE_PACKAGES, "utf8")).map(({ name, version, directory }) => {
    // A package a build tool carries inside itself, not installed: its texts come from an override.
    if (directory === null) return { kind: "npm", name, version, texts: [] };
    const { license } = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
    return { kind: "npm", name, version, licence: typeof license === "string" ? license : undefined, texts: shippedTexts(directory) };
  });
  return { packages, unlisted: null };
}

// What the Gradle build found in the jars and AARs on the variant's runtime classpath,
// and the paths of the Android library projects it builds from source.
function androidLibraries(path) {
  const { libraries, projects } = JSON.parse(readFileSync(path, "utf8"));
  return {
    libraries: libraries.map(({ group, name, version, texts }) => ({
      kind: "maven",
      name: `${group}:${name}`,
      version,
      texts: texts.map(({ entry, text }) => ({ file: entry, text: normalise(text) })),
    })),
    projects,
  };
}

// The packages an override.txt lists under "Packages:". What it says under "Source:"
// is for its reviewer; it only has to be there.
function overridePackages(path) {
  const where = repositoryPath(path);
  if (!existsSync(path)) {
    throw new NoticeError(`${where} is missing: it lists the packages the texts beside it are for, and says where the texts come from.`);
  }
  const parts = { Packages: [], Source: [] };
  let part = null;
  for (const line of readText(path).split("\n").map((text) => text.trim())) {
    const heading = line.endsWith(":") ? line.slice(0, -1) : null;
    if (heading !== null && Object.hasOwn(parts, heading)) part = parts[heading];
    else if (line === "") continue;
    else if (part === null) throw new NoticeError(`${where}: "${line}" comes before "Packages:".`);
    else part.push(line);
  }
  if (parts.Source.length === 0) throw new NoticeError(`${where} does not say where its texts come from, under "Source:".`);
  if (parts.Packages.length === 0) throw new NoticeError(`${where} lists no packages under "Packages:".`);
  return parts.Packages.map((line) => {
    const fields = line.split(/\s+/);
    if (fields.length !== 2) throw new NoticeError(`${where}: "${line}" under "Packages:" is not "name version".`);
    return fields.join(" ");
  });
}

function loadOverrides() {
  const overrides = new Map();
  for (const kind of Object.keys(KINDS)) {
    const base = join(overrideRoot, kind);
    if (!existsSync(base)) continue;
    for (const name of readdirSync(base).sort(byText)) {
      const directory = join(base, name);
      const where = repositoryPath(directory);
      if (!statSync(directory).isDirectory()) {
        throw new NoticeError(`${where}: an override is a directory, of an override.txt and the texts.`);
      }
      const packages = overridePackages(join(directory, "override.txt"));
      const texts = readdirSync(directory).filter((file) => file !== "override.txt").sort(byText)
        .map((file) => ({ file: `${where}/${file}`, text: readText(join(directory, file)) }));
      if (!texts.some(isLicenceText) || texts.some(({ text }) => text === "")) {
        throw new NoticeError(`${where} holds no licence text, or an empty file.`);
      }
      for (const id of packages) {
        const key = `${kind} ${id}`;
        const other = overrides.get(key);
        if (other) throw new NoticeError(`${where} and ${other.where} both supply ${id}.`);
        overrides.set(key, { where, texts, used: false });
      }
    }
  }
  return overrides;
}

// A package keeps the texts it ships. One that ships none takes its override's, or is
// reported missing.
function applyOverrides(packages, overrides, missing) {
  for (const pkg of packages) {
    pkg.texts = pkg.texts.filter(({ text }) => text !== "");
    if (pkg.texts.some(isLicenceText)) continue;
    const override = overrides.get(`${pkg.kind} ${pkg.name} ${pkg.version}`);
    if (override) {
      override.used = true;
      pkg.texts = [...pkg.texts, ...override.texts];
    } else {
      missing.push(pkg);
    }
  }
}

const label = (pkg) => `${pkg.name} ${pkg.version}${pkg.licence ? ` (${pkg.licence})` : ""}`;

// Each distinct text once, after the packages it covers, the most shared first. Texts
// that differ in white space alone are one: licence files are often the same terms,
// wrapped another way.
function textBlocks(packages) {
  const blocks = new Map();
  for (const pkg of [...packages].sort(byPackage)) {
    for (const { file, text } of pkg.texts) {
      const key = text.replace(/\s+/g, " ");
      if (!blocks.has(key)) blocks.set(key, { text, users: [] });
      blocks.get(key).users.push(`${label(pkg)}: ${file}`);
    }
  }
  return [...blocks.values()]
    .sort((a, b) => b.users.length - a.users.length || byText(a.users[0], b.users[0]))
    .map(({ text, users }) => [LINE, ...users, LINE, "", text, ""].join("\n"))
    .join("\n");
}

function section(number, title, intro, body) {
  return [RULE, `${number}. ${title}`, RULE, "", wrap(intro), "", body].join("\n");
}

const isInside = (path, directory) => {
  const fromDirectory = relative(directory, path);
  return fromDirectory === "" || (!fromDirectory.startsWith("..") && !isAbsolute(fromDirectory));
};

function generateNotice({ librariesPath, rustTargets, release }) {
  const crates = rustCrates(rustTargets);
  const page = pagePackages(release);
  const android = androidLibraries(librariesPath);

  const overrides = loadOverrides();
  const missing = [];
  for (const packages of [crates, page.packages, android.libraries]) applyOverrides([...packages].sort(byPackage), overrides, missing);
  if (missing.length > 0) {
    throw new NoticeError([
      "These packages ship no licence text, and scripts/android-notices/ has no override for them:",
      ...missing.map((pkg) => `  ${KINDS[pkg.kind]} ${label(pkg)}`),
      wrap("Each needs a reviewed override: a directory scripts/android-notices/<crates|npm|maven>/<name>/ with the package's licence texts and an override.txt that lists the package under \"Packages:\" as \"name version\" and says under \"Source:\" where the texts come from."),
    ].join("\n"));
  }
  for (const [key, override] of overrides) {
    if (override.used || (page.unlisted && key.startsWith("npm "))) continue;
    console.warn(`[android-notices] ${override.where} supplies ${key.slice(key.indexOf(" ") + 1)}, which this build does not have or which ships a licence text of its own.`);
  }

  // Tauri's Android libraries are projects of this build, compiled from the sources of
  // crates in section 2: tauri.settings.gradle points each at a crate's directory.
  const projects = Object.entries(android.projects).sort(([a], [b]) => byText(a, b)).map(([path, directory]) => {
    const crate = crates.find((candidate) => isInside(directory, candidate.directory));
    if (!crate) {
      throw new NoticeError(`The Android library project ${path} (${directory}) lies in none of the Rust crates of the native library, whose licence would cover it.`);
    }
    return `${path} from ${crate.name} ${crate.version}`;
  });

  const notice = [
    "FAForever Client for Android: licence notices",
    "",
    wrap("This app is the FAForever Client and the open source packages it is built from, whose licences ask for their notices to come with every copy. Its build generates this file from the packages it compiled, and prints each distinct licence text once, after the list of the packages it covers."),
    "",
    "1. The FAForever Client",
    `2. Rust crates in the native library (${crates.length})`,
    `3. npm packages in the page (${page.unlisted ? "not listed in this build" : page.packages.length})`,
    `4. Android libraries (${android.libraries.length})`,
    "",
    section(1, "The FAForever Client", "The client's own code, its Rust crates and its page, and the third-party material it includes.", [
      readText(join(root, "LICENSE")),
      "",
      LINE,
      "",
      readText(join(root, "THIRD_PARTY_NOTICES.md")),
      "",
    ].join("\n")),
    section(2, `Rust crates in the native library (${crates.length})`,
      `The crates compiled into the app's native library, libfaforever_rust_client_lib.so, for ${rustTargets.join(", ")}, with the licence files each ships. A crate that offers a choice of licences ("MIT OR Apache-2.0") is listed with every licence file it ships.`,
      textBlocks(crates)),
    page.unlisted
      ? section(3, "npm packages in the page", `This debug build has no list of the npm packages in its page (${page.unlisted}): a debug build may load its page from the development server, which builds none. A release build fails without the list.`, "")
      : section(3, `npm packages in the page (${page.packages.length})`,
        "The npm packages whose code the page's bundle contains, as its build found them, the helpers the build tools write into it included.",
        textBlocks(page.packages)),
    section(4, `Android libraries (${android.libraries.length})`,
      `The Java and Kotlin libraries on the app's runtime classpath, with the licence files in their jars and AARs.${projects.length > 0 ? ` Tauri's own Android libraries are built from the sources of crates in section 2 and come under those crates' licences: ${projects.join(", ")}.` : ""}`,
      textBlocks(android.libraries)),
  ].join("\n");
  return { notice, counts: { crates: crates.length, npm: page.unlisted ? null : page.packages.length, android: android.libraries.length } };
}

// The versions pnpm-lock.yaml holds of each package, from its `packages:` part.
function lockedNpmPackages() {
  const locked = new Map();
  let inPackages = false;
  for (const line of readText(join(root, "pnpm-lock.yaml")).split("\n")) {
    if (/^\S/.test(line)) inPackages = line === "packages:";
    const key = inPackages ? /^ {2}'?((?:@[^/@\s]+\/)?[^@'\s]+)@([^'\s(]+)'?:$/.exec(line) : null;
    if (key) locked.set(key[1], (locked.get(key[1]) ?? new Set()).add(key[2]));
  }
  return locked;
}

// What CI can check without the Android build, on every pull request: every crate of
// the native library has a licence text or an override; no crate override names a crate
// the library no longer has, at that version; and no npm override names a version of a
// package other than the ones pnpm-lock.yaml holds. A package a build tool carries inside
// itself (Rolldown's Oxc helpers) is in no lockfile and is left to the release build, as
// are the page's other packages and the Android libraries: only those builds list them.
function checkWithoutBuild(rustTargets) {
  const crates = rustCrates(rustTargets);
  const overrides = loadOverrides();
  const missing = [];
  applyOverrides([...crates].sort(byPackage), overrides, missing);
  const problems = missing.map((pkg) => `  ${KINDS[pkg.kind]} ${label(pkg)} ships no licence text and has no override.`);
  const locked = lockedNpmPackages();
  for (const [key, override] of overrides) {
    const id = key.slice(key.indexOf(" ") + 1);
    if (key.startsWith("crates ") && !override.used) {
      problems.push(`  ${override.where} supplies the crate ${id}, which the native library no longer has, or which ships a licence text of its own.`);
    } else if (key.startsWith("npm ")) {
      const [name, version] = id.split(" ");
      if (locked.has(name) && !locked.get(name).has(version)) {
        problems.push(`  ${override.where} supplies the npm package ${id}, while pnpm-lock.yaml holds ${[...locked.get(name)].join(", ")}.`);
      }
    }
  }
  if (problems.length > 0) {
    throw new NoticeError(["The Android app's licence notice would not build:", ...problems].join("\n"));
  }
  return crates.length;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const { values } = parseArgs({
    options: {
      out: { type: "string" },
      "android-libraries": { type: "string" },
      "rust-target": { type: "string", multiple: true },
      release: { type: "boolean", default: false },
      check: { type: "boolean", default: false },
    },
  });
  if (values.check) {
    try {
      const crates = checkWithoutBuild(values["rust-target"] ?? ["aarch64-linux-android"]);
      console.log(`[android-notices] ${crates} crates have their licence texts, and the overrides match the lockfiles.`);
      process.exit(0);
    } catch (error) {
      if (!(error instanceof NoticeError)) throw error;
      console.error(`[android-notices] ${error.message}`);
      process.exit(1);
    }
  }
  if (!values.out || !values["android-libraries"]) {
    console.error("Usage: node scripts/android-notices.mjs --out <NOTICE.txt> --android-libraries <json> [--rust-target <triple>]... [--release]");
    console.error("   or: node scripts/android-notices.mjs --check [--rust-target <triple>]...");
    process.exit(2);
  }
  try {
    const { notice, counts } = generateNotice({
      librariesPath: values["android-libraries"],
      rustTargets: values["rust-target"] ?? ["aarch64-linux-android"],
      release: values.release,
    });
    mkdirSync(dirname(values.out), { recursive: true });
    writeFileSync(values.out, notice);
    console.log(`[android-notices] ${counts.crates} crates, ${counts.npm ?? "no list of the"} npm packages, ${counts.android} Android libraries: ${Math.round(Buffer.byteLength(notice) / 1024)} KB`);
  } catch (error) {
    if (!(error instanceof NoticeError)) throw error;
    console.error(`[android-notices] ${error.message}`);
    process.exit(1);
  }
}
