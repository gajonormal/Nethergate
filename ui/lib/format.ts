/** UI formatting stays consistent without changing the user's local timezone. */
export const errorText = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);

export function formatDateTime(
  value: number,
  options?: Intl.DateTimeFormatOptions,
) {
  return new Date(value).toLocaleString("pt-PT", options);
}
export function formatTime(
  value = Date.now(),
  options?: Intl.DateTimeFormatOptions,
) {
  return new Date(value).toLocaleTimeString("pt-PT", options);
}
export function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  const unit = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${(value / 1024 ** unit).toLocaleString("pt-PT", { maximumFractionDigits: 1 })} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
