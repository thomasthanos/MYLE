// What the pages' API modules share for talking to the backend.
import { Channel } from "@tauri-apps/api/core";

/** A channel for a command's progress events, each handed to `onEvent`. */
export function channel<T>(onEvent: (event: T) => void): Channel<T> {
  const value = new Channel<T>();
  value.onmessage = onEvent;
  return value;
}
