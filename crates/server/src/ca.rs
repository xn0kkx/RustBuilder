use anyhow::{anyhow, Context, Result};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use time::{Duration, OffsetDateTime};

pub struct Ca {
    pub cert: Certificate,
    pub key: KeyPair,
}

pub struct Issued {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn create_ca() -> Result<(String, String)> {
    let key = KeyPair::generate().context("failed to generate ca key")?;
    let mut params = CertificateParams::new(Vec::<String>::new()).context("ca params")?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "RustBuilder CA");
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    set_validity(&mut params, 3650);
    let cert = params.self_signed(&key).context("failed to self-sign ca")?;
    Ok((cert.pem(), key.serialize_pem()))
}

pub fn load_ca(cert_pem: &str, key_pem: &str) -> Result<Ca> {
    let key = KeyPair::from_pem(key_pem).context("failed to parse ca key")?;
    let params = CertificateParams::from_ca_cert_pem(cert_pem).context("failed to parse ca cert")?;
    let cert = params
        .self_signed(&key)
        .context("failed to rebuild ca certificate")?;
    Ok(Ca { cert, key })
}

pub fn issue_server_cert(ca: &Ca, sans: Vec<String>) -> Result<Issued> {
    let key = KeyPair::generate().context("failed to generate server key")?;
    let mut params = CertificateParams::new(sans).context("server params")?;
    params
        .distinguished_name
        .push(DnType::CommonName, "rustbuilder-server");
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    set_validity(&mut params, 3650);
    let cert = params
        .signed_by(&key, &ca.cert, &ca.key)
        .context("failed to sign server cert")?;
    Ok(Issued {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    })
}

pub fn issue_client_cert(ca: &Ca, build_id: &str) -> Result<Issued> {
    let key = KeyPair::generate().context("failed to generate client key")?;
    let mut params = CertificateParams::new(Vec::<String>::new()).context("client params")?;
    params.distinguished_name.push(DnType::CommonName, build_id);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    set_validity(&mut params, 3650);
    let cert = params
        .signed_by(&key, &ca.cert, &ca.key)
        .context("failed to sign client cert")?;
    Ok(Issued {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    })
}

pub fn common_name_from_der(der: &[u8]) -> Result<String> {
    use x509_parser::prelude::FromDer;
    let (_, cert) =
        x509_parser::certificate::X509Certificate::from_der(der).map_err(|e| anyhow!("{e}"))?;
    let cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|a| a.as_str().ok())
        .ok_or_else(|| anyhow!("client certificate has no common name"))?;
    Ok(cn.to_string())
}

fn set_validity(params: &mut CertificateParams, days: i64) {
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(days);
}
