// What went wrong, as text, for every page: a command's error arrives as a string, a thrown one as an Error.
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
