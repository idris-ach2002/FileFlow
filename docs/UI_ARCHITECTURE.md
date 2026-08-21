# FileFlow — architecture UI v2

Cette architecture sépare volontairement deux expériences :

- **mode simple** : pour une personne non technique, avec une seule décision importante à la fois ;
- **mode avancé** : pour un utilisateur du domaine qui veut voir l’intégralité des capacités de FileFlow.

L’objectif n’est pas de retirer des fonctions. L’objectif est de **retirer la complexité de l’écran principal**.

## 1. Navigation principale

La barre latérale ne contient plus les outils experts directement.

### Essentiel

1. **Accueil** — point d’entrée guidé ;
2. **Fichiers** — workspace courant ;
3. **Historique** — traitements précédents ;
4. **Favoris** — actions souvent utilisées.

### Plus

5. **Outils avancés** — catalogue complet et outils experts ;
6. **Aide & guides** ;
7. **Paramètres**.

Le profil, l’état local et l’état du runtime restent dans la partie basse de la barre latérale.

## 2. Parcours simple

```text
Accueil
  ↓
Ajouter un fichier / dossier
  ↓
Détection automatique
  ↓
Propositions pertinentes
  ↓
Choisir une action
  ↓
Réglages essentiels seulement
  ↓
Traitement
  ↓
Résultat
```

### Accueil

L’accueil est construit autour d’une grande zone de dépôt. Il ne présente pas un catalogue de dizaines de fonctions.

Les seuls raccourcis visibles sont les grandes familles :

- PDF & documents ;
- Photos & images ;
- Audio & vidéo ;
- Archives.

Le reste est accessible après détection du fichier, via la recherche `⌘K / Ctrl+K`, les favoris ou l’espace avancé.

### Workspace

Le workspace conserve son mode guidé existant :

1. fichiers ajoutés ;
2. action recommandée ;
3. réglages utiles ;
4. exécution ;
5. résultat.

Les contrôles techniques restent masqués lorsque `beginnerMode` est activé.

## 3. Favoris

Nouvelle route : `/favorites`.

Cette page contient uniquement les actions marquées par l’utilisateur. Elle sert de raccourci pour les usages répétitifs sans remettre un grand catalogue sur l’accueil.

## 4. Espace avancé

Nouvelle route : `/advanced`.

Il s’agit du tableau de bord expert. Il regroupe :

- **Formats & possibilités** ;
- **Organiser & nettoyer** ;
- **Automatisations** ;
- **Moteurs & diagnostic** ;
- **Aide technique** ;
- **Préférences expertes**.

Il contient également un **catalogue complet de toutes les actions** avec :

- recherche ;
- état d’exécution local ;
- favori ;
- lancement direct ;
- nombre d’actions prêtes ;
- nombre de profils de format ;
- nombre de moteurs disponibles.

L’espace avancé est volontairement accessible mais n’est jamais injecté dans le flux simple de l’accueil.

## 5. Formats & possibilités

La route `/formats` reste une vue experte. Elle expose :

- extensions ;
- famille ;
- aperçu ;
- lecture / écriture ;
- métadonnées ;
- miniature ;
- extraction ;
- streaming ;
- capacités techniques ;
- actions exécutables ;
- conversions cibles ;
- compressions cibles.

Le redesign augmente la lisibilité sans réduire l’information.

## 6. Authentification et onboarding

L’authentification existante est conservée.

Le parcours reste :

1. créer un profil local / se connecter ;
2. choisir le dossier de sortie ;
3. choisir le niveau d’assistance ;
4. entrer dans FileFlow.

Le design system v2 augmente les tailles de texte, les champs et les surfaces sans modifier les flux de sécurité existants.

## 7. Paramètres

Les paramètres conservent les sections existantes :

- Utilisation ;
- Affichage ;
- Fichiers & stockage ;
- Sécurité ;
- Compte & profil ;
- Performances ;
- Moteurs locaux.

Les catégories techniques restent regroupées dans les sections avancées plutôt que d’être exposées sur l’accueil.

## 8. Règles UX obligatoires

1. Une décision importante par écran ou par zone majeure.
2. Le fichier arrive avant les options techniques.
3. Les réglages avancés sont révélés sur demande.
4. Les libellés utilisateur décrivent un résultat, pas un moteur.
5. Les erreurs normales sont résumées ; les détails techniques sont secondaires.
6. Les grandes zones cliquables sont préférées aux petites icônes seules.
7. Le texte principal ne doit pas devenir microscopique pour gagner de la place.
8. L’utilisateur expert ne perd aucune fonction : elles vivent dans l’espace avancé.
