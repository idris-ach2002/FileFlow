import { describe, expect, it } from 'vitest';
import { classifyUpdateFailure, friendlyUpdateError } from './update-errors';

describe('updater error classification', () => {
  it('distinguishes an unpublished manifest from a network outage', () => {
    const message = 'Could not fetch a valid release JSON from the remote';
    expect(classifyUpdateFailure(message)).toBe('not-published');
    expect(friendlyUpdateError(message)).toContain('latest.json');
  });

  it('recognizes HTTP 404 as an unpublished release', () => {
    expect(classifyUpdateFailure('HTTP status 404 Not Found')).toBe('not-published');
  });

  it('recognizes an actual transport failure', () => {
    expect(classifyUpdateFailure('error sending request for url')).toBe('network');
  });

  it('keeps configuration and signature failures separate', () => {
    expect(classifyUpdateFailure('Updater does not have any endpoints set')).toBe('configuration');
    expect(classifyUpdateFailure('signature verification failed')).toBe('signature');
  });
});
