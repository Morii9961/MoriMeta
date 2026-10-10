// SPDX-License-Identifier: GPL-3.0-or-later
// The scan strip's time left (SCREEN_SPEC 1#large): the reading rate so far, once there is enough
// of it to be worth showing.

/** Seconds left, or null while the estimate would be noise (under 3 s or 20 files read so far). */
export function scanSecondsLeft(start: { ms: number; done: number }, now: number, done: number, total: number): number | null {
  const read = done - start.done
  const elapsed = (now - start.ms) / 1000
  if (read < 20 || elapsed < 3 || done >= total) return null
  return Math.ceil((elapsed / read) * (total - done))
}

/** Rounded the way a person reads it: seconds under a minute, then whole minutes. */
export function etaParts(sec: number): { unit: 's' | 'min'; n: number } {
  if (sec < 60) return { unit: 's', n: Math.max(5, Math.ceil(sec / 5) * 5) }
  return { unit: 'min', n: Math.ceil(sec / 60) }
}
