# FileFlow Aurora — Design System v2

## Objectif

Le système visuel doit donner l’impression d’une application desktop premium, calme et accessible. Il évite l’esthétique « dashboard technique dense » au profit d’une hiérarchie forte, de grands textes et de surfaces respirantes.

## Palette

### Neutres

- `--bg: #f6f7fb`
- `--surface-1: rgba(255,255,255,.94)`
- `--surface-2: #f0f2f8`
- `--text-strong: #10121a`
- `--text: #202330`
- `--text-muted: #666d7f`
- `--border: #e1e5ef`

### Accent

- `--accent: #5b63f5`
- `--accent-hover: #4d54e4`
- `--violet: #7b5cf6`
- `--accent-soft: #eef0ff`
- `--accent-soft-2: #f5f3ff`

### États

- succès : `#1d9c69` ;
- avertissement : `#b47213` ;
- danger : `#cf5963`.

Le mode sombre possède des tokens dédiés et ne repose pas sur une simple inversion.

## Typographie

Pile utilisée :

```css
Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont,
"SF Pro Display", "SF Pro Text", "Segoe UI", sans-serif
```

Aucun téléchargement de police n’est requis.

### Échelle

- texte courant : 15 px ;
- petit texte : 12–13 px ;
- section : 24 px ;
- page : 30–48 px ;
- hero : 48–76 px.

Les titres utilisent une graisse élevée et un tracking négatif léger.

## Rayons

- contrôles : 10–13 px ;
- cartes : 17–20 px ;
- grandes surfaces : 24–30 px.

## Ombres

Les ombres sont faibles et larges. Elles servent à hiérarchiser, pas à simuler une interface flottante permanente.

- `--shadow-xs` : contrôle ;
- `--shadow-sm` : carte ;
- `--shadow-md` : hover / panneau important ;
- `--shadow-lg` : modal / onboarding.

## Composants globaux

### `.ff-button`

Bouton principal avec gradient indigo-violet, hauteur 46 px et zone de clic large.

Variantes :

- `.secondary` ;
- `.ghost`.

### `.ff-badge`

Badge arrondi pour états et compteurs.

Variantes :

- `.success` ;
- `.warning` ;
- `.accent`.

### `.ff-icon-badge`

Carré arrondi de 48 px pour donner une identité visuelle aux actions.

### `.ff-card` / `.ff-panel`

Surface de base cohérente avec bordure, rayon et ombre.

### `.ff-display`

Titre hero pour les écrans principaux.

### `.ff-title`

Titre de page secondaire.

### `.ff-subtitle`

Texte introductif avec largeur et hauteur de ligne optimisées.

## Interaction

- hover léger : translation de 1–3 px ;
- transitions 180 ms ;
- aucune animation indispensable à la compréhension ;
- `prefers-reduced-motion` neutralise les animations ;
- focus visible renforcé.

## Accessibilité et lisibilité

- cibles principales ≥ 44 px ;
- texte courant ≈ 15 px ;
- petits textes jamais utilisés pour une information critique ;
- état actif signalé par couleur + fond, pas uniquement couleur ;
- mode guidé conserve de grandes zones d’action ;
- zoom UI existant reste compatible avec le système.

## Règle d’or

> Le design peut être riche ; la décision demandée à l’utilisateur doit rester simple.
