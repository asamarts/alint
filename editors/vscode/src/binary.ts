// Binary resolution. Consistent with the other install channels:
//   1. the `alint.path` setting (explicit override),
//   2. `alint` on PATH,
//   3. a previously downloaded copy in the extension's global storage,
//   4. otherwise prompt the user to download or locate one.
// Auto-download is opt-in (the prompt) — never silent.

import * as path from "node:path";

import {
  ConfigurationTarget,
  type ExtensionContext,
  type OutputChannel,
  window,
  workspace,
} from "vscode";

import {
  findBinaryOnPath,
  isExecutableFile,
  pruneOldCachedVersions,
  versionedCacheBinaryPath,
} from "./binaryLookup";
import { downloadAlint } from "./download";

export async function resolveAlintBinary(
  context: ExtensionContext,
  log: OutputChannel,
): Promise<string | undefined> {
  // 1. Explicit setting.
  const configured = workspace.getConfiguration("alint").get<string>("path")?.trim();
  if (configured) {
    if (isExecutableFile(configured)) {
      return configured;
    }
    window.showWarningMessage(
      `alint.path does not point to an executable file: ${configured}`,
    );
  }

  // 2. PATH.
  const onPath = findBinaryOnPath();
  if (onPath) {
    return onPath;
  }

  // 3. Previously downloaded copy matching this extension version.
  const version = (context.extension.packageJSON as { version: string }).version;
  const cached = versionedCacheBinaryPath(context.globalStorageUri.fsPath, version);
  if (isExecutableFile(cached)) {
    return cached;
  }

  // 4. Prompt (opt-in download).
  const choice = await window.showInformationMessage(
    "alint was not found on your PATH. Download the matching release, or locate an existing binary?",
    "Download",
    "Locate…",
  );
  if (choice === "Download") {
    try {
      // Download the alint release matching this extension's version.
      // The extension version is stamped to the alint release at publish
      // time, so `v<version>` always exists.
      const dest = await downloadAlint(version, path.dirname(cached), (m) =>
        log.appendLine(`[download] ${m}`),
      );
      pruneOldCachedVersions(context.globalStorageUri.fsPath, version);
      return dest;
    } catch (err) {
      window.showErrorMessage(`alint download failed: ${(err as Error).message}`);
      return undefined;
    }
  }
  if (choice === "Locate…") {
    const picked = await window.showOpenDialog({
      canSelectMany: false,
      openLabel: "Select alint binary",
    });
    if (picked && picked[0]) {
      const selectedPath = picked[0].fsPath;
      if (!isExecutableFile(selectedPath)) {
        window.showWarningMessage(
          `Selected path is not an executable file: ${selectedPath}`,
        );
        return undefined;
      }
      try {
        await workspace
          .getConfiguration("alint")
          .update("path", selectedPath, ConfigurationTarget.Global);
      } catch (err) {
        log.appendLine(
          `could not persist alint.path setting: ${(err as Error).message}`,
        );
      }
      return selectedPath;
    }
  }
  return undefined;
}
