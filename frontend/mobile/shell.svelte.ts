// The phone shell's own state, for the page's parts that open it.
class Shell {
  /** The Settings sheet (security, sync and the account, about). */
  settingsOpen = $state(false);
}

export const shell = new Shell();
