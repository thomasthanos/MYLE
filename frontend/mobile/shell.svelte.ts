// The phone shell's own state, for the page's parts that open it.
class Shell {
  /** The account sheet (sign in, sync, sign out). */
  accountOpen = $state(false);
}

export const shell = new Shell();
