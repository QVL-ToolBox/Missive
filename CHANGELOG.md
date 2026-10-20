# Changelog

Toutes les modifications notables de ce projet sont documentées dans ce fichier.

Le format est basé sur [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/),
et ce projet adhère au [Semantic Versioning](https://semver.org/lang/fr/).

## [2.0.2] - 2026-10-20

### Modifié

- Publication sous licence MIT ; chaque release est reflétée sur https://github.com/QVL-ToolBox/Missive (instantané taggé), qui devient l'URL d'installation documentée.

## [2.0.1] - 2026-10-15

### Corrigé

- Dépendance `rustls` montée en 0.23.46 (vulnérabilité signalée par osv-scanner) (#161).

## [2.0.0] - 2026-10-13

### Modifié (breaking change)

- Missive devient une crate lib : `Mailer::new(&MailConfig, Environment)` et `Mailer::send(to, subject, Body)`, erreurs typées `MailError` sans erreur source ni texte du serveur SMTP (#103).
- Configuration par la struct désérialisable `MailConfig` (`#[non_exhaustive]`, constructeur `MailConfig::new` + `with_*`) au lieu des variables d'environnement ; `port` non nul.
- Mode TLS explicite (`tls`, `starttls`, `none`) ; `none` refusé en production et vers un hôte non loopback ; identifiants SMTP partiels refusés au lieu d'être ignorés.
- Plus de pool de connexions : `Mailer::new` ne requiert pas de runtime, une connexion SMTP par envoi.
- Timeout configurable `timeout_secs` (non nul, défaut 15 s, `MailConfig::with_timeout`) qui borne toute l'opération d'envoi dans tous les modes TLS, y compris face à un serveur muet ; à l'expiration, `Send { permanent: false, code: None }` et log `warn` avec `timed_out = true` ; une expiration après `DATA` ne garantit pas la non-transmission (risque de doublon en cas de nouvelle tentative). `send` exige un runtime Tokio avec le driver de temps.
- Hotfix du service v1 (en production sur le port 8184 jusqu'à #122) : à partir du tag `v1.0.0`, construit, déployé (`ch-deploy-bin missive`) et tagué à la main, `version.mjs` refusant une version inférieure à `v2.0.0`.

### Supprimé

- Serveur HTTP (`POST /v1/send`, `GET /health`), `INTERNAL_API_SECRET`, `PORT`, `BIND_ADDR`, `ALLOW_EXTERNAL_BIND`, canal `push`, lecture de `.env`.
- Déploiement : la pipeline passe en `deployMode: none`.

## [1.0.0] - 2026-10-02

### Ajouté

- Première version publiée sur Azure DevOps (historique GitHub remis à plat).
