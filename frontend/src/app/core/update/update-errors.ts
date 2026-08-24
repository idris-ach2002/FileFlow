export type UpdateFailureKind = 'configuration' | 'not-published' | 'network' | 'signature' | 'other';

export function classifyUpdateFailure(message: string): UpdateFailureKind {
  if (/does not have any endpoints|no updater endpoints?|endpoint.*not configured|pubkey|public key.*(?:missing|empty|configured)/i.test(message)) {
    return 'configuration';
  }
  if (/\b404\b|not found|could not fetch a valid release json|release json.*(?:invalid|missing)|(?:parse|deserialize).*(?:release|json)/i.test(message)) {
    return 'not-published';
  }
  if (/signature|verify|verification/i.test(message)) {
    return 'signature';
  }
  if (/network|dns|connect|timed? ?out|offline|error sending request|request failed|connection/i.test(message)) {
    return 'network';
  }
  return 'other';
}

export function friendlyUpdateError(message: string): string {
  switch (classifyUpdateFailure(message)) {
    case 'configuration':
      return 'Updater non configuré pour ce build. Initialisez la signature puis reconstruisez FileFlow.';
    case 'not-published':
      return 'Aucune release stable n’est encore publiée, ou son manifeste latest.json est invalide.';
    case 'network':
      return 'Impossible de joindre le service de mise à jour. Vérifiez la connexion puis réessayez.';
    case 'signature':
      return 'La signature de cette mise à jour n’a pas pu être vérifiée. L’installation a été bloquée.';
    default:
      return `La vérification a échoué : ${message}`;
  }
}
