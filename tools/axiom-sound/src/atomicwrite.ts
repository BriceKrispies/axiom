// Atomic file publication: write to a temp file in the destination directory,
// fsync it, then rename over the target. A rename within a directory is atomic
// on POSIX and Windows, so a reader never observes a partial file and a failed
// write never leaves a truncated destination.

import {
  closeSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  renameSync,
  rmSync,
  writeSync,
} from 'node:fs';
import { basename, dirname, join } from 'node:path';

/**
 * Atomically write `data` to `destPath`. Creates parent directories as needed.
 * Refuses to publish over an existing symlink (which could redirect the write
 * outside the intended tree). Throws on failure without leaving a partial
 * destination; the temp file is always cleaned up.
 */
export function atomicWrite(destPath: string, data: Uint8Array | string): void {
  const dir = dirname(destPath);
  mkdirSync(dir, { recursive: true });
  refuseSymlink(destPath);

  const bytes = typeof data === 'string' ? Buffer.from(data, 'utf8') : Buffer.from(data);
  // Unique-enough temp name in the SAME directory (so rename stays on one fs).
  const tmp = join(dir, `.${basename(destPath)}.tmp-${process.pid}-${counter()}`);

  let fd: number | undefined;
  try {
    fd = openSync(tmp, 'wx');
    writeSync(fd, bytes);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    renameSync(tmp, destPath);
  } catch (err) {
    if (fd !== undefined) {
      try {
        closeSync(fd);
      } catch {
        /* already closing on error */
      }
    }
    rmSync(tmp, { force: true });
    throw err;
  }
}

function refuseSymlink(destPath: string): void {
  try {
    const st = lstatSync(destPath);
    if (st.isSymbolicLink()) {
      throw new Error(`refusing to write through a symlink: ${destPath}`);
    }
  } catch (err) {
    // ENOENT is fine (target does not exist yet); rethrow the symlink refusal.
    if (err instanceof Error && err.message.startsWith('refusing to write')) {
      throw err;
    }
  }
}

let seq = 0;
function counter(): number {
  seq += 1;
  return seq;
}
