// Unit tests for binary resolution helpers (versioned cache, executable-file
// checks, directory rejection, quoted Windows PATH) and `fetchWithGet`
// maxBytes / premature-close handling.

import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";

import {
  findBinaryOnPath,
  isExecutableFile,
  pruneOldCachedVersions,
  versionedCacheBinaryPath,
} from "../../binaryLookup";
import { fetchWithGet } from "../../download";

function makeTempDir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "alint-vscode-bin-"));
}

describe("binaryLookup + fetchWithGet hardening", () => {
  it("versionedCacheBinaryPath isolates by version and prunes older versions", () => {
    const storage = makeTempDir();
    try {
      const oldPath = versionedCacheBinaryPath(storage, "0.22.0", "linux");
      const newPath = versionedCacheBinaryPath(storage, "0.23.0", "linux");
      const legacyPath = path.join(storage, "bin", "alint");
      assert.notEqual(oldPath, newPath);
      fs.mkdirSync(path.dirname(oldPath), { recursive: true });
      fs.mkdirSync(path.dirname(newPath), { recursive: true });
      fs.writeFileSync(oldPath, "old", { mode: 0o755 });
      fs.writeFileSync(newPath, "new", { mode: 0o755 });
      fs.writeFileSync(legacyPath, "legacy", { mode: 0o755 });

      pruneOldCachedVersions(storage, "0.23.0");
      assert.equal(fs.existsSync(oldPath), false);
      assert.equal(fs.existsSync(legacyPath), false);
      assert.equal(fs.existsSync(newPath), true);
    } finally {
      fs.rmSync(storage, { recursive: true, force: true });
    }
  });

  it("skips directories named alint and non-executable files on PATH", () => {
    const tmp = makeTempDir();
    try {
      const dirWithFolder = path.join(tmp, "d1");
      const dirWithNonExec = path.join(tmp, "d2");
      const dirWithExec = path.join(tmp, "d3");
      fs.mkdirSync(path.join(dirWithFolder, "alint"), { recursive: true });
      fs.mkdirSync(dirWithNonExec, { recursive: true });
      fs.mkdirSync(dirWithExec, { recursive: true });
      fs.writeFileSync(path.join(dirWithNonExec, "alint"), "#!/bin/sh\n", { mode: 0o644 });
      const realBin = path.join(dirWithExec, "alint");
      fs.writeFileSync(realBin, "#!/bin/sh\n", { mode: 0o755 });

      assert.equal(isExecutableFile(path.join(dirWithFolder, "alint"), "linux"), false);
      if (process.platform !== "win32") {
        assert.equal(isExecutableFile(path.join(dirWithNonExec, "alint"), "linux"), false);
      }
      assert.equal(isExecutableFile(realBin, "linux"), true);

      const pathVar = [dirWithFolder, dirWithNonExec, dirWithExec].join(":");
      assert.equal(findBinaryOnPath(pathVar, "linux"), realBin);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("strips quotes from Windows PATH entries", () => {
    const tmp = makeTempDir();
    try {
      const quotedDir = path.join(tmp, "Program Files", "alint");
      fs.mkdirSync(quotedDir, { recursive: true });
      const exe = path.join(quotedDir, "alint.exe");
      fs.writeFileSync(exe, "MZ", { mode: 0o755 });
      const winPath = `"${quotedDir}";C:\\nonexistent`;
      assert.equal(findBinaryOnPath(winPath, "win32"), exe);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("rejects promptly on maxBytes overflow and mid-stream socket reset", async () => {
    const server = http.createServer((req, res) => {
      if (req.url === "/big") {
        res.writeHead(200);
        res.write(Buffer.alloc(4096, 0x41));
        res.end();
      } else if (req.url === "/reset") {
        res.writeHead(200, { "Content-Length": "10000" });
        res.write("partial");
        setTimeout(() => res.socket?.destroy(), 10);
      } else {
        res.writeHead(200);
        res.end("ok");
      }
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
    const addr = server.address();
    assert.ok(addr && typeof addr !== "string");
    const base = `http://127.0.0.1:${addr.port}`;
    try {
      await assert.rejects(
        () => fetchWithGet(`${base}/big`, 1024, 5, http.get, true),
        /exceeds 1024 bytes/,
      );
      await assert.rejects(
        () => fetchWithGet(`${base}/reset`, 100000, 5, http.get, true),
        /prematurely|truncated|aborted|ECONNRESET|socket hang up/i,
      );
      const ok = await fetchWithGet(`${base}/ok`, 1024, 5, http.get, true);
      assert.equal(ok.toString("utf8"), "ok");
    } finally {
      server.close();
    }
  });
});
