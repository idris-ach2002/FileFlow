# Graphe de transformation vers PDF

Le mode Smart-to-PDF fait converger plusieurs familles de fichiers vers un PDF validé.

```mermaid
flowchart LR
    INPUT["Entrées"] --> DETECT["Détection format / famille"]

    DETECT --> IMG["Images"]
    DETECT --> PDF["PDF existant"]
    DETECT --> OFFICE["Office"]
    DETECT --> TEXT["Document / texte"]
    DETECT --> ARCH["Archive"]

    IMG --> IMG2PDF["img2pdf / pipeline image"]
    OFFICE --> LO["LibreOffice"]
    TEXT --> PANDOC["Pandoc"]
    ARCH --> EXTRACT["7-Zip / extraction"]
    EXTRACT --> AGAIN["Nouvelle détection"]

    AGAIN --> IMG2PDF
    AGAIN --> LO
    AGAIN --> PANDOC
    AGAIN --> PDF

    IMG2PDF --> COMPONENT["PDF composant"]
    LO --> COMPONENT
    PANDOC --> COMPONENT
    PDF --> COMPONENT

    COMPONENT --> MERGE["qpdf — assemblage"]
    MERGE --> OPTIONS{"Options"}
    OPTIONS -->|OCR| OCR["OCRmyPDF / Tesseract"]
    OPTIONS -->|Métadonnées| META["ExifTool"]
    OPTIONS -->|Protection| PROTECT["qpdf"]
    OPTIONS -->|aucune| VALIDATE["Validation"]

    OCR --> VALIDATE
    META --> VALIDATE
    PROTECT --> VALIDATE

    VALIDATE --> STRUCT["%PDF- + qpdf --check"]
    STRUCT --> STAGE["OutputResolver staging"]
    STAGE --> CHECK2["Validation finale"]
    CHECK2 --> FINAL["PDF final"]
```

## Validation

FileFlow contrôle d’abord que le fichier :
- existe ;
- possède une taille minimale ;
- commence par la signature `%PDF-`.

Lorsque qpdf est disponible, FileFlow ajoute `qpdf --warning-exit-0 --check`.

## Pourquoi une seconde validation ?

Le pipeline valide le PDF de travail puis le fichier copié vers le staging final. La deuxième validation porte donc sur le fichier réellement destiné à être promu vers le résultat utilisateur.

## Workspace temporaire

Les composants et fichiers techniques restent dans un workspace de job. Le résultat est promu en dehors avant que le workspace soit nettoyé.
