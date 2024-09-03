use elliptic_curve::pkcs8::DecodePrivateKey;
use futures::{AsyncRead, AsyncWrite};
use http_body_util::{BodyExt, Empty};
use hyper::{body::Bytes, Request, StatusCode};
use hyper_util::rt::TokioIo;
use notary_client::{Accepted, NotarizationRequest, NotaryClient};
use serde::{Deserialize, Serialize};
use tlsn_core::{commitment::CommitmentKind, proof::TlsProof};
use tlsn_prover::tls::{Prover, ProverConfig};
use tlsn_verifier::tls::{Verifier, VerifierConfig};
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{debug, info};

#[derive(Serialize)]
pub struct PayoutRequest {
    payoutId: String,
    amount: String,
    currency: String,
    country: String,
    correspondent: String,
    recipient: Recipient,
    customerTimestamp: String,
    statementDescription: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Recipient {
    #[serde(rename = "type")]
    recipient_type: String,
    address: Address,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Address {
    value: String,
}

#[derive(Deserialize, Debug)]
pub struct PayoutCallback {
    amount: String,
    correspondent: String,
    country: String,
    created: String,
    currency: String,
    customerTimestamp: String,
    failureReason: Option<FailureReason>,
    payoutId: String,
    recipient: Recipient,
    statementDescription: String,
    status: String,
}

#[derive(Deserialize, Debug)]
pub struct FailureReason {
    failureCode: String,
    failureMessage: String,
}

#[derive(Deserialize, Debug)]
pub struct PayoutResponse {
    pub payoutId: String,
    pub status: String,
    pub amount: String,
    pub currency: String,
    pub country: String,
    pub correspondent: String,
    pub recipient: Option<Recipient>,
    pub customerTimestamp: Option<String>,
    pub statementDescription: Option<String>,
    pub created: Option<String>,
    pub receivedByRecipient: Option<String>,
    pub correspondentIds: Option<CorrespondentIds>,
    pub metadata: Option<Metadata>,
}

#[derive(Deserialize, Debug)]
pub struct CorrespondentIds {
    pub SOME_CORRESPONDENT_ID: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Metadata {
    pub orderId: Option<String>,
    pub customerId: Option<String>,
}

/// Runs a simple Notary with the provided connection to the Prover.
pub async fn run_notary<T: AsyncWrite + AsyncRead + Send + Unpin + 'static>(conn: T) {
    // Load the notary signing key
    let signing_key_str = std::str::from_utf8(include_bytes!(
        "../../../notary/server/fixture/notary/notary.key"
    ))
    .unwrap();
    let signing_key = p256::ecdsa::SigningKey::from_pkcs8_pem(signing_key_str).unwrap();

    // Setup default config. Normally a different ID would be generated
    // for each notarization.
    let config = VerifierConfig::builder().id("example").build().unwrap();

    Verifier::new(config)
        .notarize::<_, p256::ecdsa::Signature>(conn, &signing_key)
        .await
        .unwrap();
}

pub async fn run_pawa(payout_id: &str, jwt: &str) -> std::io::Result<(bool)> {
    println!("Payout ID: {}", payout_id);
    let server_domain = "api.sandbox.pawapay.cloud";

    // tracing_subscriber::fmt()
    //     .with_env_filter("debug,yamux=info")
    //     .init();

    const NOTARY_HOST: &str = "notary.pse.dev";
    const NOTARY_PORT: u16 = 443;

    let notary_client = NotaryClient::builder()
        .host(NOTARY_HOST)
        .port(NOTARY_PORT)
        .enable_tls(true)
        .build()
        .unwrap();
    info!("Created Notary Client");

    let notarization_request = NotarizationRequest::builder().build().unwrap();

    let Accepted {
        io: notary_connection,
        id: session_id,
        ..
    } = notary_client
        .request_notarization(notarization_request)
        .await
        .unwrap();

    let config = ProverConfig::builder()
        .id(session_id)
        .server_dns(server_domain)
        .build()
        .unwrap();

    let prover = Prover::new(config)
        .setup(notary_connection.compat())
        .await
        .unwrap();

    let client_socket = tokio::net::TcpStream::connect((server_domain, 443))
        .await
        .unwrap();

    let (tls_connection, prover_fut) = prover.connect(client_socket.compat()).await.unwrap();

    let tls_connection = TokioIo::new(tls_connection.compat());

    let prover_ctrl = prover_fut.control();

    let prover_task = tokio::spawn(prover_fut);

    let (mut request_sender, connection) = hyper::client::conn::http1::handshake(tls_connection)
        .await
        .unwrap();

    tokio::spawn(connection);

    let url = format!("https://{}/payouts/{}", server_domain, payout_id);

    let request = Request::get(url.clone())
        .header("Authorization", format!("Bearer {}", jwt))
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::HOST, "api.sandbox.pawapay.cloud")
        .header("Connection", "close")
        .header(
            "Cache-Control",
            "no-cache, no-store, max-age=0, must-revalidate",
        )
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .header("Accept", "*/*")
        .body(Empty::<Bytes>::new())
        .unwrap();

    debug!("Sending request: {:?}", request);

    prover_ctrl.defer_decryption().await.unwrap();

    let response = request_sender.send_request(request).await.unwrap();

    debug!("Sent request");

    assert!(response.status() == StatusCode::OK, "{}", response.status());

    debug!("Request OK");

    let payload = response.into_body().collect().await.unwrap().to_bytes();
    debug!("Payload: {:?}", payload);
    let response_body = String::from_utf8_lossy(&payload);
    println!("Response: {}", response_body);

    let payout_responses: Vec<PayoutResponse> = serde_json::from_str(&response_body).unwrap();

    for payout_response in payout_responses {
        assert_eq!(
            payout_response.status, "COMPLETED",
            "Payout status is not COMPLETED"
        );
    }

    let prover = prover_task.await.unwrap().unwrap();

    let mut prover = prover.to_http().unwrap().start_notarize();

    prover.commit().unwrap();

    let notarized_session = prover.finalize().await.unwrap();

    debug!("Notarization complete!");

    let session_proof = notarized_session.session_proof();

    let mut proof_builder = notarized_session.session().data().build_substrings_proof();

    let request = &notarized_session.transcript().requests[0];

    proof_builder
        .reveal_sent(&request.without_data(), CommitmentKind::Blake3)
        .unwrap();

    proof_builder
        .reveal_sent(&request.request.target, CommitmentKind::Blake3)
        .unwrap();

    for header in &request.headers {
        if header.name.as_str().eq_ignore_ascii_case("Host") {
            proof_builder
                .reveal_sent(header, CommitmentKind::Blake3)
                .unwrap();
        } else {
            proof_builder
                .reveal_sent(&header.without_value(), CommitmentKind::Blake3)
                .unwrap();
        }
    }

    let response = &notarized_session.transcript().responses[0];

    proof_builder
        .reveal_recv(response, CommitmentKind::Blake3)
        .unwrap();

    let substrings_proof = proof_builder.build().unwrap();

    let proof = TlsProof {
        session: session_proof,
        substrings: substrings_proof,
    };

    //todo: extract the signature and send it on chain
    // for now: just verify the signature here and sign a message with a wallet in this .env

    // let resolution: bool = if let Some(session) = proof.session.into() {
    //     session.verify(notary_public_key, cert_verifier).is_ok()
    // } else {
    //     false
    // };

    Ok(true) // todo: fix resolution logic
}
