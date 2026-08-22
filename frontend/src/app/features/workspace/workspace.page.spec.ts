import { describe, expect, it } from 'vitest';
import { resolveWorkspaceDestination } from './workspace.page';

describe('workspace destination resolution', () => {
  it('always gives an explicitly selected directory priority', () => {
    expect(resolveWorkspaceDestination('choose', '/exports/client', '/old/default')).toEqual({
      destination: 'customFolder',
      customDirectory: '/exports/client',
    });
  });

  it('honors the same-folder preference instead of the guided directory', () => {
    expect(resolveWorkspaceDestination('same', null, '/old/default')).toEqual({
      destination: 'sameFolder',
      customDirectory: null,
    });
  });

  it('uses the configured FileFlow directory only for guided subfolder output', () => {
    expect(resolveWorkspaceDestination('subfolder', null, '/exports/fileflow')).toEqual({
      destination: 'customFolder',
      customDirectory: '/exports/fileflow',
    });
  });
});
