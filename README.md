# pap

Site web pour le podcast Portes à Potes (PAP) — épisodes, recommandations culturelles, liens d’écoute modernes.

## Fonctionnalités principales
- Affichage dynamique des épisodes depuis un flux RSS (titre, date, description, image)
- Présentation sous forme de cartes modernes et responsives
- Ajout/suppression de recommandations culturelles liées à chaque épisode (persistées en JSON)
- Boutons d’écoute ronds et stylés (Apple, Spotify, Deezer, RSS/Acast) avec logos officiels
- Interface moderne, accessible, responsive

## Lancer le serveur

1. Installez [Rust](https://www.rust-lang.org/tools/install)
2. Compilez le projet :
   ```bash
   cargo build
   ```
3. Lancez le serveur :
   ```bash
   cargo run
   ```
4. Ouvrez [http://localhost:3000](http://localhost:3000)

## Dépôt & versionning
- Seul le code source est versionné (voir `.gitignore`)
- Les fichiers générés par `cargo build` (dossier `/target`) ne sont pas suivis

---

<details>
<summary>Ancienne description minimale</summary>

Site pour ep et reco pap

</details>
