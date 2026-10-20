#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MailError {
    #[error("hôte SMTP manquant")]
    MissingHost,
    #[error("tls = \"none\" refusé en production")]
    PlaintextInProduction,
    #[error("tls = \"none\" refusé vers un hôte SMTP non loopback")]
    PlaintextToRemoteHost,
    #[error("identifiants SMTP partiels : user et password vont ensemble")]
    PartialCredentials,
    #[error("expéditeur invalide")]
    InvalidSender,
    #[error("max_per_minute doit être supérieur à 0")]
    ZeroRateLimit,
    #[error("transport SMTP inconstructible")]
    Transport,
    #[error("destinataire invalide : une seule adresse attendue")]
    InvalidRecipient,
    #[error("sujet invalide : retour à la ligne interdit")]
    InvalidSubject,
    #[error("corps de message vide")]
    EmptyBody,
    #[error("message inconstructible")]
    Build,
    #[error("plafond d'envois par minute atteint")]
    RateLimited,
    #[error("envoi SMTP en échec (permanent : {permanent}, code : {code:?})")]
    Send { permanent: bool, code: Option<u16> },
}
