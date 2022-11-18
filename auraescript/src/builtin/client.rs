/* -------------------------------------------------------------------------- *\
 *             Apache 2.0 License Copyright © 2022 The Aurae Authors          *
 *                                                                            *
 *                +--------------------------------------------+              *
 *                |   █████╗ ██╗   ██╗██████╗  █████╗ ███████╗ |              *
 *                |  ██╔══██╗██║   ██║██╔══██╗██╔══██╗██╔════╝ |              *
 *                |  ███████║██║   ██║██████╔╝███████║█████╗   |              *
 *                |  ██╔══██║██║   ██║██╔══██╗██╔══██║██╔══╝   |              *
 *                |  ██║  ██║╚██████╔╝██║  ██║██║  ██║███████╗ |              *
 *                |  ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝ |              *
 *                +--------------------------------------------+              *
 *                                                                            *
 *                         Distributed Systems Runtime                        *
 *                                                                            *
 * -------------------------------------------------------------------------- *
 *                                                                            *
 *   Licensed under the Apache License, Version 2.0 (the "License");          *
 *   you may not use this file except in compliance with the License.         *
 *   You may obtain a copy of the License at                                  *
 *                                                                            *
 *       http://www.apache.org/licenses/LICENSE-2.0                           *
 *                                                                            *
 *   Unless required by applicable law or agreed to in writing, software      *
 *   distributed under the License is distributed on an "AS IS" BASIS,        *
 *   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. *
 *   See the License for the specific language governing permissions and      *
 *   limitations under the License.                                           *
 *                                                                            *
\* -------------------------------------------------------------------------- */

//! An internally scoped rust client specific for AuraeScript.
//!
//! Manages authenticating with remove Aurae instances, as well as searching
//! the local filesystem for configuration and authentication material.

use crate::codes::*;
use crate::config::*;
use crate::runtime::*;

use anyhow::{Context, Result};
// use macros::Output;
use serde::{Deserialize, Serialize};
use std::process;
use tokio::net::UnixStream;
use tonic::transport::Uri;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tower::service_fn;
use x509_certificate::certificate::*;

const KNOWN_IGNORED_SOCKET_ADDR: &str = "hxxp://null";

// TODO @kris-nova Once we have built out more client logic and we are confident this module is "good enough" come remove unwrap() statements

/// Instance of a single client for an Aurae consumer.
#[derive(Debug, Clone)]
pub struct AuraeClient {
    /// The channel used for gRPC connections before encryption is handled.
    pub channel: Option<Channel>,
    x509: Option<X509Certificate>,
    x509_details: Option<X509Details>,
}

impl Default for AuraeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AuraeClient {
    /// Create a new AuraeClient.
    ///
    /// Note: A new client is required for every independent execution of this process.
    pub fn new() -> Self {
        Self { channel: None, x509: None, x509_details: None }
    }
    async fn client_connect(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let x = new_client().await?;
        let subject_common_name = x
            .x509
            .subject_common_name()
            .ok_or("missing subject_common_name")?;
        let issuer_common_name =
            x.x509.issuer_common_name().ok_or("missing issuer_common_name")?;
        let sha256_fingerprint = x.x509.sha256_fingerprint()?;
        let key_algorithm =
            x.x509.key_algorithm().ok_or("missing key_algorithm")?.to_string();
        self.x509_details = Some(X509Details {
            subject_common_name,
            issuer_common_name,
            sha256_fingerprint: format!("{:?}", sha256_fingerprint),
            key_algorithm,
        });
        self.x509 = Some(x.x509);
        Ok(())
    }

    /// Initialize a new instance of the runtime subsystem.
    pub fn runtime(&mut self) -> CellService {
        CellService::new()
    }

    /// Convenience method for identifying the current service or client.
    pub fn info(&mut self) -> X509Details {
        let x = self.x509_details.as_ref();
        match x {
            Some(r) => r.clone(),
            None => X509Details {
                subject_common_name: "-".to_string(),
                issuer_common_name: "-".to_string(),
                sha256_fingerprint: "-".to_string(),
                key_algorithm: "-".to_string(),
            },
        }
    }
}

/// An in-memory representation of an X509 identity, and its meta data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X509Details {
    /// From the SSL spec, the subject common name.
    pub subject_common_name: String,
    /// From the SSL spec, the issuer common name.
    pub issuer_common_name: String,
    /// From the SSL spec, the sha256 sum fingerprint of the material.
    pub sha256_fingerprint: String,
    /// From the SSL spec, the algorithm used for encryption.
    pub key_algorithm: String,
}

/// Higher order identity structure.
#[derive(Debug, Clone)]
pub struct ClientIdentity {
    /// Channel used for connecting to a remote Aurae instance.
    pub channel: Channel,
    /// Service identity.
    x509: X509Certificate,
}

/// Create a new instance of an Aurae client.
pub async fn new_client() -> Result<ClientIdentity, Box<dyn std::error::Error>>
{
    let res = default_config()?;

    let server_root_ca_cert = tokio::fs::read(res.auth.ca_crt)
        .await
        .with_context(|| "could not read ca crt")?;

    let server_root_ca_cert = Certificate::from_pem(server_root_ca_cert);

    let client_cert = tokio::fs::read(res.auth.client_crt.clone())
        .await
        .with_context(|| "could not read client crt")?;

    let client_key = tokio::fs::read(&res.auth.client_key)
        .await
        .with_context(|| "could not read client key")?;

    let client_identity =
        Identity::from_pem(client_cert.clone(), client_key.clone());

    let tls = ClientTlsConfig::new()
        .domain_name("server.unsafe.aurae.io")
        .ca_certificate(server_root_ca_cert)
        .identity(client_identity);

    let channel = Channel::from_static(KNOWN_IGNORED_SOCKET_ADDR)
        .tls_config(tls)?
        .connect_with_connector(service_fn(move |_: Uri| {
            UnixStream::connect(res.system.socket.clone())
        }))
        .await
        .with_context(|| "unable to connect auraed system socket")?;
    Ok(ClientIdentity {
        channel,
        x509: X509Certificate::from_pem(client_cert.clone())?,
    })
}

/// Connect to a remote Aurae instance.
pub fn connect() -> AuraeClient {
    let mut client =
        AuraeClient { channel: None, x509: None, x509_details: None };
    let rt = tokio::runtime::Runtime::new().expect("new tokio runtime");
    let result = rt.block_on(client.client_connect());
    if let Err(e) = result {
        eprintln!("Unable to connect to Auraed: {:?}", e);
        process::exit(EXIT_CONNECT_FAILURE);
    }
    client
}
