//! Shared verification for inbound provider webhooks.
//!
//! State-advancing webhooks (e-signature completion in
//! [`crate::esignature_webhook`]; the filing/mail callbacks that land
//! with Prompt 5) arrive from the public internet and must not be
//! trusted on URL alone. The provider signs each delivery with an
//! HMAC-SHA256 over the **raw request body** using a shared secret and
//! sends the digest in a header. We recompute it over the exact bytes
//! we received and compare in constant time.
//!
//! Verifying over the raw body — *before* any JSON parse — is the
//! load-bearing property: the digest must cover the exact bytes that
//! later name the envelope/notation, or a re-serialization gap reopens
//! the forgery vector the signature exists to close.
//!
//! Two signature shapes live here. HMAC-SHA256 (a *shared secret*) is
//! what DocuSign Connect uses. SendGrid's "Signed Event Webhook" instead
//! uses **ECDSA over the P-256 curve** (an *asymmetric* key): SendGrid
//! signs with a private key it never shares, and we verify with the
//! public key it issues — so a leaked verification key can't be used to
//! forge events, unlike a shared HMAC secret. The signed payload there is
//! `timestamp || body` (the `X-Twilio-Email-Event-Webhook-Timestamp`
//! header concatenated with the raw body), so a captured signature can't
//! be replayed against a different timestamp.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Verify a base64-encoded HMAC-SHA256 digest over `body` keyed by
/// `key`. Returns `true` only when `provided_b64` decodes and matches
/// the recomputed MAC. Both the base64 decode failure and the digest
/// mismatch return `false` — a malformed header is as untrusted as a
/// wrong one.
///
/// The comparison runs through `hmac`'s `verify_slice`, which is
/// constant-time, so a timing attack cannot probe the digest byte by
/// byte.
#[must_use]
pub fn verify_hmac_sha256_b64(key: &[u8], body: &[u8], provided_b64: &str) -> bool {
    let Ok(provided) = base64::engine::general_purpose::STANDARD.decode(provided_b64.trim()) else {
        return false;
    };
    // `new_from_slice` only errors on a zero-length key for some MAC
    // backends; HMAC accepts any key length, so this never fails in
    // practice — treat an error as "cannot verify" = untrusted.
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&provided).is_ok()
}

/// Compute the base64-encoded HMAC-SHA256 digest of `body` keyed by
/// `key`. The signing side of [`verify_hmac_sha256_b64`]; used by tests
/// (and any local tooling) to forge a *valid* provider callback so the
/// happy path can be exercised without a live provider.
#[must_use]
pub fn sign_hmac_sha256_b64(key: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(body);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Verify SendGrid's "Signed Event Webhook" ECDSA/P-256 signature.
///
/// `public_key_der_b64` is the verification key SendGrid issues — a
/// base64-encoded DER `SubjectPublicKeyInfo`. `signed_payload` is the
/// exact bytes SendGrid signed: the `X-Twilio-Email-Event-Webhook-Timestamp`
/// header value concatenated with the raw request body. `signature_b64`
/// is the base64-encoded DER ECDSA signature from
/// `X-Twilio-Email-Event-Webhook-Signature`.
///
/// Returns `true` only when the key and signature both decode and the
/// signature verifies. Every failure mode — a malformed key, a malformed
/// signature, or a genuine mismatch — returns `false`: an unparseable
/// input is as untrusted as a forged one.
#[must_use]
pub fn verify_ecdsa_p256_der_b64(
    public_key_der_b64: &str,
    signed_payload: &[u8],
    signature_b64: &str,
) -> bool {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, VerifyingKey};
    use p256::pkcs8::DecodePublicKey;

    let std = base64::engine::general_purpose::STANDARD;
    let Ok(key_der) = std.decode(public_key_der_b64.trim()) else {
        return false;
    };
    let Ok(sig_der) = std.decode(signature_b64.trim()) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_public_key_der(&key_der) else {
        return false;
    };
    let Ok(signature) = Signature::from_der(&sig_der) else {
        return false;
    };
    verifying_key.verify(signed_payload, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{sign_hmac_sha256_b64, verify_ecdsa_p256_der_b64, verify_hmac_sha256_b64};
    use base64::Engine;
    use p256::ecdsa::{signature::Signer, Signature, SigningKey};
    use p256::pkcs8::EncodePublicKey;

    const KEY: &[u8] = b"shared-webhook-secret";
    const BODY: &[u8] = br#"{"envelopeId":"abc-123","status":"completed"}"#;

    #[test]
    fn a_freshly_signed_body_verifies() {
        let sig = sign_hmac_sha256_b64(KEY, BODY);
        assert!(verify_hmac_sha256_b64(KEY, BODY, &sig));
    }

    #[test]
    fn a_tampered_body_fails_against_the_original_signature() {
        let sig = sign_hmac_sha256_b64(KEY, BODY);
        let tampered = br#"{"envelopeId":"evil-999","status":"completed"}"#;
        assert!(!verify_hmac_sha256_b64(KEY, tampered, &sig));
    }

    #[test]
    fn the_wrong_key_fails() {
        let sig = sign_hmac_sha256_b64(KEY, BODY);
        assert!(!verify_hmac_sha256_b64(b"not-the-key", BODY, &sig));
    }

    #[test]
    fn a_non_base64_header_is_rejected_not_panicked() {
        assert!(!verify_hmac_sha256_b64(KEY, BODY, "@@@not base64@@@"));
    }

    #[test]
    fn an_empty_signature_is_rejected() {
        assert!(!verify_hmac_sha256_b64(KEY, BODY, ""));
    }

    #[test]
    fn surrounding_whitespace_in_the_header_is_tolerated() {
        let sig = sign_hmac_sha256_b64(KEY, BODY);
        let padded = format!("  {sig}\n");
        assert!(verify_hmac_sha256_b64(KEY, BODY, &padded));
    }

    // --- ECDSA / P-256 (SendGrid Signed Event Webhook) ---

    const STD: base64::engine::general_purpose::GeneralPurpose =
        base64::engine::general_purpose::STANDARD;

    /// A fixed (deterministic) P-256 signer — `0x42` repeated is a valid
    /// scalar below the curve order, so the fixture needs no RNG and the
    /// test is reproducible. Returns `(public_key_der_b64, signature_b64)`
    /// the way SendGrid would present them.
    fn ecdsa_fixture(scalar: u8, payload: &[u8]) -> (String, String) {
        let sk = SigningKey::from_slice(&[scalar; 32]).expect("valid P-256 scalar");
        let pk_der = sk
            .verifying_key()
            .to_public_key_der()
            .expect("encode SubjectPublicKeyInfo");
        let sig: Signature = sk.sign(payload);
        (
            STD.encode(pk_der.as_bytes()),
            STD.encode(sig.to_der().as_bytes()),
        )
    }

    #[test]
    fn a_freshly_signed_payload_verifies_under_ecdsa() {
        let payload = b"1716940800{\"event\":\"delivered\"}";
        let (pk, sig) = ecdsa_fixture(0x42, payload);
        assert!(verify_ecdsa_p256_der_b64(&pk, payload, &sig));
    }

    #[test]
    fn a_tampered_payload_fails_ecdsa_verification() {
        let payload = b"1716940800{\"event\":\"delivered\"}";
        let (pk, sig) = ecdsa_fixture(0x42, payload);
        // Same signature, a different timestamp prefix — replay defense.
        assert!(!verify_ecdsa_p256_der_b64(
            &pk,
            b"9999999999{\"event\":\"delivered\"}",
            &sig
        ));
    }

    #[test]
    fn the_wrong_public_key_fails_ecdsa_verification() {
        let payload = b"1716940800{\"event\":\"delivered\"}";
        let (_pk, sig) = ecdsa_fixture(0x42, payload);
        let (other_pk, _) = ecdsa_fixture(0x07, payload);
        assert!(!verify_ecdsa_p256_der_b64(&other_pk, payload, &sig));
    }

    #[test]
    fn a_non_base64_ecdsa_input_is_rejected_not_panicked() {
        let payload = b"x";
        let (pk, sig) = ecdsa_fixture(0x42, payload);
        assert!(!verify_ecdsa_p256_der_b64("@@@", payload, &sig));
        assert!(!verify_ecdsa_p256_der_b64(&pk, payload, "@@@"));
    }
}
