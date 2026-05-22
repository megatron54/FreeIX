//! FreeIX Certificate Authority
//!
//! Generates and manages a local root CA for HTTPS interception.
//! The root CA cert is installed into the Windows certificate store
//! so browsers trust our MITM proxy.

use std::path::PathBuf;

use rcgen::{
    BasicConstraints, CertificateParams, Certificate, DistinguishedName, DnType, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tracing::info;

#[derive(Debug, Error)]
pub enum CaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("certificate generation error: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("CA not initialized")]
    NotInitialized,
}

/// The FreeIX root CA, used to sign per-domain certificates on the fly.
pub struct RootCa {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
    ca_cert: Certificate,
    ca_key: KeyPair,
}

impl RootCa {
    /// Load existing CA from disk, or generate a new one.
    pub fn load_or_create() -> Result<Self, CaError> {
        let ca_dir = Self::ca_dir();
        let cert_path = ca_dir.join("freeix-ca.crt");
        let key_path = ca_dir.join("freeix-ca.key");

        if cert_path.exists() && key_path.exists() {
            let cert_pem = std::fs::read_to_string(&cert_path)?;
            let key_pem = std::fs::read_to_string(&key_path)?;

            let key = KeyPair::from_pem(&key_pem)?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)?;
            let ca_cert = params.self_signed(&key)?;
            let cert_der = ca_cert.der().to_vec();

            info!("Loaded existing FreeIX root CA");
            return Ok(Self {
                cert_pem,
                key_pem,
                cert_der,
                ca_cert,
                ca_key: key,
            });
        }

        // Generate new CA
        std::fs::create_dir_all(&ca_dir)?;

        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "FreeIX Root CA");
        dn.push(DnType::OrganizationName, "FreeIX");
        params.distinguished_name = dn;

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(3650);

        let key = KeyPair::generate()?;
        let ca_cert = params.self_signed(&key)?;

        let cert_pem = ca_cert.pem();
        let key_pem = key.serialize_pem();
        let cert_der = ca_cert.der().to_vec();

        std::fs::write(&cert_path, &cert_pem)?;
        std::fs::write(&key_path, &key_pem)?;

        info!(?cert_path, "Generated new FreeIX root CA");

        Ok(Self {
            cert_pem,
            key_pem,
            cert_der,
            ca_cert,
            ca_key: key,
        })
    }

    /// Generate a leaf certificate for a specific domain, signed by this CA.
    pub fn issue_cert(&self, domain: &str) -> Result<(Vec<u8>, Vec<u8>), CaError> {
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, domain);
        params.distinguished_name = dn;
        params.subject_alt_names = vec![SanType::DnsName(domain.try_into().unwrap())];

        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(365);

        let leaf_key = KeyPair::generate()?;
        let cert = params.signed_by(&leaf_key, &self.ca_cert, &self.ca_key)?;

        Ok((cert.der().to_vec(), leaf_key.serialize_der()))
    }

    /// Install the root CA into the Windows certificate store.
    pub fn install_to_system(&self) -> Result<(), CaError> {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let cert_path = Self::ca_dir().join("freeix-ca.crt");
            let script = format!(
                "Import-Certificate -FilePath '{}' -CertStoreLocation Cert:\\LocalMachine\\Root",
                cert_path.display()
            );
            let _ = Command::new("powershell")
                .args(&[
                    "-NoProfile", "-WindowStyle", "Hidden", "-Command",
                    &format!(
                        "Start-Process powershell -ArgumentList '-NoProfile -WindowStyle Hidden -Command {}' -Verb RunAs -Wait -WindowStyle Hidden",
                        script.replace("'", "''")
                    ),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            info!("Installed FreeIX root CA to Windows certificate store");
        }
        Ok(())
    }

    /// Check if the CA is installed in the system store.
    pub fn is_installed(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let output = Command::new("powershell")
                .args(&[
                    "-NoProfile", "-WindowStyle", "Hidden", "-Command",
                    "Get-ChildItem Cert:\\LocalMachine\\Root | Where-Object { $_.Subject -like '*FreeIX*' } | Measure-Object | Select-Object -ExpandProperty Count",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            if let Ok(out) = output {
                let count: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0);
                return count > 0;
            }
        }
        false
    }

    fn ca_dir() -> PathBuf {
        directories::ProjectDirs::from("com", "freeix", "FreeIX")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("C:\\FreeIX"))
            .join("ca")
    }
}
