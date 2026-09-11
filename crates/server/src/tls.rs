use std::io;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::extract::Extension;
use axum::middleware::AddExtension;
use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use futures_util::future::BoxFuture;
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;
use tower_layer::Layer;

use crate::ca;

#[derive(Clone, Debug)]
pub struct PeerCn(pub Option<String>);

#[derive(Clone)]
pub struct MtlsAcceptor {
    inner: RustlsAcceptor,
}

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn build_acceptor(
    ca_cert_pem: &str,
    server_cert_pem: &str,
    server_key_pem: &str,
) -> Result<MtlsAcceptor> {
    let ca_ders = pem_certs(ca_cert_pem)?;
    let mut roots = RootCertStore::empty();
    for der in &ca_ders {
        roots
            .add(der.clone())
            .context("failed to add ca to root store")?;
    }

    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .context("failed to build client verifier")?;

    let server_certs = pem_certs(server_cert_pem)?;
    let server_key = pem_key(server_key_pem)?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(server_certs, server_key)
        .context("failed to build server config")?;

    let rustls_config = RustlsConfig::from_config(Arc::new(config));
    Ok(MtlsAcceptor {
        inner: RustlsAcceptor::new(rustls_config),
    })
}

fn pem_certs(pem: &str) -> Result<Vec<CertificateDer<'static>>> {
    let mut reader = io::Cursor::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to parse certificates")?;
    if certs.is_empty() {
        return Err(anyhow!("no certificates found in pem"));
    }
    Ok(certs)
}

fn pem_key(pem: &str) -> Result<PrivateKeyDer<'static>> {
    let mut reader = io::Cursor::new(pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut reader)
        .context("failed to parse private key")?
        .ok_or_else(|| anyhow!("no private key found in pem"))?;
    Ok(key)
}

impl<I, S> Accept<I, S> for MtlsAcceptor
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = TlsStream<I>;
    type Service = AddExtension<S, PeerCn>;
    type Future = BoxFuture<'static, io::Result<(Self::Stream, Self::Service)>>;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            tracing::info!("mTLS handshake starting on incoming TLS socket");
            let (stream, service) = inner.accept(stream, service).await?;
            let cn = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certs| certs.first())
                .and_then(|cert| ca::common_name_from_der(cert.as_ref()).ok());
            if let Some(ref peer_cn) = cn {
                tracing::info!(peer_cn, "mTLS handshake completed successfully");
            } else {
                tracing::warn!("mTLS handshake completed without a peer certificate common name");
            }
            let service = Extension(PeerCn(cn)).layer(service);
            Ok((stream, service))
        })
    }
}
