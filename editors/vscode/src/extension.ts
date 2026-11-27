// alint VS Code extension entry point.
//
// The extension is glue: it locates the `alint` binary, launches
// `alint lsp` as a language server over stdio, and registers an LSP
// client. Diagnostics, hover, and quick-fixes are rendered natively by
// VS Code from the server's LSP messages — none of that logic lives
// here. See `docs/design/v0.11/vscode_extension.md`.
//
// Workspace trust: the server is told whether the workspace is trusted
// (`initializationOptions.trusted`). Until it is, the server runs nothing the
// repository's config could start (command rules, rules that run Git, custom
// facts), fetches no remote `extends:`, refuses `allow_out_of_root:`, and
// reads no environment value.
// Granting trust restarts the server with `trusted: true`. `alint.path` and `alint.serverArgs` are
// restricted settings (package.json), so a workspace cannot choose the binary
// or its arguments while untrusted.

import { execFile, spawn } from "node:child_process";

import { type Disposable, type ExtensionContext, type LogOutputChannel, Uri, commands, window, workspace } from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  type StreamInfo,
} from "vscode-languageclient/node";

import { resolveAlintBinary } from "./binary";

let client: LanguageClient | undefined;
let configWatcher: Disposable | undefined;
let channel: LogOutputChannel | undefined;
let lifecycle: Promise<void> = Promise.resolve();
let lifecycleGen = 0;

function enqueueLifecycle(op: (gen: number) => Promise<void>): Promise<void> {
  const gen = ++lifecycleGen;
  lifecycle = lifecycle.then(
    () => op(gen),
    () => op(gen),
  );
  return lifecycle;
}

export async function activate(context: ExtensionContext): Promise<void> {
  channel = window.createOutputChannel("alint", { log: true });
  context.subscriptions.push(channel);
  context.subscriptions.push(
    commands.registerCommand("alint.restartServer", () => restart(context)),
    commands.registerCommand("alint.showRules", () => showRules(context)),
    commands.registerCommand("alint.openConfig", openConfig),
    // The server reads trust at `initialize`, so a grant restarts it.
    workspace.onDidGrantWorkspaceTrust(() => restart(context)),
  );
  await enqueueLifecycle((gen) => start(context, gen));
}

async function start(context: ExtensionContext, gen: number): Promise<void> {
  const log = channel!;
  const binary = await resolveAlintBinary(context, log);
  if (gen !== lifecycleGen) {
    return;
  }
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
    server.on("error", (err: Error) => {
      log.appendLine(`alint lsp spawn error: ${err.message}`);
    });
    server.stderr.on("data", (chunk: Buffer) => log.append(chunk.toString()));
    return Promise.resolve({ reader: server.stdout, writer: server.stdin });
  };
  const watcher = workspace.createFileSystemWatcher("**/.alint.yml");
  configWatcher = watcher;
  const clientOptions: LanguageClientOptions = {
    // alint is repo-structural, not language-specific — watch every
    // file in the workspace. The server decides which rules apply.
    documentSelector: [{ scheme: "file" }],
    synchronize: {
      fileEvents: watcher,
    },
    // An untrusted workspace's config runs no command rule, custom fact,
    // remote `extends:`, `allow_out_of_root:` or environment read until trust is granted.
    initializationOptions: { trusted: workspace.isTrusted },
    outputChannel: log,
  };
  const nextClient = new LanguageClient("alint", "alint", serverOptions, clientOptions);
  client = nextClient;
  await nextClient.start();
  if (gen !== lifecycleGen && client === nextClient) {
    await stopUnlocked();
    return;
  }
  log.appendLine(`alint language server started (${binary}).`);
}

async function stopUnlocked(): Promise<void> {
  if (configWatcher) {
    configWatcher.dispose();
    configWatcher = undefined;
  }
  if (client) {
    const current = client;
    client = undefined;
    await current.stop();
  }
}

async function restart(context: ExtensionContext): Promise<void> {
  await enqueueLifecycle(async (gen) => {
    await stopUnlocked();
    if (gen === lifecycleGen) {
      await start(context, gen);
    }
  });
}

async function showRules(context: ExtensionContext): Promise<void> {
  const log = channel!;
  // `alint list` loads the repository's config, which can fetch a remote
  // `extends:` and read the environment.
  if (!workspace.isTrusted) {
    window.showInformationMessage(
      "alint: trust this workspace to list its rules (Workspace Trust). Until then the alint language server lints it without command rules, rules that run Git, custom facts, remote `extends:`, `allow_out_of_root:` or environment values.",
    );
    return;
  }
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
  await enqueueLifecycle(() => stopUnlocked());
}
