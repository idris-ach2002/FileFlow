# Documentation FileFlow

Cette documentation décrit l’architecture du projet FileFlow **1.0.1** et sert de référence aux utilisateurs, développeurs et mainteneurs.

## Sommaire

1. [Architecture générale](01-architecture.md)
2. [Technologies et responsabilités](02-technologies.md)
3. [Algorithmes, implémentations et complexité](03-algorithms.md)
4. [Graphe de transformation vers PDF](04-pdf-transformation-graph.md)
5. [Fonctionnalités FileFlow](05-features.md)
6. [Interface utilisateur](06-ui.md)
7. [CI/CD et workflows](07-ci-cd.md)
8. [Difficultés techniques rencontrées](08-difficulties.md)
9. [Exécuter FileFlow : local ou workflow](09-execution.md)
10. [Installation générale](10-installation.md)

## Principes structurants

- desktop natif via Tauri ;
- frontend Angular/TypeScript ;
- backend Rust modulaire ;
- moteurs de conversion **system-managed** ;
- processus externes lancés directement sans interpolation shell ;
- concurrence bornée par profils de ressources ;
- staging des sorties avant finalisation ;
- builds natifs séparés par OS et architecture.
