// Single shared timestamp formatter for the Dump Management +
// Forensic Archive surfaces. `YYYY-MM-DD HH:MM:SS` — 24-hour,
// sortable, locale-free. Joel asked for one clean format across
// the board so log lines, archive rows, and "last run" displays all
// look the same.

export function formatTs(input: string | Date | null | undefined): string {
  if (input == null) return '—';
  const d = typeof input === 'string' ? new Date(input) : input;
  if (Number.isNaN(d.getTime())) {
    // Invalid timestamp — render the raw string rather than "Invalid Date"
    // so the user can still see what the source had.
    return typeof input === 'string' ? input : '—';
  }
  const pad = (n: number) => String(n).padStart(2, '0');
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  );
}
