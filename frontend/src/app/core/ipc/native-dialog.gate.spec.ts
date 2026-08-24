import { describe, expect, it, vi } from 'vitest';
import { NativeDialogGate } from './native-dialog.gate';

describe('NativeDialogGate', () => {
  it('allows only one native dialog at a time', async () => {
    let finish!: (value: string | null) => void;
    const firstDialog = new Promise<string | null>((resolve) => { finish = resolve; });
    const operation = vi.fn().mockReturnValue(firstDialog);
    const gate = new NativeDialogGate();

    const first = gate.run(operation);
    const overlapping = await gate.run(vi.fn().mockResolvedValue('/tmp/second'));

    expect(gate.busy()).toBe(true);
    expect(operation).toHaveBeenCalledOnce();
    expect(overlapping).toBeNull();

    finish('/tmp/first');
    await expect(first).resolves.toBe('/tmp/first');
    expect(gate.busy()).toBe(false);
  });

  it('always releases the gate after a dialog error', async () => {
    const gate = new NativeDialogGate();

    await expect(gate.run(() => Promise.reject(new Error('dialog failed')))).rejects.toThrow('dialog failed');
    expect(gate.busy()).toBe(false);
    await expect(gate.run(() => Promise.resolve('/tmp/retry'))).resolves.toBe('/tmp/retry');
  });
});
