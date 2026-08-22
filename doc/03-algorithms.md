# Algorithmes, implémentations et complexité

Les complexités ci-dessous couvrent **l’orchestration FileFlow**. Le coût interne d’un moteur comme FFmpeg, qpdf ou Tesseract dépend de ses propres algorithmes et des données.

## Détection et classification

Pour N entrées, FileFlow inspecte chaque asset et lui associe un format/famille.

- orchestration : **O(N)** ;
- mémoire : **O(N)** pour les métadonnées du lot ;
- le probing du contenu ajoute un coût d’I/O propre au format.

## Déduplication

L’executor utilise des ensembles/hash maps pour éviter des chemins dupliqués lors de la collecte ou de l’expansion.

- temps moyen : **O(N)** ;
- mémoire : **O(N)**.

## Graphe de capacités et planner

Le code du planner contient `BinaryHeap`. La recherche est donc documentée comme une recherche à priorité (famille Dijkstra/best-first selon les coûts effectivement calculés). La borne classique d’un Dijkstra avec tas binaire est **O((V + E) log V)** en temps et **O(V + E)** en mémoire.

Conceptuellement :
- **V** = formats/états ;
- **E** = capacités de conversion ;
- une suite de `ConversionStep` = chemin exécutable dans le graphe.

## Scheduling à ressources bornées

Chaque action acquiert un `ResourceProfile` avant de lancer le moteur.

Pour N jobs et une capacité P :
- admission/coordination : **O(N)** ;
- concurrence active : **≤ P** pour la classe de ressource ;
- état en attente : **O(N)** au pire.

Ce mécanisme évite qu’un lot lourd ne lance simultanément un nombre incontrôlé de processus CPU/RAM intensifs.

## Exécution asynchrone et batching

Les tâches indépendantes peuvent être regroupées dans des ensembles de tâches Tokio puis collectées.

- création/collecte : **O(N)** ;
- parallélisme réel : borné par le scheduler ;
- temps mur : dépend du coût de chaque moteur et des ressources disponibles.

## Annulation

`CancellationToken` permet de signaler l’arrêt d’un job sans dépendre de l’UI.

- test d’annulation : **O(1)** par point de contrôle ;
- le délai d’arrêt dépend du prochain point de contrôle et de la capacité à terminer/arrêter le processus enfant.

## Résolution de conflits de sortie

Le resolver applique `Increment`, `Skip`, `Replace` ou `Ask`.

Avec K noms déjà occupés successivement :
- vérifications : **O(K)** appels filesystem ;
- mémoire additionnelle : **O(1)** hors chaînes de chemins.

## Staging transactionnel

Une sortie est écrite vers un temporaire, validée puis promue vers le nom final.

- rename même filesystem : généralement opération de métadonnées ;
- fallback copy : **O(B)** où B est la taille du fichier ;
- suppression/cleanup : dépend du nombre d’intermédiaires.

## Pipeline Smart-to-PDF

1. détecter les entrées ;
2. développer éventuellement les archives ;
3. convertir chaque composant vers PDF ;
4. fusionner ;
5. appliquer les options (OCR, métadonnées, protection…) ;
6. valider ;
7. copier dans le staging de destination ;
8. valider le staging ;
9. finaliser.

Pour N composants :
- orchestration FileFlow : **O(N)** ;
- coût réel : **Σ conversion(i) + fusion + validations** ;
- espace disque temporaire : proportionnel à la somme des intermédiaires.

## Liste NUL pour img2pdf

Les chemins d’images peuvent contenir espaces et caractères spéciaux. La liste d’entrée est séparée par NUL plutôt que par espace/nouvelle ligne.

Pour S octets cumulés de chemins :
- construction : **O(S)** ;
- mémoire : **O(S)**.

## SHA-256 des artefacts

Le checksum est calculable en streaming.

Pour un artefact de B octets :
- temps : **O(B)** ;
- mémoire : **O(1)** en streaming.
