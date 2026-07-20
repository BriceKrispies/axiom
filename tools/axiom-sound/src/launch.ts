// Cross-platform "open this file with the OS default app", mirroring the
// three-arm convention in tools/axiom-serve/src/main.rs `open_browser`:
//   Windows -> cmd /C start "" <target>   (empty title arg is required)
//   macOS   -> open <target>
//   else    -> xdg-open <target>
// Failure is a non-fatal warning, never a hard error.

import { spawn } from 'node:child_process';

export function openWithOs(target: string, warn: (message: string) => void): void {
  const [command, args] =
    process.platform === 'win32'
      ? (['cmd', ['/C', 'start', '', target]] as const)
      : process.platform === 'darwin'
        ? (['open', [target]] as const)
        : (['xdg-open', [target]] as const);

  try {
    const child = spawn(command, [...args], { stdio: 'ignore', detached: true });
    child.on('error', (err) => warn(`could not open ${target} (${err.message})`));
    child.unref();
  } catch (err) {
    warn(`could not open ${target} (${err instanceof Error ? err.message : String(err)})`);
  }
}
