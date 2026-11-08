// Platform → release-target mapping. Mirrors `npm/platforms.json` and the
// `release.yml` build matrix exactly so the extension downloads the
// byte-identical tarballs the other install paths use.

/** Map a Node `process.platform`/`process.arch` pair to the alint
 * release-target triple. Throws for unsupported platforms. */
export function resolveTarget(platform: string, arch: string): string {
  const map: Record<string, string> = {
    "linux/x64": "x86_64-unknown-linux-musl",
    "linux/arm64": "aarch64-unknown-linux-musl",
    "darwin/x64": "x86_64-apple-darwin",
    "darwin/arm64": "aarch64-apple-darwin",
    "win32/x64": "x86_64-pc-windows-msvc",
    "win32/arm64": "aarch64-pc-windows-msvc",
  };
  const key = `${platform}/${arch}`;
  const target = map[key];
  if (!target) {
    const supported = Object.keys(map).join(", ");
    throw new Error(
      `unsupported platform ${platform}/${arch} (supported: ${supported}). ` +
        `Download manually from https://github.com/asamarts/alint/releases`,
    );
  }
  return target;
}

/** The binary's filename for a platform (`alint.exe` on Windows). */
export function binaryName(platform: string): string {
  return platform === "win32" ? "alint.exe" : "alint";
}
