# Missive

Librairie Rust d'envoi de mails transactionnels par SMTP. L'appelant fournit un sujet et un corps déjà rendus ; Missive ne fait ni templating ni personnalisation. Elle impose un mode TLS explicite, plafonne le nombre d'envois par minute et ne journalise aucun contenu.

Stack : Rust (stable) + lettre (rustls), runtime Tokio fourni par l'appelant.

Miroir public à chaque release : [GitHub `QVL-ToolBox/Missive`](https://github.com/QVL-ToolBox/Missive) (un commit par version, tag `vX.Y.Z`).

Depuis la v2.0.0, Missive n'expose plus de serveur HTTP : elle s'embarque directement dans le service qui envoie des mails.

## Installation

```toml
[dependencies]
missive = { git = "https://github.com/QVL-ToolBox/Missive", tag = "v2.0.2" }
```

## API

```rust
#[non_exhaustive]
pub struct MailConfig {
    pub host: String,
    pub port: Option<NonZeroU16>,
    pub tls: TlsMode,
    pub user: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub max_per_minute: u32,
    pub timeout_secs: Option<NonZeroU64>,
}

impl MailConfig {
    pub fn new(host: impl Into<String>, tls: TlsMode, from: impl Into<String>) -> Self;
    pub fn with_port(self, port: NonZeroU16) -> Self;
    pub fn with_credentials(self, user: impl Into<String>, password: impl Into<String>) -> Self;
    pub fn with_max_per_minute(self, max_per_minute: u32) -> Self;
    pub fn with_timeout(self, timeout_secs: NonZeroU64) -> Self;
}

#[non_exhaustive] pub enum TlsMode { Tls, StartTls, Plaintext }
#[non_exhaustive] pub enum Environment { Dev, Prod }

pub enum Body<'a> { Text(&'a str), Html(&'a str), Both { text: &'a str, html: &'a str } }

impl Mailer {
    pub fn new(config: &MailConfig, environment: Environment) -> Result<Mailer, MailError>;
    pub async fn send(&self, to: &str, subject: &str, body: Body<'_>) -> Result<(), MailError>;
}
```

`MailConfig` est `#[non_exhaustive]` : hors de la crate, on la construit par désérialisation ou par `MailConfig::new` suivi des `with_*` ; ses champs restent lisibles et modifiables.

`Mailer::new` valide toute la configuration et construit le transport SMTP sans ouvrir de connexion : une configuration invalide échoue au démarrage du service appelant, jamais au premier envoi. `new` ne requiert pas de runtime ; `send` s'exécute sous Tokio, avec le driver de temps activé (`enable_all()` ou `#[tokio::main]`), car l'envoi entier est borné par `timeout_secs` ; une connexion SMTP par envoi. Un `Mailer` est `Send + Sync` et se partage derrière un `Arc` : le plafond d'envois est global à l'instance.

`send` vérifie, dans cet ordre, le destinataire, le sujet et le corps, puis le plafond, et seulement ensuite contacte le serveur SMTP. Un envoi refusé ne consomme pas de quota, sauf s'il est refusé par le serveur SMTP.

| Contrainte d'envoi | Erreur |
|---|---|
| `to` : une seule adresse valide (une virgule est refusée) | `InvalidRecipient` |
| `subject` : sans `\r` ni `\n` | `InvalidSubject` |
| `body` : aucune partie vide ; `Body::Both` produit un multipart texte/HTML | `EmptyBody` |
| Plafond `max_per_minute` atteint (fenêtre fixe d'une minute) | `RateLimited` |
| Échec de transmission SMTP | `Send { permanent, code }` (code SMTP s'il y en a un, jamais le texte du serveur) |

L'expéditeur est toujours `from`, jamais un paramètre d'envoi.

## Configuration

`MailConfig` se désérialise (serde) depuis la configuration de l'appelant ; Missive ne lit aucune variable d'environnement.

| Champ | Rôle | Requis | Défaut |
|---|---|---|---|
| `host` | Hôte du serveur SMTP. | Oui | — |
| `port` | Port SMTP, non nul (`0` est refusé à la désérialisation). | Non | Celui du mode TLS : `465` (`tls`), `587` (`starttls`), `25` (`none`) |
| `tls` | `tls`, `starttls` ou `none`. | Oui | — |
| `user` / `password` | Authentification SMTP. Les deux ou aucun. | Non | — |
| `from` | Expéditeur : `Nom <adresse@domaine>` ou `adresse@domaine`. | Oui | — |
| `max_per_minute` | Plafond global d'envois par minute, > 0. | Non | `60` |
| `timeout_secs` | Délai maximal, en secondes, de toute l'opération d'envoi (connexion, bannière, TLS, commandes SMTP), quel que soit le mode TLS ; au-delà, `send` renvoie `Send { permanent: false, code: None }`. L'expiration ne garantit pas que le mail n'a pas été transmis : survenue après `DATA`, une nouvelle tentative peut créer un doublon. Non nul (`0` est refusé à la désérialisation). | Non | `15` |

`Debug` de `MailConfig` masque `user` et `password`.

Erreurs de validation renvoyées par `Mailer::new` :

| Cas | Erreur |
|---|---|
| `host` vide | `MissingHost` |
| `tls = "none"` avec `Environment::Prod` | `PlaintextInProduction` |
| `tls = "none"` vers un hôte autre que `localhost`, `127.0.0.0/8` ou `::1` | `PlaintextToRemoteHost` |
| `user` sans `password` ou l'inverse (une valeur blanche compte comme absente) | `PartialCredentials` |
| `from` illisible | `InvalidSender` |
| `max_per_minute = 0` | `ZeroRateLimit` |
| Paramètres TLS inconstructibles pour l'hôte | `Transport` |

## Modes TLS

- `tls` : TLS implicite dès la connexion (SMTPS). Mode recommandé vers un relais distant.
- `starttls` : connexion en clair puis passage obligatoire en TLS via `STARTTLS` ; l'envoi échoue si le serveur ne le propose pas.
- `none` : aucun chiffrement. Réservé au développement contre un SMTP local jetable (Mailpit) : accepté uniquement en `Environment::Dev` et vers un hôte loopback.

Aucun mode ne retombe silencieusement sur une connexion en clair.

## Confidentialité des logs

Missive émet un log `info` par envoi réussi et `warn` par échec SMTP, avec uniquement le domaine du destinataire (`recipient_domain`), le code SMTP (`smtp_code`), le caractère permanent de l'échec et son expiration (`permanent`, `timed_out`, en `warn`) et la latence (`latency_ms`). Ni le sujet, ni le corps, ni l'adresse complète, ni les identifiants, ni le texte de réponse du serveur SMTP n'apparaissent dans les logs. L'appelant installe son propre subscriber `tracing`.

Côté appelant, seul le `Display` de `MailError` est à journaliser : c'est la forme garantie sans contenu. `MailError` n'embarque aucune erreur source.

## Exemple d'intégration

Configuration TOML de l'appelant :

```toml
[mail]
host = "smtp.example.com"
port = 587
tls = "starttls"
user = "no-reply@example.com"
password = "secret-hors-depot"
from = "Acme <no-reply@example.com>"
max_per_minute = 60
```

Démarrage et envoi :

```rust
use missive::{Body, Environment, MailConfig, MailError, Mailer};
use std::sync::Arc;

fn build_mailer(config: &MailConfig, environment: Environment) -> Result<Arc<Mailer>, MailError> {
    Mailer::new(config, environment).map(Arc::new)
}

async fn send_reset_link(mailer: &Mailer, to: &str, link: &str) -> Result<(), MailError> {
    let text = format!("Réinitialisez votre mot de passe : {link}");
    mailer.send(to, "Réinitialisation du mot de passe", Body::Text(&text)).await
}
```

Sans fichier de configuration :

```rust
let config = MailConfig::new("smtp.example.com", TlsMode::Tls, "Acme <no-reply@example.com>")
    .with_credentials(user, password)
    .with_max_per_minute(30);
```

En développement, contre Mailpit (`docker run -p 1025:1025 -p 8025:8025 axllent/mailpit`) :

```toml
[mail]
host = "127.0.0.1"
port = 1025
tls = "none"
from = "missive@example.test"
```

Les mails capturés sont visibles sur http://127.0.0.1:8025.

## CI

`azure-pipelines.yml` étend `service.yml` de [QVL-ToolBox/Pipelines](https://dev.azure.com/qvl-project/QVL-ToolBox/_git/Pipelines) en `deployMode: none` : Qualite (`cargo fmt --check`, `clippy -D warnings`, `cargo test`) sur toutes les branches, puis tag `vX.Y.Z` sur `main` d'après la version de `Cargo.toml`. Rien n'est déployé. `Cargo.lock` reste committé (scanné par osv-scanner).

## Décisions

- 2026-07-10 (SCRUM-324) : mailer « bête » sans templating, sujet et corps fournis par l'appelant.
- 2026-10-06 : CI/CD par le template unique `service.yml` de QVL-ToolBox/Pipelines.
- 2026-10-13 (#103) : Missive v2.0.0 devient une crate lib (plus de serveur HTTP, plus de `/v1/send`, `/health`, `INTERNAL_API_SECRET` ni canal `push`), pipeline en `deployMode: none`.
- 2026-10-16 (#122) : le service HTTP v1 (port 8184) est retiré de CustHome ; CH-Api-Authenticator envoie ses mails par la crate, via la lib Auth v0.7.0 (#119). MISS-01 (loopback-only) et l'appel HTTP de SCRUM-325/326 sont sans objet.
