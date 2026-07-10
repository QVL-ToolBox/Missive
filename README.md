# Missive

Mailer transactionnel générique, auto-hébergeable. Missive expose une petite API HTTP et achemine un message vers un serveur SMTP, sans logique métier. Le service est volontairement « bête » : l'appelant fournit un sujet et un corps déjà rendus, Missive ne fait aucun templating ni personnalisation. Seul le canal `email` est opérationnel ; le canal `push` est réservé et répond `501 Not Implemented`.

Stack : Rust (stable) + Axum + lettre.

> Intégration dans l'écosystème QVL : voir `Tools/ENVCONFIG.md`. Ce README couvre uniquement l'auto-hébergement autonome via un fichier `.env`.

## Démarrage rapide

Prérequis : Rust stable (`rustc` / `cargo`, via [rustup](https://rustup.rs)).

```
git clone <url-du-repo> missive
cd missive
```

Créer le fichier de configuration à partir du modèle, puis l'éditer :

Linux / macOS :

```
cp .env.example .env
```

Windows (PowerShell) :

```
Copy-Item .env.example .env
```

Éditer `.env` et renseigner au minimum `SMTP_HOST`, `MAIL_FROM` et `INTERNAL_API_SECRET` (voir [Configuration](#configuration)). Compiler puis lancer le binaire :

```
cargo build --release
```

Linux / macOS :

```
./target/release/missive
```

Windows (PowerShell) :

```
.\target\release\missive.exe
```

Missive lit automatiquement le fichier `.env` du répertoire courant au démarrage. Vérifier que le service répond :

```
curl http://127.0.0.1:8184/health
```

Réponse attendue : `{"status":"ok"}`.

## Configuration

Toute la configuration passe par des variables d'environnement (ou le fichier `.env`). Une valeur vide ou composée uniquement d'espaces est traitée comme absente. Toute variable requise manquante, ou toute valeur invalide, provoque un refus de démarrage immédiat (fail-fast) avec un log d'erreur.

| Variable | Rôle | Requis | Défaut |
|---|---|---|---|
| `PORT` | Port d'écoute HTTP. Doit être > 0. | Optionnel | `8184` |
| `BIND_ADDR` | Adresse d'écoute HTTP. Une adresse non-loopback exige `ALLOW_EXTERNAL_BIND=true`. | Optionnel | `127.0.0.1` |
| `ALLOW_EXTERNAL_BIND` | Autorise explicitement un bind non-loopback. Toute valeur autre que `true` (insensible à la casse) refuse le bind externe. | Optionnel | `false` |
| `SMTP_HOST` | Hôte du serveur SMTP relais. | **Requis** | — |
| `SMTP_PORT` | Port du serveur SMTP. | Optionnel | Défaut du transport : `587` (avec credentials, STARTTLS) ou `25` (sans credentials) |
| `SMTP_USER` | Identifiant SMTP. Actif uniquement si `SMTP_PASSWORD` est également renseigné. | Optionnel | — |
| `SMTP_PASSWORD` | Mot de passe SMTP. Actif uniquement si `SMTP_USER` est également renseigné. | Optionnel | — |
| `MAIL_FROM` | Expéditeur appliqué à tous les envois. Format Mailbox : `Nom <adresse@domaine>` ou `adresse@domaine`. | **Requis** | — |
| `INTERNAL_API_SECRET` | Secret d'authentification inter-services (header `x-internal-secret`). Minimum 32 octets UTF-8. Refus de démarrer si absent ou trop court. Sensible : ne jamais committer. | **Requis** | — |
| `RATE_LIMIT_PER_MINUTE` | Nombre maximal d'envois acceptés par minute (fenêtre fixe, global au service). Doit être un entier > 0 ; valeur invalide ou `0` = refus de démarrer. | Optionnel | `60` |

Mode SMTP selon les credentials :

- `SMTP_USER` **et** `SMTP_PASSWORD` renseignés : authentification SMTP avec chiffrement STARTTLS.
- L'un des deux manquant (ou les deux) : connexion en clair, sans TLS ni authentification. Ce mode convient à un serveur SMTP local de développement (type Mailpit) et ne doit pas être utilisé vers un relais distant.

Verbosité des logs : contrôlée par `RUST_LOG` (format `EnvFilter`, ex. `RUST_LOG=info`). Défaut : `info`. Les logs sont émis au format JSON.

## Sécurité

Bind loopback par défaut. `BIND_ADDR` vaut `127.0.0.1` : le service n'est joignable que localement. Pour l'exposer sur une interface externe (ex. `0.0.0.0`), il faut à la fois définir `BIND_ADDR` et positionner `ALLOW_EXTERNAL_BIND=true`. Sans cet opt-in explicite, un bind non-loopback fait échouer le démarrage : il n'y a pas de repli silencieux. En exposition externe, placer Missive derrière un reverse-proxy terminant TLS ; Missive ne sert pas de TLS lui-même.

Authentification. Chaque `POST /v1/send` doit porter le header `x-internal-secret` égal à `INTERNAL_API_SECRET`. La comparaison est faite en temps constant. Un secret absent, vide ou incorrect renvoie `401`.

Expéditeur. L'adresse d'expédition est toujours `MAIL_FROM`, définie côté serveur. Elle n'est jamais dérivée du payload de la requête.

Confidentialité des logs. Missive ne journalise ni le corps, ni le sujet, ni l'adresse complète du destinataire, ni le secret, ni les credentials SMTP. Seuls le domaine du destinataire, le code de réponse SMTP et la latence sont tracés.

### Rotation du secret

1. Générer une nouvelle valeur d'au moins 32 octets.

   Linux / macOS :

   ```
   openssl rand -base64 48
   ```

   Windows (PowerShell) :

   ```
   $b = New-Object byte[] 48; [System.Security.Cryptography.RNGCryptoServiceProvider]::Create().GetBytes($b); [Convert]::ToBase64String($b)
   ```

2. Mettre à jour `INTERNAL_API_SECRET` dans le `.env` de Missive.
3. Redémarrer Missive pour charger le nouveau secret.
4. Mettre à jour la valeur `x-internal-secret` chez tous les clients appelants.

## API

### `GET /health`

Sonde de disponibilité. Aucune authentification. Répond toujours `200` avec `{"status":"ok"}`.

### `POST /v1/send`

Header requis : `x-internal-secret`. Corps JSON :

```json
{
  "channel": "email",
  "to": "destinataire@example.com",
  "subject": "Sujet du message",
  "html": "<p>Corps HTML</p>",
  "text": "Corps texte"
}
```

Contraintes du payload :

- `channel` : `email` (opérationnel) ou `push` (réservé, renvoie `501`).
- `to` : un seul destinataire, adresse valide (une virgule est refusée).
- `subject` : requis, sans retour à la ligne (`\r` ou `\n` refusés).
- `html` et `text` : optionnels, mais au moins un des deux doit être non vide. Fournir les deux produit un message multipart (alternative texte/HTML).
- Taille du corps de requête : ≤ 512 Ko.

En cas de succès : `200` avec `{"status":"sent"}` (le message a été accepté par le serveur SMTP). Les erreurs applicatives (`400` de validation, `401`, `429`, `501`, `502`, `500`) ont un corps vide. Les rejets faits par le framework avant d'atteindre la logique applicative (`400` JSON malformé, `413`, `415`, `422`) renvoient un corps `text/plain` décrivant le rejet.

| Code | Cause |
|---|---|
| `200` | Message accepté et transmis au serveur SMTP. |
| `400` | Payload invalide, deux origines : validation applicative (canal inconnu, destinataire absent/invalide, plusieurs destinataires, sujet contenant un retour à la ligne, ou ni `html` ni `text` non vide) ou JSON syntaxiquement invalide. |
| `401` | Header `x-internal-secret` absent, vide ou incorrect. |
| `413` | Corps de requête au-delà de 512 Ko. |
| `415` | En-tête `content-type` absent ou différent de `application/json`. |
| `422` | JSON syntaxiquement valide mais champ requis manquant ou de type incorrect (ex. `subject` absent). |
| `429` | Limite d'envois par minute atteinte. |
| `501` | Canal `push` : réservé, non implémenté. |
| `502` | Échec de transmission au serveur SMTP (aucun détail exposé). |
| `500` | Erreur interne (message non constructible). |

## Exemples

Vérifier la disponibilité :

```
curl http://127.0.0.1:8184/health
```

Envoyer un email (corps HTML + texte) :

```
curl -X POST http://127.0.0.1:8184/v1/send -H "content-type: application/json" -H "x-internal-secret: REMPLACER_PAR_VOTRE_SECRET" -d '{"channel":"email","to":"destinataire@example.com","subject":"Bonjour","html":"<p>Ceci est un test</p>","text":"Ceci est un test"}'
```

Sous PowerShell, utiliser `curl.exe` (le `curl` de PowerShell est un alias d'`Invoke-WebRequest` à la syntaxe différente).

## Tester en local avec Mailpit

Mailpit fournit un serveur SMTP jetable et une interface web pour inspecter les mails capturés (rien n'est réellement envoyé).

Avec Docker :

```
docker run -p 1025:1025 -p 8025:8025 axllent/mailpit
```

Sans Docker : télécharger le binaire standalone depuis [github.com/axllent/mailpit/releases](https://github.com/axllent/mailpit/releases) et le lancer (il écoute par défaut sur `1025` pour le SMTP et `8025` pour l'interface web).

Configurer `.env` pour pointer sur Mailpit (SMTP en clair, sans credentials) :

```
SMTP_HOST=127.0.0.1
SMTP_PORT=1025
SMTP_USER=
SMTP_PASSWORD=
MAIL_FROM=missive@example.test
INTERNAL_API_SECRET=changez-ce-secret-de-32-octets-minimum
```

Lancer Missive, envoyer un message via l'exemple `POST /v1/send` ci-dessus, puis ouvrir l'interface Mailpit sur [http://127.0.0.1:8025](http://127.0.0.1:8025) pour voir le mail reçu.

## Décisions

- 2026-07-10 (SCRUM-323) : contrat d'API versionné sous `/v1/send`.
- 2026-07-10 (SCRUM-324) : mailer « bête » sans templating (sujet et corps fournis par l'appelant) ; canal `push` réservé et renvoyant `501`.
