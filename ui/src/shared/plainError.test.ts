import { describe, expect, it } from "vitest";
import { plainError } from "./plainError";
import { en } from "../i18n/catalog/en";

describe("plainError", () => {
  it("reads a locked folder from the os error, whatever language the system wrote it in", () => {
    // The report: a mod install on a German Windows.
    const reason =
      "could not finish installing C:\\ProgramData\\FAForever\\user\\My Games\\Gas Powered Games\\Supreme Commander Forged Alliance\\mods\\StrategicDefenceRangePreview: Zugriff verweigert (os error 5)";
    expect(plainError(reason)).toBe(en["errors.cause.locked"]);
    expect(plainError("Permission denied (os error 13)")).toBe(en["errors.cause.locked"]);
  });

  it("tells a file in use, a full disk and a missing path apart", () => {
    expect(plainError("The process cannot access the file (os error 32)")).toBe(en["errors.cause.inUse"]);
    expect(plainError("No space left on device (os error 28)")).toBe(en["errors.cause.diskFull"]);
    expect(plainError("Das System kann den angegebenen Pfad nicht finden. (os error 3)")).toBe(en["errors.cause.missing"]);
  });

  it("names network failures and HTTP statuses", () => {
    expect(plainError("error sending request for url (https://api.faforever.com/data/mod)")).toBe(en["errors.cause.offline"]);
    expect(plainError("operation timed out")).toBe(en["errors.cause.timeout"]);
    expect(plainError("HTTP 404 from the content server")).toBe(en["errors.cause.notFound"]);
    expect(plainError("server returned status 503")).toBe(en["errors.cause.server"]);
    expect(plainError("not logged in")).toBe(en["errors.cause.signIn"]);
  });

  it("keeps a sentence the client wrote itself", () => {
    const own = "That avatar is not in the server-provided list.";
    expect(plainError(own)).toBe(own);
  });

  it("translates why the vaults install nothing on Android", () => {
    // `vault_install::NOT_ON_ANDROID`, word for word.
    const refused = "Maps and mods are not installed on Android yet. A later version hands them to the game app.";
    expect(plainError(refused)).toBe(en["errors.cause.vaultInstallsAndroid"]);
  });

  it("says plainly that a picker is not available on Android", () => {
    // The shell's `pick_path` refusal, now and as test build 3 still worded it.
    expect(plainError("Choosing files and folders is not available on Android.")).toBe(en["errors.cause.notOnAndroid"]);
    expect(plainError("folders cannot be picked on Android")).toBe(en["errors.cause.notOnAndroid"]);
  });

  it("tidies what it does not recognise: no long paths, no snake case, a full stop", () => {
    expect(plainError("game_not_found")).toBe("Game not found.");
    expect(plainError("could not read D:\\Games\\FA\\maps\\scmp_009\\scmp_009_scenario.lua: bad header")).toBe(
      "Could not read scmp_009_scenario.lua: bad header.",
    );
  });

  it("keeps a web address whole rather than cutting it like a path", () => {
    // The tournament service names its address when it cannot be reached, so
    // whoever runs a local server knows which one to start.
    const unreachable =
      "Could not reach the tournament service at https://tourney.example/api/. If that is a local server, check it is running.";
    expect(plainError(unreachable)).toBe(unreachable);
    expect(plainError("could not fetch https://tourney.example/api/map_pool: bad_gateway_reply")).toBe(
      "Could not fetch https://tourney.example/api/map_pool: bad gateway reply.",
    );
  });
});
