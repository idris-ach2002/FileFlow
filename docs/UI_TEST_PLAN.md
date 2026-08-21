# Validation UI FileFlow v2

## Contrôles rapides

```bash
pnpm run frontend:build
pnpm run frontend:test
pnpm run verify
```

Puis lancer l’application :

```bash
pnpm run dev
```

## Scénarios à tester

1. Créer un profil local puis terminer l’onboarding.
2. Vérifier que l’accueil affiche en priorité la zone de dépôt.
3. Ajouter un PDF et vérifier que le workspace guidé propose des actions pertinentes.
4. Ajouter une action aux favoris et vérifier `/favorites`.
5. Ouvrir `/advanced`, rechercher une action et la lancer.
6. Ouvrir `/formats` et vérifier la lisibilité à 100 % et 120 %.
7. Tester Paramètres : thème clair/sombre, zoom, densité et mode guidé.
8. Tester les fenêtres à largeur réduite.
9. Vérifier `⌘K` sur macOS et `Ctrl+K` sur Windows/Linux.
10. Vérifier que les workflows FileFlow CI + natifs restent verts.

## Critères de validation utilisateur non technique

Le testeur doit pouvoir, sans explication préalable :

- ajouter un fichier ;
- comprendre les actions proposées ;
- lancer une transformation ;
- retrouver le résultat ;
- revenir à l’accueil ;
- retrouver une action favorite.

Il n’a pas besoin de comprendre « moteur », « codec », « runtime », « adapter » ou « pipeline » pour accomplir ces tâches.
