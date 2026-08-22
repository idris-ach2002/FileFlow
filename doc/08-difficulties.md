# Difficultés techniques rencontrées

Cette page conserve les principaux incidents comme documentation d’ingénierie et base de non-régression.

| Difficulté | Symptôme | Cause | Correction |
| --- | --- | --- | --- |
| AppImage Linux et moteurs hôte | erreurs ABI/symboles alors que l’outil système fonctionne seul | variables de runtime AppImage héritées par les processus externes | suppression ciblée de `LD_LIBRARY_PATH`, `LD_PRELOAD`, variables Python/GTK/GStreamer/ImageMagick/Tesseract avant les moteurs système |
| PowerShell 5.1 | erreurs de parse | ambiguïté de `$variable:` et différences d’encodage | `${variable}:`, normalisation ASCII/CRLF et parser preflight |
| Alias Python Microsoft Store | `python3` pointe sur `WindowsApps` | alias prioritaire sans vrai runtime utilisable | rejet de `WindowsApps` et recherche des installations Python réelles |
| pipx sous Windows | outil installé mais non détecté | `.local\bin` absent du PATH | ajout au PATH courant/persistant |
| Installation de moteurs Windows | certains gestionnaires ne trouvent pas tous les outils | couverture variable de winget/choco/scoop | fallbacks vers installateurs officiels/archives portables + doctor |
| `install.env` fantôme | « déjà installé » après désinstallation | marqueur conservé sans EXE/entrée registre | vérifier installation réelle avant de faire confiance au marqueur |
| Nom de staging commençant par `.` | comportement incorrect d’outils Windows | convention temporaire Unix appliquée à Windows | nom temporaire Windows sans point initial |
| Chemins verbatim Windows `\\?\...` | qpdf transforme le chemin et renvoie `No such file or directory` | qpdf n’accepte pas cette représentation Win32 étendue | conserver les PathBuf internes, normaliser seulement au passage vers les CLI |
| Noms d’artefacts multi-architecture | collisions lors de l’agrégation release | Tauri produit des noms identiques | renommer avec le triple de target |
| Runtime embarqué | taille et fragilité importantes | tentative d’embarquer de nombreux runtimes | architecture system-managed + installation/doctor |
| Différences de plateformes | code correct localement mais échec sur une cible | filesystem, ABI, quoting, packaging | workflows natifs séparés + validation réelle |

## Méthode de résolution adoptée

1. reproduire sur l’OS réel ;
2. exécuter directement le moteur avec une commande minimale ;
3. déterminer si l’erreur vient du moteur, de l’environnement ou de FileFlow ;
4. corriger au niveau architectural le plus bas pertinent ;
5. ajouter un test ou invariant ;
6. relancer uniquement le workflow affecté lorsque possible.

## Exemple : qpdf et `\\?\`

Sur Windows, qpdf accepte :

```text
C:\Users\...\FileFlow\result.pdf
```

mais peut refuser :

```text
\\?\C:\Users\...\FileFlow\result.pdf
```

Rust/Tokio peut néanmoins utiliser le second en interne. La correction correcte est donc une adaptation **à la frontière du processus externe**, pas une réécriture globale de tous les `PathBuf`.
