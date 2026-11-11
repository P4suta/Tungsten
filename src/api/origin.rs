use crate::Error;
use axum::http::{HeaderMap, Uri, header};

#[derive(Clone)]
pub struct ServerOrigin {
    origin: String,
    authority: String,
}

impl ServerOrigin {
    fn parse(value: &str, secure: bool) -> Result<Self, Error> {
        let invalid = || Error::InvalidData("Configure a valid HTTPS origin.");
        let uri: Uri = value.parse().map_err(|_| invalid())?;
        let authority = uri.authority().ok_or_else(invalid)?;
        let host = authority.host().trim_matches(['[', ']']);
        let address = host.parse::<std::net::IpAddr>();
        let local = host == "localhost" || address.as_ref().is_ok_and(|ip| ip.is_loopback());
        if uri.scheme_str() != Some(if secure { "https" } else { "http" })
            || authority.as_str() != authority.as_str().to_ascii_lowercase()
            || authority.port_u16() == Some(if secure { 443 } else { 80 })
            || authority.as_str().contains('@')
            || !matches!(
                uri.path_and_query().map(|path| path.as_str()),
                None | Some("/")
            )
            || if secure {
                local || address.is_ok()
            } else {
                !local
            }
        {
            return Err(invalid());
        }
        Ok(Self {
            origin: format!("{}://{}", if secure { "https" } else { "http" }, authority),
            authority: authority.to_string(),
        })
    }

    pub fn https(value: &str) -> Result<Self, Error> {
        Self::parse(value, true)
    }

    pub fn development(value: &str) -> Result<Self, Error> {
        Self::parse(value, false)
    }

    pub(super) fn permits(&self, headers: &HeaderMap) -> bool {
        headers.get_all(header::HOST).iter().count() == 1
            && headers
                .get(header::HOST)
                .is_some_and(|host| host.as_bytes() == self.authority.as_bytes())
            && headers.get_all(header::ORIGIN).iter().count() <= 1
            && headers
                .get(header::ORIGIN)
                .is_none_or(|origin| origin.as_bytes() == self.origin.as_bytes())
    }
}
