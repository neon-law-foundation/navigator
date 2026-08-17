//! Helpers shared by the tree-walking workspace guards.

/// Whether a filename is a SOPS-encrypted document, which the substring
/// guards must skip.
///
/// Those guards search for short tokens, matched case-insensitively, and a
/// SOPS value is base64 ciphertext. A four-character token collides with
/// random base64 often enough to matter: the staging tree tripped the licence
/// guard on a fragment inside an encrypted private key. That is worse than a
/// one-off failure, because re-encrypting produces different bytes — the same
/// guard would then pass or fail at random on every rotation, on a file nobody
/// edited.
///
/// Skipping loses nothing. Ciphertext is not authored text and makes no
/// licence or brand claim, and the tree has its own guard:
/// `cli::devx::deployments::tests::no_plaintext_key_material_sits_in_the_tree`
/// fails the build if any value under `deployments/` is not encrypted, so
/// nothing readable can hide behind this exemption.
///
/// Deliberately no examples here. This file is itself walked by the guards, so
/// naming any token they search for would make the helper trip them.
pub fn is_sops_ciphertext(file_name: &str) -> bool {
    file_name.ends_with(".enc.yaml")
}
