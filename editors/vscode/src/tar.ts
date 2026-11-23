// Minimal, dependency-free in-memory `.tar.gz` single-file extractor.
//
// Replaces `node-tar` for extracting the single `alint` binary from a
// SHA-256-verified release archive. Works entirely in memory via Node's
// built-in `node:zlib` and fails closed on symlinks, hardlinks, path
// traversal, corrupt headers, truncated archives, and duplicate targets.

import * as zlib from "node:zlib";

const BLOCK_SIZE = 512;

/** Default upper bound on uncompressed archive bytes (256 MiB). */
export const DEFAULT_MAX_ARCHIVE_BYTES = 256 * 1024 * 1024;

interface PaxOverrides {
  path?: string;
  size?: number;
}

function isZeroBlock(buf: Buffer, offset: number): boolean {
  for (let i = 0; i < BLOCK_SIZE; i++) {
    if (buf[offset + i] !== 0) {
      return false;
    }
  }
  return true;
}

function readCString(buf: Buffer, start: number, length: number): string {
  const slice = buf.subarray(start, start + length);
  const nul = slice.indexOf(0);
  return slice.subarray(0, nul === -1 ? length : nul).toString("utf8");
}

function parseOctalField(buf: Buffer, start: number, length: number, fieldName: string): number {
  const raw = readCString(buf, start, length).trim();
  if (raw.length === 0) {
    return 0;
  }
  if (!/^[0-7]+$/.test(raw)) {
    throw new Error(`invalid tar ${fieldName} field: ${JSON.stringify(raw)}`);
  }
  const value = Number.parseInt(raw, 8);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`out-of-range tar ${fieldName} field: ${JSON.stringify(raw)}`);
  }
  return value;
}

function parseSizeField(buf: Buffer, start: number, length: number): number {
  const first = buf[start];
  // GNU base-256 extension: high bit set on the first byte.
  if ((first & 0x80) !== 0) {
    if (first === 0xff) {
      throw new Error("negative tar entry size is not supported");
    }
    let value = first & 0x7f;
    for (let i = 1; i < length; i++) {
      value = value * 256 + buf[start + i];
      if (!Number.isSafeInteger(value)) {
        throw new Error("tar entry size exceeds safe integer range");
      }
    }
    return value;
  }
  return parseOctalField(buf, start, length, "size");
}

function verifyHeaderChecksum(buf: Buffer, offset: number): void {
  const expected = parseOctalField(buf, offset + 148, 8, "checksum");
  let sum = 0;
  for (let i = 0; i < BLOCK_SIZE; i++) {
    sum += i >= 148 && i < 156 ? 0x20 : buf[offset + i];
  }
  if (sum !== expected) {
    throw new Error(
      `corrupt tar header checksum (expected ${expected}, computed ${sum})`,
    );
  }
}

function parsePaxRecords(data: Buffer): PaxOverrides {
  const out: PaxOverrides = {};
  let pos = 0;
  while (pos < data.length) {
    const space = data.indexOf(0x20, pos);
    if (space === -1) {
      throw new Error("malformed PAX extended header record");
    }
    const lenStr = data.subarray(pos, space).toString("ascii");
    if (!/^[1-9][0-9]*$/.test(lenStr)) {
      throw new Error(`invalid PAX record length: ${JSON.stringify(lenStr)}`);
    }
    const recLen = Number.parseInt(lenStr, 10);
    if (recLen < space - pos + 2 || pos + recLen > data.length) {
      throw new Error("truncated PAX extended header record");
    }
    if (data[pos + recLen - 1] !== 0x0a) {
      throw new Error("PAX extended header record missing trailing newline");
    }
    const body = data.subarray(space + 1, pos + recLen - 1).toString("utf8");
    const eq = body.indexOf("=");
    if (eq === -1) {
      throw new Error("PAX extended header record missing '='");
    }
    const key = body.slice(0, eq);
    const val = body.slice(eq + 1);
    if (key === "path") {
      out.path = val;
    } else if (key === "size") {
      if (!/^[0-9]+$/.test(val)) {
        throw new Error(`invalid PAX size value: ${JSON.stringify(val)}`);
      }
      const parsed = Number.parseInt(val, 10);
      if (!Number.isSafeInteger(parsed)) {
        throw new Error("PAX size exceeds safe integer range");
      }
      out.size = parsed;
    }
    pos += recLen;
  }
  return out;
}

function validateAndNormalizePath(rawPath: string): string {
  if (rawPath.includes("\0")) {
    throw new Error("tar archive contains NUL byte in entry path");
  }
  if (rawPath.includes("\\")) {
    throw new Error(`tar archive contains backslash in entry path: ${rawPath}`);
  }
  if (rawPath.startsWith("/") || /^[A-Za-z]:/.test(rawPath)) {
    throw new Error(`tar archive contains absolute entry path: ${rawPath}`);
  }
  let normalized = rawPath;
  while (normalized.startsWith("./")) {
    normalized = normalized.slice(2);
  }
  const segments = normalized.split("/");
  if (segments.some((seg) => seg === "..")) {
    throw new Error(`tar archive contains path traversal entry: ${rawPath}`);
  }
  return normalized;
}

/** Decompress a `.tar.gz` archive with a strict output cap and return the exact
 * bytes of the single regular file at `expectedPath`. Rejects archives carrying
 * symlinks, hardlinks, traversal or absolute paths, corrupt headers, truncated
 * payloads, duplicate target entries, or missing `expectedPath`. */
export function extractSingleFileFromTarGz(
  archive: Buffer,
  expectedPath: string,
  maxUncompressedBytes = DEFAULT_MAX_ARCHIVE_BYTES,
): Buffer {
  const want = validateAndNormalizePath(expectedPath);
  const tarBuf = zlib.gunzipSync(archive, {
    maxOutputLength: maxUncompressedBytes,
  });

  let offset = 0;
  let pax: PaxOverrides | undefined;
  let gnuLongName: string | undefined;
  let matched: Buffer | undefined;

  while (offset + BLOCK_SIZE <= tarBuf.length) {
    if (isZeroBlock(tarBuf, offset)) {
      break;
    }
    verifyHeaderChecksum(tarBuf, offset);

    const typeflag = String.fromCharCode(tarBuf[offset + 156]);
    const headerSize = parseSizeField(tarBuf, offset + 124, 12);
    const dataStart = offset + BLOCK_SIZE;

    if (typeflag === "x") {
      const dataEnd = dataStart + headerSize;
      const nextOffset = dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in PAX extended header");
      }
      pax = parsePaxRecords(tarBuf.subarray(dataStart, dataEnd));
      offset = nextOffset;
      continue;
    }

    if (typeflag === "g") {
      const nextOffset = dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in PAX global header");
      }
      offset = nextOffset;
      continue;
    }

    if (typeflag === "L") {
      const dataEnd = dataStart + headerSize;
      const nextOffset = dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in GNU longname entry");
      }
      const raw = tarBuf.subarray(dataStart, dataEnd);
      const nul = raw.indexOf(0);
      gnuLongName = raw.subarray(0, nul === -1 ? raw.length : nul).toString("utf8");
      offset = nextOffset;
      continue;
    }

    if (typeflag === "K") {
      throw new Error("tar archive contains unsupported GNU longlink entry");
    }

    const entryPax = pax;
    const entryLongName = gnuLongName;
    pax = undefined;
    gnuLongName = undefined;

    const magic = readCString(tarBuf, offset + 257, 6);
    const name = readCString(tarBuf, offset, 100);
    const prefix = magic.startsWith("ustar")
      ? readCString(tarBuf, offset + 345, 155)
      : "";
    const rawPath =
      entryPax?.path ?? entryLongName ?? (prefix ? `${prefix}/${name}` : name);

    const entrySize = entryPax?.size ?? headerSize;
    const dataEnd = dataStart + entrySize;
    const nextOffset = dataStart + Math.ceil(entrySize / BLOCK_SIZE) * BLOCK_SIZE;
    if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
      throw new Error(`truncated tar archive at entry: ${rawPath}`);
    }

    if (typeflag === "1" || typeflag === "2") {
      throw new Error(`tar archive contains unsupported link entry: ${rawPath}`);
    }

    const normalizedPath = validateAndNormalizePath(rawPath);
    const isRegularFile = typeflag === "0" || typeflag === "\0";
    if (isRegularFile && normalizedPath === want) {
      if (matched !== undefined) {
        throw new Error(`tar archive contains duplicate entry: ${want}`);
      }
      matched = Buffer.from(tarBuf.subarray(dataStart, dataEnd));
    }

    offset = nextOffset;
  }

  if (offset < tarBuf.length && !isZeroBlock(tarBuf, offset)) {
    throw new Error("truncated tar archive header");
  }
  if (matched === undefined) {
    throw new Error(`extracted tarball missing expected ${want}`);
  }
  return matched;
}
