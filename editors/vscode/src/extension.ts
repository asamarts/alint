// alint VS Code extension entry point.
//
// The extension is glue: it locates the `alint` binary, launches
// `alint lsp` as a language server over stdio, and registers an LSP
// client. Diagnostics, hover, and quick-fixes are rendered natively by
// VS Code from the server's LSP messages — none of that logic lives
// here. See `docs/design/v0.11/vscode_extension.md`.

import { execFile, spawn } from "node:child_process";

import { type ExtensionContext, type LogOutputChannel, Uri, commands, window, workspace } from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  type StreamInfo,
} from "vscode-languageclient/node";

import { resolveAlintBinary } from "./binary";

let client: LanguageClient | undefined;
let channel: LogOutputChannel | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  channel = window.createOutputChannel("alint", { log: true });
  context.subscriptions.push(channel);
  context.subscriptions.push(
    commands.registerCommand("alint.restartServer", () => restart(context)),
    commands.registerCommand("alint.showRules", () => showRules(context)),
    commands.registerCommand("alint.openConfig", openConfig),
  );
  await start(context);
}

async function start(context: ExtensionContext): Promise<void> {
  const log = channel!;
  const binary = await resolveAlintBinary(context, log);
  if (!binary) {
    log.appendLine("alint binary not found; language server not started.");
    return;
  }
  const extraArgs = workspace.getConfiguration("alint").get<string[]>("serverArgs") ?? [];
  // Spawn the server ourselves and hand the client the streams
  // (StreamInfo), rather than the Executable/stdio form where
  // vscode-languageclient spawns it. The Executable path triggers an
  // "Unexpected SIGPIPE" in the extension host that disposes the
  // connection during start() under headless test hosts; spawning
  // directly is stable. alint speaks LSP over stdio either way.
  const serverOptions: ServerOptions = (): Promise<StreamInfo> => {
    const server = spawn(binary, ["lsp", ...extraArgs], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    server.stderr.on("data", (chunk: Buffer) => log.append(chunk.toString()));
    return Promise.resolve({ reader: server.stdout, writer: server.stdin });
  };
  const clientOptions: LanguageClientOptions = {
    // alint is repo-structural, not language-specific — watch every
    // file in the workspace. The server decides which rules apply.
    documentSelector: [{ scheme: "file" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/.alint.yml"),
    },
    outputChannel: log,
  };
  client = new LanguageClient("alint", "alint", serverOptions, clientOptions);
  await client.start();
  log.appendLine(`alint language server started (${binary}).`);
}

async function stop(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

async function restart(context: ExtensionContext): Promise<void> {
  await stop();
  await start(context);
}

async function showRules(context: ExtensionContext): Promise<void> {
  const log = channel!;
  const binary = await resolveAlintBinary(context, log);
  if (!binary) {
    return;
  }
  const cwd = workspace.workspaceFolders?.[0]?.uri.fsPath;
  log.show(true);
  log.appendLine("=== alint list ===");
  execFile(binary, ["list"], { cwd }, (err, stdout, stderr) => {
    if (err) {
      log.appendLine(`alint list failed: ${err.message}\n${stderr}`);
      return;
    }
    log.appendLine(stdout);
  });
}

async function openConfig(): Promise<void> {
  const folders = workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    window.showWarningMessage("No workspace folder is open.");
    return;
  }
  const uri = Uri.joinPath(folders[0].uri, ".alint.yml");
  try {
    const doc = await workspace.openTextDocument(uri);
    await window.showTextDocument(doc);
  } catch {
    window.showWarningMessage("No .alint.yml found in the workspace root.");
  }
}

export async function deactivate(): Promise<void> {
  await stop();
}
