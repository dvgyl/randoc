//! Tests for oracle output that holds a script of Pandoc. The repository
//! stores only the SHA-256 and the length of such output (`src/digests.json`,
//! from `tools/digests.py`), never its text.

use randoc_core::{Context, HtmlMathMethod, ObfuscationMethod, WriterOptions};
use randoc_templates::{Template, Val};

use crate::Version;
use crate::html::write;

/// Short pieces of the two scripts. `concat!` keeps them out of this file.
const MARKERS: [&str; 2] = [concat!("document", ".write("), concat!("katex", ".render(")];

/// The one permitted place of a marker: the `KATEX_SCRIPT` literal (decision D7).
const PERMITTED: (&str, &str) = ("standalone.rs", concat!("katex", ".render("));

fn options(case: &serde_json::Value) -> WriterOptions {
    let opts = &case["opts"];
    let mut options = WriterOptions::default();
    if opts["email"].as_str() == Some("javascript") {
        options.email_obfuscation = ObfuscationMethod::JavascriptObfuscation;
    }
    if let Some(url) = opts["katex"].as_str() {
        options.html_math_method = HtmlMathMethod::KaTex(url.into());
    }
    for (key, value) in opts["variables"].as_object().into_iter().flatten() {
        options
            .variables
            .insert(key.clone(), Val::text(value.as_str().unwrap_or_default()));
    }
    if let Some(source) = case["template"].as_str() {
        let mut load = |_: &str| -> Result<String, &str> { Err("no partial") };
        options.template = Some(Template::compile("probe", source, 64, &mut load).unwrap());
    }
    options
}

#[test]
fn the_writer_output_has_the_oracle_digests() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/digests.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 6, "only {} cases", cases.len());
    let mut failures = Vec::new();
    for case in cases {
        let doc = randoc_ast::json::from_json(case["doc"].as_str().unwrap()).unwrap();
        let version = match case["format"].as_str().unwrap() {
            "html4" => Version::Html4,
            _ => Version::Html5,
        };
        let output = write(&doc, &options(case), &mut Context::pure(), version).unwrap();
        let equal = sha256_hex(output.as_bytes()) == case["sha256"].as_str().unwrap()
            && output.len() as u64 == case["length"].as_u64().unwrap();
        // A known divergence must still differ: when it is equal, remove the mark.
        if equal == case["known"].is_string() {
            failures.push(case["id"].as_str().unwrap().to_owned());
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// No stored file of the crate and of the suite `html-writer` holds a piece
/// of the e-mail script or of the KaTeX loader script.
#[test]
fn no_stored_file_holds_a_script_of_pandoc() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dirs = [
        root.to_path_buf(),
        root.join("../../conformance/cases/html-writer"),
        root.join("../../conformance/expected/html-writer"),
    ];
    let mut files = Vec::new();
    let mut pending: Vec<std::path::PathBuf> = dirs.to_vec();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with("._") || name.starts_with('.') || name == "target" {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((name, path));
            }
        }
    }
    assert!(files.len() >= 40, "only {} files", files.len());
    let mut found = Vec::new();
    for (name, path) in &files {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        for marker in MARKERS {
            let count = text.matches(marker).count();
            let permitted = usize::from((name.as_str(), marker) == PERMITTED);
            if count != permitted {
                found.push(format!("{} {marker} {count}", path.display()));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn sha256_gives_the_standard_test_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

/// Returns the SHA-256 of `data` as lowercase hexadecimal text (FIPS 180-4).
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_length = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}
