//! # PDF Digital Certification
//!
//! Certifies PDF files using X.509 certificates with PKCS#7 detached signatures.
//! Uses incremental update to append signature objects without modifying the
//! original PDF content.
//!
//! ## Two-Pass Approach
//!
//! 1. **Pass 1**: Append signature dictionary (with placeholder `/Contents`),
//!    signature field widget, and updated catalog via PDF incremental update.
//! 2. **Pass 2**: Compute SHA-256 hash over the byte ranges excluding the
//!    placeholder, sign with RSA, build PKCS#7 SignedData, and write the
//!    DER-encoded signature into the placeholder.

use crate::error::FormeError;
use crate::model::CertificationConfig;

use der::Encode;
use pkcs8::{DecodePrivateKey, EncodePublicKey};
use rsa::pkcs1v15::SigningKey;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};
use signature::{SignatureEncoding, SignerMut};
use x509_cert::Certificate;

/// Size of the hex-encoded signature placeholder (4096 bytes = 8192 hex chars).
/// This must be large enough to hold the DER-encoded PKCS#7 SignedData.
const SIG_PLACEHOLDER_HEX_LEN: usize = 8192;

/// Result of scanning an existing PDF for structural metadata.
struct PdfScanResult {
    /// Byte offset of the last `startxref` value.
    startxref_offset: usize,
    /// Value of `/Size` in the trailer (next available object ID).
    size: usize,
    /// Object number of `/Root` (the Catalog).
    root_obj: usize,
    /// Object number of the first `/Type /Page` object found.
    first_page_obj: usize,
}

/// Certify PDF bytes with an X.509 certificate, producing a valid digitally certified PDF.
///
/// Works on any valid PDF — either freshly rendered or loaded from disk.
/// Uses incremental update to preserve the original PDF bytes.
pub fn certify_pdf(pdf_bytes: &[u8], config: &CertificationConfig) -> Result<Vec<u8>, FormeError> {
    // Parse certificate and private key
    let cert = parse_pem_certificate(&config.certificate_pem)?;
    let private_key = parse_pem_private_key(&config.private_key_pem)?;

    // Verify key matches certificate by comparing public key DER
    let cert_pub_key_der = cert
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|e| FormeError::RenderError(format!("Failed to encode cert public key: {e}")))?;
    let key_pub_der = rsa::RsaPublicKey::from(&private_key)
        .to_public_key_der()
        .map_err(|e| FormeError::RenderError(format!("Failed to encode public key: {e}")))?;

    if cert_pub_key_der != key_pub_der.as_bytes() {
        return Err(FormeError::RenderError(
            "Certificate and private key do not match".to_string(),
        ));
    }

    // Scan existing PDF for metadata
    let scan = scan_pdf_metadata(pdf_bytes)?;

    // Encode certificate to DER for embedding in PKCS#7
    let cert_der = cert
        .to_der()
        .map_err(|e| FormeError::RenderError(format!("Failed to DER-encode certificate: {e}")))?;

    // Pass 1: Build incremental update with signature placeholder
    let (mut output, placeholder_offset) =
        build_incremental_update(pdf_bytes, &scan, config, &cert_der)?;

    // The placeholder starts at `placeholder_offset` and is SIG_PLACEHOLDER_HEX_LEN chars
    // (enclosed in angle brackets: `<hex...>`).
    // ByteRange: [0, before_sig_hex, after_sig_hex, total_len]
    let before_sig_hex = placeholder_offset; // offset of '<'
    let after_sig_hex = placeholder_offset + 1 + SIG_PLACEHOLDER_HEX_LEN + 1; // after '>'
    let total_len = output.len();

    // Update the ByteRange value in the output
    update_byte_range(&mut output, before_sig_hex, after_sig_hex, total_len)?;

    // Pass 2: Concatenate the signed byte ranges and sign
    let mut signed_data = Vec::with_capacity(before_sig_hex + (total_len - after_sig_hex));
    signed_data.extend_from_slice(&output[0..before_sig_hex]);
    signed_data.extend_from_slice(&output[after_sig_hex..total_len]);

    // Sign with RSA PKCS#1 v1.5 (SigningKey hashes internally with SHA-256)
    let mut signing_key = SigningKey::<Sha256>::new(private_key);
    let sig_result: rsa::pkcs1v15::Signature = signing_key.sign(&signed_data);
    let sig_bytes = sig_result.to_bytes();

    // Compute the hash for PKCS#7 SignedData construction
    let hash = Sha256::digest(&signed_data);

    // Build PKCS#7 SignedData
    let pkcs7_der = build_pkcs7_signed_data(&cert_der, &sig_bytes, &hash)?;

    // Hex-encode and write into placeholder
    let hex_sig = hex_encode(&pkcs7_der);
    if hex_sig.len() > SIG_PLACEHOLDER_HEX_LEN {
        return Err(FormeError::RenderError(format!(
            "PKCS#7 signature ({} hex chars) exceeds placeholder size ({})",
            hex_sig.len(),
            SIG_PLACEHOLDER_HEX_LEN
        )));
    }

    // Write hex signature into placeholder, pad with zeros
    let sig_start = placeholder_offset + 1; // skip '<'
    for (i, b) in hex_sig.bytes().enumerate() {
        output[sig_start + i] = b;
    }
    // Remaining bytes are already '0' from the placeholder

    Ok(output)
}

/// Parse a PEM-encoded X.509 certificate.
fn parse_pem_certificate(pem: &str) -> Result<Certificate, FormeError> {
    use der::DecodePem;
    Certificate::from_pem(pem)
        .map_err(|e| FormeError::RenderError(format!("Failed to parse PEM certificate: {e}")))
}

/// Parse a PEM-encoded RSA private key (PKCS#8 or PKCS#1 format).
///
/// Tries PKCS#8 first (`-----BEGIN PRIVATE KEY-----`), then falls back to
/// PKCS#1 (`-----BEGIN RSA PRIVATE KEY-----`). Only RSA keys are supported.
/// ECDSA, Ed25519, and other key types will produce a clear error message.
fn parse_pem_private_key(pem: &str) -> Result<RsaPrivateKey, FormeError> {
    use rsa::pkcs1::DecodeRsaPrivateKey;

    // Try PKCS#8 first (most common format)
    match RsaPrivateKey::from_pkcs8_pem(pem) {
        Ok(key) => Ok(key),
        Err(pkcs8_err) => {
            // If it looks like a PKCS#1 key, try that format
            if pem.contains("BEGIN RSA PRIVATE KEY") {
                return RsaPrivateKey::from_pkcs1_pem(pem).map_err(|e| {
                    FormeError::RenderError(format!(
                        "Failed to parse PKCS#1 (RSA) private key: {e}"
                    ))
                });
            }

            let msg = pkcs8_err.to_string();
            // Detect non-RSA key algorithms (ECDSA, Ed25519, etc.)
            if msg.contains("algorithm") || msg.contains("OID") {
                return Err(FormeError::RenderError(
                    "Only RSA private keys are supported for PDF signing. \
                     ECDSA, Ed25519, and other key types are not supported."
                        .to_string(),
                ));
            }

            Err(FormeError::RenderError(format!(
                "Failed to parse PEM private key: {pkcs8_err}"
            )))
        }
    }
}

/// Scan a PDF for structural metadata needed for incremental update.
///
/// Uses byte-level searching for `startxref` and trailer values to avoid
/// offset corruption from `String::from_utf8_lossy` (which replaces invalid
/// UTF-8 sequences with multi-byte U+FFFD, shifting string positions relative
/// to byte positions).
fn scan_pdf_metadata(pdf: &[u8]) -> Result<PdfScanResult, FormeError> {
    // Find startxref by scanning raw bytes (last occurrence)
    let startxref_pos = rfind_bytes(pdf, b"startxref")
        .ok_or_else(|| FormeError::RenderError("No startxref found in PDF".to_string()))?;
    let after_startxref = &pdf[startxref_pos + 9..];
    let startxref_offset: usize = parse_number_from_bytes(after_startxref)
        .ok_or_else(|| FormeError::RenderError("Cannot parse startxref value".to_string()))?;

    // Find trailer by scanning raw bytes (last occurrence)
    let trailer_pos = rfind_bytes(pdf, b"trailer")
        .ok_or_else(|| FormeError::RenderError("No trailer found in PDF".to_string()))?;
    let trailer_section = &pdf[trailer_pos..startxref_pos];

    // Find /Size in trailer
    let size = find_value_in_bytes(trailer_section, b"/Size")
        .ok_or_else(|| FormeError::RenderError("No /Size found in trailer".to_string()))?;

    // Find /Root reference
    let root_obj = find_ref_in_bytes(trailer_section, b"/Root")
        .ok_or_else(|| FormeError::RenderError("No /Root found in trailer".to_string()))?;

    // Find first /Type /Page object (not /Pages) — use lossy string since we
    // only need the object number, not byte offsets
    let text = String::from_utf8_lossy(pdf);
    let first_page_obj = find_first_page_obj(&text)
        .ok_or_else(|| FormeError::RenderError("No /Type /Page found in PDF".to_string()))?;

    Ok(PdfScanResult {
        startxref_offset,
        size,
        root_obj,
        first_page_obj,
    })
}

/// Reverse-find a byte needle in a haystack.
fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    for i in (0..=haystack.len() - needle.len()).rev() {
        if haystack[i..i + needle.len()] == *needle {
            return Some(i);
        }
    }
    None
}

/// Parse a decimal number from the start of a byte slice, skipping leading whitespace.
fn parse_number_from_bytes(bytes: &[u8]) -> Option<usize> {
    let start = bytes.iter().position(|&b| b.is_ascii_digit())?;
    let end = bytes[start..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map(|p| start + p)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()
}

/// Find a numeric value after a key in raw bytes (e.g., "/Size 42").
fn find_value_in_bytes(section: &[u8], key: &[u8]) -> Option<usize> {
    let pos = find_bytes(section, key)?;
    parse_number_from_bytes(&section[pos + key.len()..])
}

/// Find an object reference after a key in raw bytes (e.g., "/Root 1 0 R" → 1).
fn find_ref_in_bytes(section: &[u8], key: &[u8]) -> Option<usize> {
    let pos = find_bytes(section, key)?;
    parse_number_from_bytes(&section[pos + key.len()..])
}

/// Find the object number of the first `/Type /Page` (not `/Type /Pages`).
fn find_first_page_obj(text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = text[search_from..].find("/Type /Page") {
        let abs_pos = search_from + pos;
        let after = &text[abs_pos + 11..];
        if after.starts_with('s') || after.starts_with('S') {
            search_from = abs_pos + 11;
            continue;
        }

        let before = &text[..abs_pos];
        if let Some(obj_pos) = before.rfind(" 0 obj") {
            let line_start = before[..obj_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let obj_num_str = text[line_start..obj_pos].trim();
            if let Ok(obj_num) = obj_num_str.parse::<usize>() {
                return Some(obj_num);
            }
        }
        search_from = abs_pos + 11;
    }
    None
}

/// Build the incremental update appended to the original PDF.
/// Returns the complete PDF bytes and the byte offset of the signature placeholder.
fn build_incremental_update(
    original: &[u8],
    scan: &PdfScanResult,
    config: &CertificationConfig,
    cert_der: &[u8],
) -> Result<(Vec<u8>, usize), FormeError> {
    let mut buf = Vec::from(original);

    // Ensure original ends with newline
    if !buf.ends_with(b"\n") {
        buf.push(b'\n');
    }

    let next_id = scan.size;
    let sig_dict_id = next_id;
    let sig_field_id = next_id + 1;
    let new_catalog_id = next_id + 2;
    // If visible, allocate an extra object for the appearance stream XObject
    let ap_xobj_id = if config.visible {
        Some(next_id + 3)
    } else {
        None
    };
    let new_size = next_id + if config.visible { 4 } else { 3 };

    // Record xref entries: (obj_id, byte_offset)
    let mut xref_entries: Vec<(usize, usize)> = Vec::new();

    // --- Signature Dictionary Object ---
    xref_entries.push((sig_dict_id, buf.len()));
    let date_str = format_pdf_date();

    // Build the sig dict with a placeholder ByteRange and Contents
    // ByteRange placeholder must be large enough — we use fixed-width formatting
    let byte_range_placeholder = "/ByteRange [0 0000000000 0000000000 0000000000]";

    let mut sig_dict = format!(
        "{sig_dict_id} 0 obj\n<<\n/Type /Sig\n/Filter /Adobe.PPKLite\n/SubFilter /adbe.pkcs7.detached\n{byte_range_placeholder}\n/M ({date_str})\n"
    );

    if let Some(ref reason) = config.reason {
        sig_dict.push_str(&format!(
            "/Reason {}\n",
            super::PdfWriter::encode_text_string(reason)
        ));
    }
    if let Some(ref location) = config.location {
        sig_dict.push_str(&format!(
            "/Location {}\n",
            super::PdfWriter::encode_text_string(location)
        ));
    }
    if let Some(ref contact) = config.contact {
        sig_dict.push_str(&format!(
            "/ContactInfo {}\n",
            super::PdfWriter::encode_text_string(contact)
        ));
    }

    // Cert as hex string
    let cert_hex = hex_encode(cert_der);
    sig_dict.push_str(&format!("/Cert <{cert_hex}>\n"));

    sig_dict.push_str("/Contents <");
    buf.extend_from_slice(sig_dict.as_bytes());

    // Record position of the placeholder (including the '<')
    let placeholder_offset = buf.len() - 1; // the '<' char

    // Write placeholder zeros
    buf.extend(std::iter::repeat_n(b'0', SIG_PLACEHOLDER_HEX_LEN));
    buf.extend_from_slice(b">\n>>\nendobj\n");

    // --- Appearance Stream XObject (visible signatures only) ---
    if let Some(ap_id) = ap_xobj_id {
        xref_entries.push((ap_id, buf.len()));

        let w = config.width.unwrap_or(200.0);
        let h = config.height.unwrap_or(50.0);

        // Extract signer name from certificate CN
        let signer_name =
            extract_cn_from_cert_der(cert_der).unwrap_or_else(|| "Unknown".to_string());
        let date_display = format_display_date();

        // Build appearance stream content
        let mut content = String::new();
        let font_size = 9.0_f64;
        let line_height = font_size + 3.0;
        let margin = 4.0_f64;
        let mut y_pos = h - margin - font_size;

        // "Digitally signed by"
        content.push_str(&format!(
            "BT /Helv {font_size:.1} Tf {margin:.2} {y_pos:.2} Td (Digitally signed by) Tj ET\n"
        ));
        y_pos -= line_height;

        // Signer name
        content.push_str(&format!(
            "BT /Helv {font_size:.1} Tf {margin:.2} {y_pos:.2} Td ({}) Tj ET\n",
            escape_pdf_string(&signer_name)
        ));
        y_pos -= line_height;

        // Date
        content.push_str(&format!(
            "BT /Helv {font_size:.1} Tf {margin:.2} {y_pos:.2} Td (Date: {date_display}) Tj ET\n"
        ));
        y_pos -= line_height;

        // Reason (if present)
        if let Some(ref reason) = config.reason {
            content.push_str(&format!(
                "BT /Helv {font_size:.1} Tf {margin:.2} {y_pos:.2} Td (Reason: {}) Tj ET\n",
                escape_pdf_string(reason)
            ));
            let _ = y_pos; // suppress unused warning on last iteration
        }
        let _ = y_pos;

        let content_bytes = content.as_bytes();
        let ap_obj = format!(
            "{ap_id} 0 obj\n<<\n/Type /XObject\n/Subtype /Form\n/BBox [0 0 {w:.2} {h:.2}]\n/Resources << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>\n/Length {}\n>>\nstream\n",
            content_bytes.len()
        );
        buf.extend_from_slice(ap_obj.as_bytes());
        buf.extend_from_slice(content_bytes);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    // --- Signature Field Widget ---
    xref_entries.push((sig_field_id, buf.len()));

    let sig_name = next_signature_name(original);

    let rect = if config.visible {
        let x = config.x.unwrap_or(0.0);
        let y = config.y.unwrap_or(0.0);
        let w = config.width.unwrap_or(200.0);
        let h = config.height.unwrap_or(50.0);
        format!("[{x:.2} {y:.2} {:.2} {:.2}]", x + w, y + h)
    } else {
        "[0 0 0 0]".to_string()
    };

    let ap_entry = if let Some(ap_id) = ap_xobj_id {
        format!("/AP << /N {ap_id} 0 R >>\n")
    } else {
        String::new()
    };

    let sig_field = format!(
        "{sig_field_id} 0 obj\n<<\n/Type /Annot\n/Subtype /Widget\n/FT /Sig\n/T ({sig_name})\n/V {sig_dict_id} 0 R\n/Rect {rect}\n/P {page_ref} 0 R\n/F 132\n{ap_entry}>>\nendobj\n",
        page_ref = scan.first_page_obj
    );
    buf.extend_from_slice(sig_field.as_bytes());

    // --- Updated Catalog with AcroForm ---
    xref_entries.push((new_catalog_id, buf.len()));

    // Read existing catalog to preserve its entries (especially /Pages reference)
    // We need to find the /Pages ref from the original catalog
    let original_lossy = String::from_utf8_lossy(original);
    let original_text: &str = &original_lossy;
    let pages_ref = find_catalog_pages_ref(original_text, scan.root_obj).unwrap_or(2);

    // Merge existing AcroForm fields (from <TextField>, <Checkbox>, etc.) with the new signature field
    let existing_fields = find_existing_acroform_fields(original, scan.root_obj);
    let all_fields = if existing_fields.is_empty() {
        format!("{sig_field_id} 0 R")
    } else {
        let mut fields = existing_fields.join(" ");
        fields.push(' ');
        fields.push_str(&format!("{sig_field_id} 0 R"));
        fields
    };

    // Preserve existing AcroForm metadata (DA, NeedAppearances) from the original PDF
    let acroform_meta = find_existing_acroform_metadata(original, scan.root_obj);

    let mut acroform_entries = format!("/Fields [{all_fields}] /SigFlags 3");
    if acroform_meta.need_appearances {
        acroform_entries.push_str(" /NeedAppearances true");
    }
    if let Some(ref da) = acroform_meta.da {
        acroform_entries.push_str(&format!(" /DA ({})", escape_pdf_string(da)));
    }
    if config.visible {
        acroform_entries.push_str(
            " /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        );
    }

    let mut catalog = format!(
        "{new_catalog_id} 0 obj\n<<\n/Type /Catalog\n/Pages {pages_ref} 0 R\n/AcroForm << {acroform_entries} >>\n"
    );

    // Preserve /Lang if present
    if let Some(lang) = find_catalog_string(original_text, scan.root_obj, "/Lang") {
        catalog.push_str(&format!("/Lang ({lang})\n"));
    }

    // Preserve /MarkInfo if present
    if catalog_has_key(original_text, scan.root_obj, "/MarkInfo") {
        catalog.push_str("/MarkInfo << /Marked true >>\n");
    }

    // Preserve /StructTreeRoot if present
    if let Some(struct_ref) = find_catalog_ref(original_text, scan.root_obj, "/StructTreeRoot") {
        catalog.push_str(&format!("/StructTreeRoot {struct_ref} 0 R\n"));
    }

    // Preserve /Metadata if present
    if let Some(meta_ref) = find_catalog_ref(original_text, scan.root_obj, "/Metadata") {
        catalog.push_str(&format!("/Metadata {meta_ref} 0 R\n"));
    }

    // Preserve /Names if present
    if let Some(names_ref) = find_catalog_ref(original_text, scan.root_obj, "/Names") {
        catalog.push_str(&format!("/Names {names_ref} 0 R\n"));
    }

    // Preserve /ViewerPreferences if present
    if let Some(vp_ref) = find_catalog_ref(original_text, scan.root_obj, "/ViewerPreferences") {
        catalog.push_str(&format!("/ViewerPreferences {vp_ref} 0 R\n"));
    }

    // Preserve /OutputIntents if present (for PDF/A)
    if let Some(oi_content) =
        find_catalog_array_content(original_text, scan.root_obj, "/OutputIntents")
    {
        catalog.push_str(&format!("/OutputIntents {oi_content}\n"));
    }

    catalog.push_str(">>\nendobj\n");
    buf.extend_from_slice(catalog.as_bytes());

    // --- Cross-Reference Table ---
    let xref_offset = buf.len();
    buf.extend_from_slice(b"xref\n");

    // Write each entry as a separate subsection
    // Sort entries by object ID for proper xref table
    let mut sorted_entries = xref_entries.clone();
    sorted_entries.sort_by_key(|(id, _)| *id);

    // Group consecutive IDs into subsections
    let mut i = 0;
    while i < sorted_entries.len() {
        let start_id = sorted_entries[i].0;
        let mut count = 1;
        while i + count < sorted_entries.len() && sorted_entries[i + count].0 == start_id + count {
            count += 1;
        }
        buf.extend_from_slice(format!("{start_id} {count}\n").as_bytes());
        for j in 0..count {
            let offset = sorted_entries[i + j].1;
            buf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        i += count;
    }

    // --- Trailer ---
    buf.extend_from_slice(
        format!(
            "trailer\n<<\n/Size {new_size}\n/Root {new_catalog_id} 0 R\n/Prev {prev}\n>>\nstartxref\n{xref_offset}\n%%EOF\n",
            prev = scan.startxref_offset
        )
        .as_bytes(),
    );

    Ok((buf, placeholder_offset))
}

/// Update the ByteRange placeholder with actual values.
fn update_byte_range(
    buf: &mut [u8],
    before_sig: usize,
    after_sig: usize,
    total_len: usize,
) -> Result<(), FormeError> {
    // Find the ByteRange placeholder in the buffer
    let needle = b"/ByteRange [0 0000000000 0000000000 0000000000]";
    let pos = find_bytes(buf, needle).ok_or_else(|| {
        FormeError::RenderError("ByteRange placeholder not found in output".to_string())
    })?;

    // Format the actual byte range with same total width
    let br_str = format!(
        "/ByteRange [0 {:>10} {:>10} {:>10}]",
        before_sig,
        after_sig,
        total_len - after_sig
    );
    let br_bytes = br_str.as_bytes();

    // Verify lengths match
    if br_bytes.len() != needle.len() {
        return Err(FormeError::RenderError(format!(
            "ByteRange replacement length mismatch: {} vs {}",
            br_bytes.len(),
            needle.len()
        )));
    }

    buf[pos..pos + br_bytes.len()].copy_from_slice(br_bytes);
    Ok(())
}

/// Find a byte sequence in a buffer.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Build a PKCS#7 SignedData structure (DER-encoded).
///
/// This is a minimal but valid CMS SignedData for PDF signatures:
/// - version 1
/// - digestAlgorithms: SHA-256
/// - encapContentInfo: id-data (detached, no content)
/// - certificates: the signing certificate
/// - signerInfos: one signer with RSA signature
fn build_pkcs7_signed_data(
    cert_der: &[u8],
    signature_bytes: &[u8],
    _hash: &[u8],
) -> Result<Vec<u8>, FormeError> {
    // We build the DER manually since the `cms` crate API can be tricky.
    // PKCS#7 SignedData structure:
    //
    // ContentInfo {
    //   contentType: id-signedData (1.2.840.113549.1.7.2)
    //   content: SignedData {
    //     version: 1
    //     digestAlgorithms: { sha-256 }
    //     encapContentInfo: { id-data }  (detached)
    //     certificates: [0] IMPLICIT { cert }
    //     signerInfos: {
    //       SignerInfo {
    //         version: 1
    //         issuerAndSerialNumber: { issuer, serial }
    //         digestAlgorithm: sha-256
    //         signatureAlgorithm: rsaEncryption
    //         signature: <bytes>
    //       }
    //     }
    //   }
    // }

    use der::Decode;
    let cert = x509_cert::Certificate::from_der(cert_der)
        .map_err(|e| FormeError::RenderError(format!("Failed to parse cert DER: {e}")))?;

    let issuer_der = cert
        .tbs_certificate
        .issuer
        .to_der()
        .map_err(|e| FormeError::RenderError(format!("Failed to encode issuer: {e}")))?;
    let serial_der = cert.tbs_certificate.serial_number.as_bytes();

    // OIDs
    let oid_signed_data: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
    ]; // 1.2.840.113549.1.7.2
    let oid_data: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01,
    ]; // 1.2.840.113549.1.7.1
    let oid_sha256: &[u8] = &[
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
    ]; // 2.16.840.1.101.3.4.2.1
    let oid_rsa: &[u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
    ]; // 1.2.840.113549.1.1.1

    // Build SignerInfo
    let signer_info = {
        let mut si = Vec::new();
        // version INTEGER 1
        si.extend_from_slice(&der_integer(1));
        // issuerAndSerialNumber SEQUENCE { issuer, serial }
        let mut ias = Vec::new();
        ias.extend_from_slice(&issuer_der);
        ias.extend_from_slice(&der_integer_bytes(serial_der));
        si.extend_from_slice(&der_sequence(&ias));
        // digestAlgorithm AlgorithmIdentifier { sha-256, NULL }
        let mut da = Vec::new();
        da.extend_from_slice(oid_sha256);
        da.extend_from_slice(&[0x05, 0x00]); // NULL
        si.extend_from_slice(&der_sequence(&da));
        // signatureAlgorithm AlgorithmIdentifier { rsaEncryption, NULL }
        let mut sa = Vec::new();
        sa.extend_from_slice(oid_rsa);
        sa.extend_from_slice(&[0x05, 0x00]); // NULL
        si.extend_from_slice(&der_sequence(&sa));
        // signature OCTET STRING
        si.extend_from_slice(&der_octet_string(signature_bytes));

        der_sequence(&si)
    };

    // Build SignedData
    let signed_data = {
        let mut sd = Vec::new();
        // version INTEGER 1
        sd.extend_from_slice(&der_integer(1));
        // digestAlgorithms SET OF { AlgorithmIdentifier }
        let mut da_set_content = Vec::new();
        let mut alg_id = Vec::new();
        alg_id.extend_from_slice(oid_sha256);
        alg_id.extend_from_slice(&[0x05, 0x00]);
        da_set_content.extend_from_slice(&der_sequence(&alg_id));
        sd.extend_from_slice(&der_set(&da_set_content));
        // encapContentInfo SEQUENCE { id-data } (detached — no content)
        let mut eci = Vec::new();
        eci.extend_from_slice(oid_data);
        sd.extend_from_slice(&der_sequence(&eci));
        // certificates [0] IMPLICIT SET OF Certificate
        sd.extend_from_slice(&der_context_constructed(0, cert_der));
        // signerInfos SET OF SignerInfo
        let mut si_set = Vec::new();
        si_set.extend_from_slice(&signer_info);
        sd.extend_from_slice(&der_set(&si_set));

        der_sequence(&sd)
    };

    // Build ContentInfo
    let content_info = {
        let mut ci = Vec::new();
        ci.extend_from_slice(oid_signed_data);
        // [0] EXPLICIT SignedData
        ci.extend_from_slice(&der_context_constructed(0, &signed_data));
        der_sequence(&ci)
    };

    Ok(content_info)
}

// --- DER encoding helpers ---

fn der_integer(value: i64) -> Vec<u8> {
    if (0..=127).contains(&value) {
        vec![0x02, 0x01, value as u8]
    } else {
        let bytes = value.to_be_bytes();
        // Find first non-zero (or non-0xFF for negatives) byte
        let start = bytes
            .iter()
            .position(|&b| if value >= 0 { b != 0 } else { b != 0xFF })
            .unwrap_or(bytes.len() - 1);
        let significant = &bytes[start..];
        // Add leading zero if high bit is set on positive number
        if value >= 0 && significant[0] & 0x80 != 0 {
            let mut result = vec![0x02];
            result.extend_from_slice(&der_length(significant.len() + 1));
            result.push(0x00);
            result.extend_from_slice(significant);
            result
        } else {
            let mut result = vec![0x02];
            result.extend_from_slice(&der_length(significant.len()));
            result.extend_from_slice(significant);
            result
        }
    }
}

fn der_integer_bytes(bytes: &[u8]) -> Vec<u8> {
    // INTEGER from raw bytes (for serial numbers)
    let mut result = vec![0x02];
    // If high bit is set, prepend a zero byte
    if !bytes.is_empty() && bytes[0] & 0x80 != 0 {
        result.extend_from_slice(&der_length(bytes.len() + 1));
        result.push(0x00);
    } else {
        result.extend_from_slice(&der_length(bytes.len()));
    }
    result.extend_from_slice(bytes);
    result
}

fn der_octet_string(data: &[u8]) -> Vec<u8> {
    let mut result = vec![0x04];
    result.extend_from_slice(&der_length(data.len()));
    result.extend_from_slice(data);
    result
}

fn der_sequence(content: &[u8]) -> Vec<u8> {
    let mut result = vec![0x30];
    result.extend_from_slice(&der_length(content.len()));
    result.extend_from_slice(content);
    result
}

fn der_set(content: &[u8]) -> Vec<u8> {
    let mut result = vec![0x31];
    result.extend_from_slice(&der_length(content.len()));
    result.extend_from_slice(content);
    result
}

fn der_context_constructed(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut result = vec![0xA0 | tag];
    result.extend_from_slice(&der_length(content.len()));
    result.extend_from_slice(content);
    result
}

fn der_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len < 0x100 {
        vec![0x81, len as u8]
    } else if len < 0x10000 {
        vec![0x82, (len >> 8) as u8, len as u8]
    } else if len < 0x1000000 {
        vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
    } else {
        vec![
            0x84,
            (len >> 24) as u8,
            (len >> 16) as u8,
            (len >> 8) as u8,
            len as u8,
        ]
    }
}

/// Hex-encode bytes to uppercase hex string.
fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

/// Escape special characters in a PDF string.
fn escape_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// Get current Unix timestamp in seconds, portable across native and WASM.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(super) fn current_timestamp_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(super) fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format current time as PDF date string: D:YYYYMMDDHHmmss+00'00'
pub(super) fn format_pdf_date() -> String {
    let now = current_timestamp_secs();

    // Convert epoch seconds to date components (UTC)
    let days = now / 86400;
    let time_of_day = now % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to year/month/day
    let (year, month, day) = epoch_days_to_ymd(days);

    format!("D:{year:04}{month:02}{day:02}{hours:02}{minutes:02}{seconds:02}+00'00'")
}

/// Convert days since 1970-01-01 to (year, month, day).
pub(super) fn epoch_days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil calendar algorithm from Howard Hinnant
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Format current time as a human-readable display date: YYYY-MM-DD HH:MM UTC
fn format_display_date() -> String {
    let now = current_timestamp_secs();

    let days = now / 86400;
    let time_of_day = now % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let (year, month, day) = epoch_days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02} UTC")
}

/// Parse a DER tag-length-value header, returning (tag, content_length, header_size).
/// Handles both single-byte and multi-byte DER length encoding.
fn parse_der_tag_length(bytes: &[u8]) -> Option<(u8, usize, usize)> {
    if bytes.len() < 2 {
        return None;
    }
    let tag = bytes[0];
    let first = bytes[1];
    if first < 0x80 {
        // Short form: length is a single byte
        Some((tag, first as usize, 2))
    } else {
        // Long form: first byte = 0x80 | num_length_bytes
        let num_bytes = (first & 0x7F) as usize;
        if num_bytes == 0 || num_bytes > 4 || bytes.len() < 2 + num_bytes {
            return None;
        }
        let mut len: usize = 0;
        for i in 0..num_bytes {
            len = (len << 8) | (bytes[2 + i] as usize);
        }
        Some((tag, len, 2 + num_bytes))
    }
}

/// Extract the Common Name (CN) from a DER-encoded X.509 certificate's Subject.
fn extract_cn_from_cert_der(cert_der: &[u8]) -> Option<String> {
    use der::Decode;
    let cert = x509_cert::Certificate::from_der(cert_der).ok()?;

    // OID for CommonName: 2.5.4.3
    let cn_oid = const_oid::ObjectIdentifier::new_unwrap("2.5.4.3");

    // Walk the RDN sequence looking for CN
    for rdn in cert.tbs_certificate.subject.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid == cn_oid {
                // The value is an ANY — try to extract as UTF8String or PrintableString
                let value_bytes = atv.value.to_der().ok()?;
                let (tag, len, hdr) = parse_der_tag_length(&value_bytes)?;
                if value_bytes.len() >= hdr + len {
                    let s = std::str::from_utf8(&value_bytes[hdr..hdr + len]).ok()?;
                    // Filter for UTF8String (0x0C), PrintableString (0x13), IA5String (0x16)
                    if tag == 0x0C || tag == 0x13 || tag == 0x16 {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
}

// --- Catalog parsing helpers ---

/// Find the /Pages reference in the original catalog object.
fn find_catalog_pages_ref(text: &str, root_obj: usize) -> Option<usize> {
    find_catalog_ref(text, root_obj, "/Pages")
}

/// Find an object reference for a key within a specific object.
fn find_catalog_ref(text: &str, obj_id: usize, key: &str) -> Option<usize> {
    let obj_header = format!("{obj_id} 0 obj");
    let obj_start = text.find(&obj_header)?;
    let obj_section = &text[obj_start..];
    let obj_end = obj_section.find("endobj")?;
    let obj_content = &obj_section[..obj_end];

    let key_pos = obj_content.find(key)?;
    let after_key = &obj_content[key_pos + key.len()..];
    let trimmed = after_key.trim_start();
    // Parse "N 0 R"
    let end = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if end == 0 {
        return None;
    }
    trimmed[..end].parse().ok()
}

/// Check if a key exists in a catalog object.
fn catalog_has_key(text: &str, obj_id: usize, key: &str) -> bool {
    let obj_header = format!("{obj_id} 0 obj");
    if let Some(obj_start) = text.find(&obj_header) {
        let obj_section = &text[obj_start..];
        if let Some(obj_end) = obj_section.find("endobj") {
            return obj_section[..obj_end].contains(key);
        }
    }
    false
}

/// Find a string value for a key in a catalog object (e.g., /Lang (en-US)).
fn find_catalog_string(text: &str, obj_id: usize, key: &str) -> Option<String> {
    let obj_header = format!("{obj_id} 0 obj");
    let obj_start = text.find(&obj_header)?;
    let obj_section = &text[obj_start..];
    let obj_end = obj_section.find("endobj")?;
    let obj_content = &obj_section[..obj_end];

    let key_pos = obj_content.find(key)?;
    let after_key = &obj_content[key_pos + key.len()..];
    let trimmed = after_key.trim_start();
    if !trimmed.starts_with('(') {
        return None;
    }
    let end = trimmed[1..].find(')')? + 1;
    Some(trimmed[1..end].to_string())
}

/// Metadata extracted from an existing AcroForm dictionary.
struct AcroFormMetadata {
    need_appearances: bool,
    da: Option<String>,
}

/// Extract AcroForm metadata (/NeedAppearances, /DA) from the original PDF's catalog.
fn find_existing_acroform_metadata(pdf: &[u8], root_obj: usize) -> AcroFormMetadata {
    let text = String::from_utf8_lossy(pdf);
    let obj_header = format!("{root_obj} 0 obj");
    let obj_start = match text.find(&obj_header) {
        Some(pos) => pos,
        None => {
            return AcroFormMetadata {
                need_appearances: false,
                da: None,
            }
        }
    };
    let obj_section = &text[obj_start..];
    let obj_end = match obj_section.find("endobj") {
        Some(pos) => pos,
        None => {
            return AcroFormMetadata {
                need_appearances: false,
                da: None,
            }
        }
    };
    let obj_content = &obj_section[..obj_end];

    let acroform_pos = match obj_content.find("/AcroForm") {
        Some(pos) => pos,
        None => {
            return AcroFormMetadata {
                need_appearances: false,
                da: None,
            }
        }
    };
    let after_acroform = &obj_content[acroform_pos..];

    let need_appearances = after_acroform.contains("/NeedAppearances true");

    // Extract /DA (default appearance) string
    let da = if let Some(da_pos) = after_acroform.find("/DA") {
        let after_da = after_acroform[da_pos + 3..].trim_start();
        if let Some(stripped) = after_da.strip_prefix('(') {
            stripped.find(')').map(|end| stripped[..end].to_string())
        } else {
            None
        }
    } else {
        None
    };

    AcroFormMetadata {
        need_appearances,
        da,
    }
}

/// Determine the next unique signature field name by scanning for existing `/T (SignatureN)` entries.
fn next_signature_name(pdf: &[u8]) -> String {
    let text = String::from_utf8_lossy(pdf);
    let mut max_num = 0u32;
    let prefix = "/T (Signature";
    let mut pos = 0;
    while let Some(idx) = text[pos..].find(prefix) {
        let after = &text[pos + idx + prefix.len()..];
        if let Some(end) = after.find(')') {
            if let Ok(n) = after[..end].parse::<u32>() {
                max_num = max_num.max(n);
            }
        }
        pos = pos + idx + prefix.len();
    }
    format!("Signature{}", max_num + 1)
}

/// Find existing AcroForm /Fields references from the original PDF.
/// Returns a list of "N 0 R" strings for each existing field.
fn find_existing_acroform_fields(pdf: &[u8], root_obj: usize) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf);
    let obj_header = format!("{root_obj} 0 obj");
    let obj_start = match text.find(&obj_header) {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    let obj_section = &text[obj_start..];
    let obj_end = match obj_section.find("endobj") {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    let obj_content = &obj_section[..obj_end];

    // Find /AcroForm in the catalog
    let acroform_pos = match obj_content.find("/AcroForm") {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    let after_acroform = &obj_content[acroform_pos..];

    // Look for /Fields array within the AcroForm dict
    let fields_pos = match after_acroform.find("/Fields") {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    let after_fields = &after_acroform[fields_pos + 7..]; // skip "/Fields"
    let trimmed = after_fields.trim_start();
    if !trimmed.starts_with('[') {
        return Vec::new();
    }
    let bracket_end = match trimmed.find(']') {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    let fields_content = &trimmed[1..bracket_end];

    // Parse "N 0 R" references
    let mut fields = Vec::new();
    let mut remaining = fields_content.trim();
    while !remaining.is_empty() {
        // Parse object number
        let end = remaining
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(remaining.len());
        if end == 0 {
            break;
        }
        let obj_num = &remaining[..end];
        remaining = remaining[end..].trim_start();
        // Expect "0 R"
        if remaining.starts_with("0 R") {
            fields.push(format!("{obj_num} 0 R"));
            remaining = remaining[3..].trim_start();
        } else {
            break;
        }
    }
    fields
}

/// Find an array value for a key in a catalog object (returns the raw "[...]" string).
fn find_catalog_array_content(text: &str, obj_id: usize, key: &str) -> Option<String> {
    let obj_header = format!("{obj_id} 0 obj");
    let obj_start = text.find(&obj_header)?;
    let obj_section = &text[obj_start..];
    let obj_end = obj_section.find("endobj")?;
    let obj_content = &obj_section[..obj_end];

    let key_pos = obj_content.find(key)?;
    let after_key = &obj_content[key_pos + key.len()..];
    let trimmed = after_key.trim_start();
    if !trimmed.starts_with('[') {
        return None;
    }
    let end = trimmed.find(']')? + 1;
    Some(trimmed[..end].to_string())
}
