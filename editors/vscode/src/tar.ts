// Minimal, dependency-free in-memory `.tar.gz` single-file extractor.
//
// Replaces `node-tar` for extracting the single `alint` binary from a
// SHA-256-verified release archive. Works entirely in memory via Node's
// built-in `node:zlib` and fails closed on disallowed typeflags, symlinks,
// hardlinks, sparse entries, path traversal, non-canonical/duplicate paths,
// corrupt headers, truncated archives, and trailing garbage after zero blocks.

import * as zlib from "node:zlib";

const BLOCK_SIZE = 512;

/** Default upper bound on uncompressed archive bytes (256 MiB). */
export const DEFAULT_MAX_ARCHIVE_BYTES = 256 * 1024 * 1024;

interface PaxOverrides {
  path?: string;
  size?: number;
}

function isZeroBlock(buf: Buffer, offset: number): boolean {
  if (offset + BLOCK_SIZE > buf.length) {
    return false;
  }
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

function parseOctalField(
  buf: Buffer,
  start: number,
  length: number,
  fieldName: string,
): number {
  const raw = readCString(buf, start, length).replace(/^ +| +$/g, "");
  if (raw.length === 0) {
    return 0;
  }
  if (!/^[0-7]+$/.test(raw)) {
    throw new Error(`invalid tar ${fieldName} field: ${JSON.stringify(raw)}`);
  }
  const value = Number.parseInt(raw, 8);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(
      `out-of-range tar ${fieldName} field: ${JSON.stringify(raw)}`,
    );
  }
  return value;
}

function parseSizeField(buf: Buffer, start: number, length: number): number {
  const first = buf[start];
  // GNU base-256 extension: high bit set on the first byte; bit 6 indicates
  // two's-complement negative numbers.
  if ((first & 0x80) !== 0) {
    if ((first & 0x40) !== 0) {
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

function parsePaxRecords(data: Buffer, isGlobal: boolean): PaxOverrides {
  const out: PaxOverrides = {};
  let pos = 0;
  while (pos < data.length) {
    const space = data.indexOf(0x20, pos);
    if (space === -1 || space === pos) {
      throw new Error("malformed PAX extended header record");
    }
    // Validate raw ASCII digits directly (avoid Buffer.toString("ascii") which
    // strips bit 7 from non-ASCII bytes).
    if (data[pos] < 0x31 || data[pos] > 0x39) {
      throw new Error("invalid PAX record length");
    }
    for (let i = pos + 1; i < space; i++) {
      if (data[i] < 0x30 || data[i] > 0x39) {
        throw new Error("invalid PAX record length");
      }
    }
    const recLen = Number.parseInt(
      data.subarray(pos, space).toString("latin1"),
      10,
    );
    if (
      !Number.isSafeInteger(recLen) ||
      recLen < space - pos + 2 ||
      pos + recLen > data.length
    ) {
      throw new Error("truncated PAX extended header record");
    }
    if (data[pos + recLen - 1] !== 0x0a) {
      throw new Error("PAX extended header record missing trailing newline");
    }
    const body = data.subarray(space + 1, pos + recLen - 1).toString("utf8");
    const eq = body.indexOf("=");
    if (eq <= 0) {
      throw new Error("PAX extended header record missing key or '='");
    }
    const key = body.slice(0, eq);
    const val = body.slice(eq + 1);
    if (
      key === "linkpath" ||
      key.startsWith("GNU.sparse.") ||
      key === "SCHILY.realsize"
    ) {
      throw new Error(`tar archive contains unsupported PAX key: ${key}`);
    }
    if (isGlobal && (key === "path" || key === "size")) {
      throw new Error(
        `tar archive contains unsupported PAX global key: ${key}`,
      );
    }
    if (key === "path") {
      if (val.length === 0) {
        throw new Error("PAX extended header contains empty path override");
      }
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

function validateAndNormalizePath(rawPath: string, isDirectory: boolean): string {
  if (rawPath.length === 0) {
    throw new Error("tar archive contains empty entry path");
  }
  if (rawPath.includes("\0")) {
    throw new Error("tar archive contains NUL byte in entry path");
  }
  if (rawPath.includes("\\")) {
    throw new Error(`tar archive contains backslash in entry path: ${rawPath}`);
  }
  for (let i = 0; i < rawPath.length; i++) {
    const code = rawPath.charCodeAt(i);
    if (code < 0x20 || code === 0x7f) {
      throw new Error(`tar archive contains control char in entry path: ${rawPath}`);
    }
  }
  if (rawPath.startsWith("/") || /^[A-Za-z]:/.test(rawPath)) {
    throw new Error(`tar archive contains absolute entry path: ${rawPath}`);
  }

  let normalized = rawPath;
  while (normalized.startsWith("./")) {
    normalized = normalized.slice(2);
  }
  if (isDirectory && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  // Re-check after stripping `./` so `.//etc/passwd` and `./C:/...` cannot
  // bypass absolute/drive checks.
  if (normalized.length === 0) {
    if (isDirectory && (rawPath === "." || rawPath === "./")) {
      return ".";
    }
    throw new Error(`tar archive contains empty normalized path: ${rawPath}`);
  }
  if (normalized.startsWith("/") || /^[A-Za-z]:/.test(normalized)) {
    throw new Error(`tar archive contains absolute entry path: ${rawPath}`);
  }

  const segments = normalized.split("/");
  for (const seg of segments) {
    if (seg === "..") {
      throw new Error(`tar archive contains path traversal entry: ${rawPath}`);
    }
    if (seg === "." || seg.length === 0) {
      throw new Error(`tar archive contains non-canonical entry path: ${rawPath}`);
    }
  }
  return normalized;
}

/** Decompress a `.tar.gz` archive with a strict output cap and return the exact
 * bytes of the single regular file at `expectedPath`. Fails closed on
 * disallowed typeflags, symlinks/hardlinks, sparse files, traversal/absolute/
 * non-canonical paths, corrupt headers, dangling metadata, truncated payloads,
 * duplicate targets, and trailing non-zero bytes after end-of-archive. */
export function extractSingleFileFromTarGz(
  archive: Buffer,
  expectedPath: string,
  maxUncompressedBytes = DEFAULT_MAX_ARCHIVE_BYTES,
): Buffer {
  const want = validateAndNormalizePath(expectedPath, false);
  const wantLower = want.toLowerCase();
  const tarBuf = zlib.gunzipSync(archive, {
    maxOutputLength: maxUncompressedBytes,
  });

  if (tarBuf.length === 0 || tarBuf.length % BLOCK_SIZE !== 0) {
    throw new Error("truncated tar archive header");
  }

  let offset = 0;
  let pax: PaxOverrides | undefined;
  let gnuLongName: string | undefined;
  let matched: Buffer | undefined;
  let sawZeroBlock = false;

  while (offset + BLOCK_SIZE <= tarBuf.length) {
    if (isZeroBlock(tarBuf, offset)) {
      if (pax !== undefined || gnuLongName !== undefined) {
        throw new Error("tar archive contains dangling extended header before end-of-archive");
      }
      sawZeroBlock = true;
      offset += BLOCK_SIZE;
      break;
    }
    verifyHeaderChecksum(tarBuf, offset);

    const typeflag = String.fromCharCode(tarBuf[offset + 156]);
    const headerSize = parseSizeField(tarBuf, offset + 124, 12);
    const dataStart = offset + BLOCK_SIZE;

    if (typeflag === "x") {
      if (pax !== undefined || gnuLongName !== undefined) {
        throw new Error("tar archive contains stacked PAX/GNU extended headers");
      }
      const dataEnd = dataStart + headerSize;
      const nextOffset =
        dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in PAX extended header");
      }
      pax = parsePaxRecords(tarBuf.subarray(dataStart, dataEnd), false);
      offset = nextOffset;
      continue;
    }

    if (typeflag === "g") {
      if (pax !== undefined || gnuLongName !== undefined) {
        throw new Error("tar archive contains stacked PAX/GNU extended headers");
      }
      const dataEnd = dataStart + headerSize;
      const nextOffset =
        dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in PAX global header");
      }
      parsePaxRecords(tarBuf.subarray(dataStart, dataEnd), true);
      offset = nextOffset;
      continue;
    }

    if (typeflag === "L") {
      if (pax !== undefined || gnuLongName !== undefined) {
        throw new Error("tar archive contains stacked PAX/GNU extended headers");
      }
      const dataEnd = dataStart + headerSize;
      const nextOffset =
        dataStart + Math.ceil(headerSize / BLOCK_SIZE) * BLOCK_SIZE;
      if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
        throw new Error("truncated tar archive in GNU longname entry");
      }
      const raw = tarBuf.subarray(dataStart, dataEnd);
      const nul = raw.indexOf(0);
      if (nul !== -1) {
        for (let i = nul + 1; i < raw.length; i++) {
          if (raw[i] !== 0) {
            throw new Error("tar archive contains embedded NUL in GNU longname");
          }
        }
      }
      gnuLongName = raw.subarray(0, nul === -1 ? raw.length : nul).toString("utf8");
      offset = nextOffset;
      continue;
    }

    const entryPax = pax;
    const entryLongName = gnuLongName;
    pax = undefined;
    gnuLongName = undefined;

    if (typeflag === "1" || typeflag === "2" || typeflag === "K") {
      throw new Error(`tar archive contains unsupported link entry (typeflag ${JSON.stringify(typeflag)})`);
    }
    const isRegularFile = typeflag === "0" || typeflag === "\0";
    const isDirectory = typeflag === "5";
    if (!isRegularFile && !isDirectory) {
      throw new Error(
        `tar archive contains unsupported entry typeflag: ${JSON.stringify(typeflag)}`,
      );
    }

    // Strict POSIX USTAR magic (`"ustar\0"`, byte 262 === 0x00). GNU tar writes
    // `"ustar  \0"`, where bytes 345..499 are GNU metadata rather than a prefix.
    const isPosixUstar =
      tarBuf.subarray(offset + 257, offset + 263).equals(Buffer.from("ustar\0", "ascii"));
    const name = readCString(tarBuf, offset, 100);
    const prefix = isPosixUstar ? readCString(tarBuf, offset + 345, 155) : "";
    const rawPath =
      entryPax?.path ?? entryLongName ?? (prefix ? `${prefix}/${name}` : name);

    const entrySize = entryPax?.size ?? headerSize;
    if (isDirectory && entrySize !== 0) {
      throw new Error(`tar archive directory entry has non-zero size: ${rawPath}`);
    }
    const dataEnd = dataStart + entrySize;
    const nextOffset =
      dataStart + Math.ceil(entrySize / BLOCK_SIZE) * BLOCK_SIZE;
    if (dataEnd > tarBuf.length || nextOffset > tarBuf.length) {
      throw new Error(`truncated tar archive at entry: ${rawPath}`);
    }

    const normalizedPath = validateAndNormalizePath(rawPath, isDirectory);
    if (isRegularFile) {
      if (normalizedPath === want) {
        if (matched !== undefined) {
          throw new Error(`tar archive contains duplicate entry: ${want}`);
        }
        matched = Buffer.from(tarBuf.subarray(dataStart, dataEnd));
      } else if (normalizedPath.toLowerCase() === wantLower) {
        throw new Error(
          `tar archive contains case-colliding duplicate entry: ${normalizedPath}`,
        );
      }
    }

    offset = nextOffset;
  }

  if (pax !== undefined || gnuLongName !== undefined) {
    throw new Error("tar archive contains dangling extended header at EOF");
  }
  if (sawZeroBlock) {
    // Reject trailing non-zero bytes/entries smuggled after a zero block.
    for (let i = offset; i < tarBuf.length; i++) {
      if (tarBuf[i] !== 0) {
        throw new Error("tar archive contains trailing data after end-of-archive");
      }
    }
  }
  if (matched === undefined) {
    throw new Error(`extracted tarball missing expected ${want}`);
  }
  return matched;
}
