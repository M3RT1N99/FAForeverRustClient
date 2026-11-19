// faf-uid for the Android app: the program the lobby's sign-in runs, which FAF builds and
// publishes on FAForever/uid's releases. The Gradle build (src-tauri/gen/android/app/
// build.gradle.kts) runs this for every APK. It fetches FAF's build for each ABI at the
// version and SHA-256 that crates/faf-app/src/infra/lobby_ws/android_uid_pin.json pins, the
// pin the client checks the file against before it runs it, and packs it as
// <abi>/libfafuid.so, since Android runs an app's own programs only from its native library
// folder, with its licence text in assets/licenses/faf-uid. While the pin is empty, as until
// FAF publishes an Android build (FAForever/uid#18), it packs nothing and says so.
//
// Test builds only: FAF_UID_ANDROID_TEST_BUILDS names a folder of faf-uid builds made
// elsewhere, as faf-uid-android-<abi>, to pack instead, so that a test can run the program
// where the app runs it. The client never runs one to sign in: its SHA-256 is not the pin.
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

import { downloadVerified } from "./download-verified.mjs";

const root = resolve(import.meta.dirname, "..");
const PIN = join(root, "crates", "faf-app", "src", "infra", "lobby_ws", "android_uid_pin.json");
const LICENCE = join(root, "scripts", "android-notices", "faf-uid", "LICENSE");
// The downloads, kept between builds, as natives/ keeps the desktop helpers.
const CACHE = join(root, "natives", "android");

const say = (message) => console.log(`[faf-uid] ${message}`);

async function prepare({ jniLibs, assets, shipped, abis }) {
  const pin = JSON.parse(readFileSync(PIN, "utf8"));
  const testBuilds = process.env.FAF_UID_ANDROID_TEST_BUILDS || "";
  // What an earlier build packed for an ABI that now has none.
  for (const directory of [jniLibs, assets]) {
    rmSync(directory, { recursive: true, force: true });
    mkdirSync(directory, { recursive: true });
  }

  const packed = [];
  for (const abi of abis) {
    const asset = `faf-uid-android-${abi}`;
    let source;
    if (testBuilds) {
      source = join(testBuilds, asset);
      if (!existsSync(source)) throw new Error(`FAF_UID_ANDROID_TEST_BUILDS (${testBuilds}) has no ${asset}.`);
      say(`TEST BUILD: packing ${asset} from FAF_UID_ANDROID_TEST_BUILDS, which is not FAF's official build. The client never runs it to sign in, and the APK is for tests only.`);
    } else if (!Object.hasOwn(pin.sha256, abi)) {
      say(`FAF builds no faf-uid for ${abi}: the APK ships none for it.`);
      continue;
    } else if (!pin.sha256[abi]) {
      say(`No official faf-uid for Android is pinned yet (FAForever/uid#18): the APK ships none for ${abi}, and the client holds the lobby back.`);
      continue;
    } else {
      source = join(CACHE, pin.version, asset);
      const downloaded = await downloadVerified({
        label: "faf-uid",
        url: `https://github.com/FAForever/uid/releases/download/${pin.version}/${asset}`,
        target: source,
        expectedSha256: pin.sha256[abi],
      });
      say(`${downloaded ? "Downloaded and verified" : "Verified"} ${asset} ${pin.version}`);
    }
    mkdirSync(join(jniLibs, abi), { recursive: true });
    copyFileSync(source, join(jniLibs, abi, "libfafuid.so"));
    packed.push(abi);
  }

  if (packed.length > 0) {
    mkdirSync(join(assets, "licenses", "faf-uid"), { recursive: true });
    copyFileSync(LICENCE, join(assets, "licenses", "faf-uid", "LICENSE"));
  }
  // For scripts/android-notices.mjs, whose notice names what the APK ships.
  writeFileSync(shipped, JSON.stringify({ abis: packed, official: !testBuilds, version: testBuilds ? null : pin.version }));
}

const { values } = parseArgs({
  options: {
    "jni-libs": { type: "string" },
    assets: { type: "string" },
    shipped: { type: "string" },
    abi: { type: "string", multiple: true },
  },
});
if (!values["jni-libs"] || !values.assets || !values.shipped || !values.abi) {
  console.error("Usage: node scripts/ensure-faf-uid-android.mjs --jni-libs <dir> --assets <dir> --shipped <json> --abi <abi>...");
  process.exit(2);
}
try {
  await prepare({ jniLibs: values["jni-libs"], assets: values.assets, shipped: values.shipped, abis: values.abi });
} catch (error) {
  console.error(`[faf-uid] ${error.message}`);
  process.exit(1);
}
