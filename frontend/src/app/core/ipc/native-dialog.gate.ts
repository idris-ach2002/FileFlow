import { signal } from '@angular/core';

/** Ensures that macOS/Windows/Linux never receive overlapping native dialogs. */
export class NativeDialogGate {
  readonly busy = signal(false);

  async run<T>(operation: () => Promise<T>): Promise<T | null> {
    if (this.busy()) return null;
    this.busy.set(true);
    try {
      return await operation();
    } finally {
      this.busy.set(false);
    }
  }
}
