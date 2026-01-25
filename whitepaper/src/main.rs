//! Build script to generate the pqcoin whitepaper PDF using genpdf.
//!
//! Run with: cargo run --manifest-path whitepaper/Cargo.toml

use genpdf::{elements, fonts, style, Alignment, Document, Element};
use genpdf::elements::{Break, Paragraph};

fn main() {
    // Load font from system
    let font_family = fonts::from_files("/usr/share/fonts/truetype/dejavu", "DejaVuSans", None)
        .or_else(|_| fonts::from_files("/usr/share/fonts/truetype/liberation", "LiberationSans", None))
        .expect("Could not load fonts - please install dejavu or liberation fonts");

    let mut doc = Document::new(font_family);
    doc.set_title("pqcoin: A Post-Quantum Cryptocurrency");

    // Title
    doc.push(
        Paragraph::new("pqcoin: A Post-Quantum Cryptocurrency")
            .aligned(Alignment::Center)
            .styled(style::Style::new().bold().with_font_size(24)),
    );
    doc.push(Break::new(1));
    doc.push(
        Paragraph::new("Version 0.1.0")
            .aligned(Alignment::Center)
            .styled(style::Style::new().with_font_size(12)),
    );
    doc.push(Break::new(2));

    // Abstract
    doc.push(section_title("Abstract"));
    doc.push(Paragraph::new(
        "pqcoin is a proof-of-work cryptocurrency designed to provide security against both \
        classical and quantum computers. By utilizing NIST-standardized post-quantum \
        cryptographic primitives - specifically ML-DSA-87 (FIPS 204) for digital signatures \
        and SHA3-512 (FIPS 202) for hashing - pqcoin offers a quantum-resistant alternative \
        to existing cryptocurrencies that rely on elliptic curve cryptography."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "The design follows Bitcoin's proven UTXO model while making deliberate simplifications: \
        removing the scripting language in favor of two well-defined output types (P2PKH and \
        M-of-N multisig), and modernizing the cryptographic foundation for the post-quantum era."
    ));
    doc.push(Break::new(2));

    // 1. Introduction
    doc.push(section_title("1. Introduction"));
    doc.push(subsection_title("1.1 Motivation"));
    doc.push(Paragraph::new(
        "Current cryptocurrencies like Bitcoin rely on the Elliptic Curve Digital Signature \
        Algorithm (ECDSA) for transaction authorization. While secure against classical \
        computers, ECDSA is vulnerable to Shor's algorithm running on a sufficiently powerful \
        quantum computer. As quantum computing advances, the cryptocurrency ecosystem faces \
        an existential threat to its security model."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "pqcoin addresses this threat by building on cryptographic primitives that are believed \
        to be secure against both classical and quantum attacks, as standardized by NIST in \
        their Post-Quantum Cryptography project."
    ));
    doc.push(Break::new(1));

    doc.push(subsection_title("1.2 Design Goals"));
    doc.push(Paragraph::new("1. Quantum Resistance: Use NIST-standardized post-quantum cryptographic algorithms"));
    doc.push(Paragraph::new("2. Simplicity: Remove unnecessary complexity while maintaining Bitcoin's proven security model"));
    doc.push(Paragraph::new("3. Correctness: Prioritize security and correctness over feature richness"));
    doc.push(Paragraph::new("4. Compatibility: Follow familiar patterns from Bitcoin where appropriate"));
    doc.push(Break::new(2));

    // 2. Cryptographic Primitives
    doc.push(section_title("2. Cryptographic Primitives"));
    doc.push(subsection_title("2.1 Hash Function: SHA3-512 (FIPS 202)"));
    doc.push(Paragraph::new(
        "All hashing operations in pqcoin use SHA3-512, providing 512-bit output (64 bytes), \
        256-bit classical security, and ~256-bit security against Grover's algorithm."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new("SHA3-512 is used for: Transaction ID computation, Block header hashing (proof-of-work), Address derivation (hash of public key), and Merkle tree construction."));
    doc.push(Break::new(1));

    doc.push(subsection_title("2.2 Digital Signatures: ML-DSA-87 (FIPS 204)"));
    doc.push(Paragraph::new(
        "Transaction authorization uses ML-DSA-87 (formerly Dilithium5), a lattice-based \
        digital signature scheme providing NIST Level 5 security (~256-bit classical, \
        ~128-bit quantum security)."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new("Key sizes: Public Key = 2,592 bytes, Secret Key = 4,896 bytes, Signature = 4,627 bytes"));
    doc.push(Break::new(2));

    // 3. Transaction Model
    doc.push(section_title("3. Transaction Model"));
    doc.push(subsection_title("3.1 UTXO Model"));
    doc.push(Paragraph::new(
        "pqcoin uses the Unspent Transaction Output (UTXO) model: Transactions consume existing \
        outputs (inputs) and create new outputs. Each output can only be spent once. The sum of \
        input values must equal or exceed output values. The difference is the transaction fee."
    ));
    doc.push(Break::new(1));

    doc.push(subsection_title("3.2 Output Types"));
    doc.push(Paragraph::new(
        "Unlike Bitcoin's scripting system, pqcoin supports exactly two output types: \
        Pay-to-Public-Key-Hash (P2PKH) for single-signature outputs, and M-of-N Multisig \
        for multi-signature outputs (limited to 15 keys due to post-quantum signature sizes)."
    ));
    doc.push(Break::new(2));

    // 4. Block Structure
    doc.push(section_title("4. Block Structure"));
    doc.push(Paragraph::new(
        "The block header is 176 bytes: version (4 bytes), prev_hash (64 bytes), merkle_root \
        (64 bytes), timestamp (8 bytes), difficulty_bits (4 bytes), nonce (32 bytes)."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "pqcoin uses a 256-bit nonce, providing 2^256 possible values. This eliminates nonce \
        exhaustion concerns and removes the need for Bitcoin-style extraNonce mechanisms."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "Block limits: Max 1,000 transactions, 16 MB max block size, 10,000 max inputs/outputs \
        per transaction, 15 max multisig keys."
    ));
    doc.push(Break::new(2));

    // 5. Consensus Mechanism
    doc.push(section_title("5. Consensus Mechanism"));
    doc.push(Paragraph::new(
        "Valid blocks must have SHA3-512(block_header) <= target. Difficulty adjusts every \
        2,016 blocks to maintain 10-minute average block time, with adjustment clamped to 4x \
        in either direction. The chain with most cumulative proof-of-work is canonical. \
        Reorganizations are limited to 100 blocks."
    ));
    doc.push(Break::new(2));

    // 6. Monetary Policy
    doc.push(section_title("6. Monetary Policy"));
    doc.push(Paragraph::new(
        "Initial reward: 50,000,000 quanta (50 coins). Halving interval: 210,000 blocks. \
        Total supply asymptotically approaches 21 million coins. Coinbase maturity: 100 blocks."
    ));
    doc.push(Break::new(2));

    // 7. Network Protocol
    doc.push(section_title("7. Network Protocol"));
    doc.push(Paragraph::new(
        "Messages use magic bytes 'PQCN', with length-prefixed framing and SHA3-512 checksum. \
        Message types include version/verack, ping/pong, inv/getdata, getheaders/headers, \
        block/tx, and addr/addrv2."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "Network parameters: Default port 8333, max 125 connections (8 outbound), max 3 per IP, \
        max 2 per /16 subnet, 24-hour ban duration."
    ));
    doc.push(Break::new(2));

    // 8. Security Considerations
    doc.push(section_title("8. Security Considerations"));
    doc.push(Paragraph::new(
        "Quantum resistance: ML-DSA-87 (based on Module-LWE problem) and SHA3-512 (Grover's \
        algorithm provides only quadratic speedup). Classical security: 256-bit hash and \
        signature security."
    ));
    doc.push(Break::new(1));
    doc.push(Paragraph::new(
        "Known limitations: ML-DSA-87 signatures are ~72x larger than ECDSA (4,627 vs 64 bytes), \
        no scripting support, and mempool does not support chained unconfirmed transactions."
    ));
    doc.push(Break::new(2));

    // 9. Comparison with Bitcoin
    doc.push(section_title("9. Comparison with Bitcoin"));
    doc.push(Paragraph::new("pqcoin vs Bitcoin:"));
    doc.push(Paragraph::new("  - Hash: SHA3-512 (64 bytes) vs SHA-256 (32 bytes)"));
    doc.push(Paragraph::new("  - Signatures: ML-DSA-87 vs ECDSA"));
    doc.push(Paragraph::new("  - Signature size: 4,627 bytes vs 64 bytes"));
    doc.push(Paragraph::new("  - Quantum resistant: Yes vs No"));
    doc.push(Paragraph::new("  - Scripting: No (P2PKH + Multisig only) vs Yes"));
    doc.push(Paragraph::new("  - Nonce: 256 bits vs 32 bits"));
    doc.push(Paragraph::new("  - Block size: 16 MB vs ~4 MB"));
    doc.push(Break::new(2));

    // 10. Conclusion
    doc.push(section_title("10. Conclusion"));
    doc.push(Paragraph::new(
        "pqcoin provides a quantum-resistant cryptocurrency built on NIST-standardized \
        cryptographic primitives. By simplifying Bitcoin's design while upgrading its \
        cryptographic foundation, pqcoin offers a secure platform for value transfer in \
        the post-quantum era."
    ));
    doc.push(Break::new(2));

    // References
    doc.push(section_title("References"));
    doc.push(Paragraph::new("1. NIST FIPS 202: SHA-3 Standard"));
    doc.push(Paragraph::new("2. NIST FIPS 204: Module-Lattice-Based Digital Signature Standard"));
    doc.push(Paragraph::new("3. Nakamoto, S. (2008). Bitcoin: A Peer-to-Peer Electronic Cash System"));
    doc.push(Paragraph::new("4. NIST Post-Quantum Cryptography Standardization Process"));

    // Render to PDF
    doc.render_to_file("pqcoin-whitepaper.pdf")
        .expect("Failed to render PDF");

    println!("Generated pqcoin-whitepaper.pdf");
}

fn section_title(text: &str) -> impl Element {
    Paragraph::new(text)
        .styled(style::Style::new().bold().with_font_size(16))
}

fn subsection_title(text: &str) -> impl Element {
    Paragraph::new(text)
        .styled(style::Style::new().bold().with_font_size(13))
}
