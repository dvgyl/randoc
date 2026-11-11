//! A test that the Lua sources are the files of lua.org.
//!
//! ADR 11 permits the copy of the Lua 5.4.8 sources on one condition: each
//! file stays byte-identical to the file in the folder `src` of
//! `lua-5.4.8.tar.gz` (SHA-256
//! `4f18ddae154e793e46eeab727c59ef1c0c0c2b744e7b94219710d76f530629ae`).

/// The SHA-256 of each file of `lua-5.4.8/`, computed from the archive.
const SOURCE_HASHES: [(&str, &str); 59] = [
    (
        "lapi.c",
        "6afef609bc1d93280a1da898c82734b14ca94d98f72d786c397c5988e6cc6e98",
    ),
    (
        "lauxlib.c",
        "50ef12f21926fa3b80d1e9e611c5e4669012efe792b5a3515fd5a7ee1cc796e3",
    ),
    (
        "lbaselib.c",
        "5dc9a64468c1ac31e59066c7d3597eeb8ea4ac8fc19a257a74c431624300b487",
    ),
    (
        "lcode.c",
        "2cf13c7f8205e9f07c2ccf1fedc3f0f0a05d238c194b176c72cc22b2cc96210e",
    ),
    (
        "lcorolib.c",
        "5408ccec737678d045d421f84a56e1ce03192a42c55440ffef07bec356aaec89",
    ),
    (
        "lctype.c",
        "3e21ae6a8faab3ed470ae0de19360da6b4e21a0a0f8572f502f7e13d590186f8",
    ),
    (
        "ldblib.c",
        "349afd63c8be2cb8a522e4a04e9f444b65ed1fc7f4dabba5b4b61fd8c11d0c42",
    ),
    (
        "ldebug.c",
        "06d065d1f26c29546cc74b61a50267cb70567ae538691f190bf15ca5cd29d308",
    ),
    (
        "ldo.c",
        "ff588c58143d3dd75157004a5a785044beb9a6746378ccc6e46d4b3a303657c2",
    ),
    (
        "ldump.c",
        "3c05402c5e282e06c2218f2469b1f38e1b84c783b9387aec7740c60b287d82c9",
    ),
    (
        "lfunc.c",
        "38809428b76fafa79db9f65b7248cc0c02c9b696e7410fc4ab1699940768f1b2",
    ),
    (
        "lgc.c",
        "bf28b521954d66ba02a49569c0f7bf37ebdd4f0a62fde4312284e817fb1e42fe",
    ),
    (
        "linit.c",
        "3578568e27b9c1e2f4a7f9a53fdb4bc8ac095db75cc86e966bb1deec2792158e",
    ),
    (
        "liolib.c",
        "bd3176d1c5d30d61fdfc2aa17d7af0819f2da51694942a24b6c185ef797f0060",
    ),
    (
        "llex.c",
        "5cfbaabf3dc0d75835aaafc37fcd87a92f3dba451e002732747b602fda122958",
    ),
    (
        "lmathlib.c",
        "da5c7d74cffce22f0a9eb468e6e7e80b543ea6bec5c21aefb8cf285a3556722a",
    ),
    (
        "lmem.c",
        "6154c2865bec97e61a92d8284be7e8ba21818f6edc95f5d72adb5dcf3e9532ca",
    ),
    (
        "loadlib.c",
        "f21cafa3258669d6f8c4f09195c654a4a588ab402f5e976c731918658b747a5b",
    ),
    (
        "lobject.c",
        "7a9748e38c52e45bc468cae74ccea08b98f8c53a523c1b0f9a117e5cf162c921",
    ),
    (
        "lopcodes.c",
        "bdc9d39fd8bf1a350c0249f726c5f61bef64b122b93a0fbc172b0328dc01f594",
    ),
    (
        "loslib.c",
        "474f2a3ee7ca8083e4552177da426ea159be82b4b34f3bd2151239810b57bb0e",
    ),
    (
        "lparser.c",
        "c5507307586c1a2336a7722229f7b350ae9c548b854933b370ad150e9e6e7f84",
    ),
    (
        "lstate.c",
        "5b2003a6bd7c8d98af10f4851ced899581130d5ba9db831cedcc7ac53314e806",
    ),
    (
        "lstring.c",
        "b6162f3a65735a902911ffafcafe1db0994c3b0df441581dcbb04dca6bc0b71d",
    ),
    (
        "lstrlib.c",
        "3257cd857653560773947d70b65c8b8bf8ef43f920ed449587fa662224d859e1",
    ),
    (
        "ltable.c",
        "27a711276b1e22aaea3b9182e2d17693f598cfe9bc7fd203c05d400df6673853",
    ),
    (
        "ltablib.c",
        "3d430a6f9cde66d0b791444c6164e7559ccf62b7cdc3a6325882408c6806e26c",
    ),
    (
        "ltm.c",
        "7cbe645ed7e1161d31eb32ea467d8220b592c0acfa6107f0194487f0ddbf954b",
    ),
    (
        "lundump.c",
        "7f7b078026e162161d402feba73a69cbe66a98558ec0745a445a595f42eac688",
    ),
    (
        "lutf8lib.c",
        "ed24c3faa8abda78ee6df53f011475ab9565a2e3ace64e714bd412c90160742c",
    ),
    (
        "lvm.c",
        "88b10a2f1f539cdfbefac818c64ceee59ac1b5f55038643637109a98834bb926",
    ),
    (
        "lzio.c",
        "fba61ceeeb257a3fe5f63310f21e14fc26c4be647583dcf8e74cbc7fb7e43e8a",
    ),
    (
        "lapi.h",
        "9522209670086adfbb11379416078a7f4dafe154043e320cd435fa1811aaabfb",
    ),
    (
        "lauxlib.h",
        "2d9e18e577a6062646268e7f7228264c28bd9b972c186e1af81b8357179c0ab4",
    ),
    (
        "lcode.h",
        "c5ad73fb12fd3e9f6b6a5f37fea5b60e4c09f8a262e9ac3ca85e8e047f745cea",
    ),
    (
        "lctype.h",
        "c800311872ecba642f6d013b290eda83ba6cd8df8061e25cdca1dd4adcbed8b5",
    ),
    (
        "ldebug.h",
        "b0fd38f6461251ec481cf5633d622569cef939067e37cac23331d9f96b601702",
    ),
    (
        "ldo.h",
        "f150c09811f1d1f302dd9eafea36d4003a834c7af24fb6329b4e9a1970c3dfec",
    ),
    (
        "lfunc.h",
        "81dc02d55ae8000461ab90bb9689d2600ba51f2ea9ac9fb019386995c07e3e4a",
    ),
    (
        "lgc.h",
        "c83d3b8e31345c0aacd37442954a7f2434312ee48f98a8b9ae4fc8aad26d0c99",
    ),
    (
        "ljumptab.h",
        "6c3e9858b9dad03ccd61445ab0bff0eb6ddfa5e731e044f93350fe6c6b2ad8d5",
    ),
    (
        "llex.h",
        "5ce0302528260ed6a40ec3ebf5e27d3bc108242d9dca57e8cdbe80fe34e13410",
    ),
    (
        "llimits.h",
        "d588bdac3c8e1adc88df3dc92c950a1bfa29663d15f9221f58e2aa553b05a422",
    ),
    (
        "lmem.h",
        "8318a170a1d4bf21c3cb4deb1f8661f1dc7cf4eb688e4ec9a1e7ea16bfc3b5cf",
    ),
    (
        "lobject.h",
        "8539beae7dd9a08efc09b04d64c09663e9afc0f2ad342abc612bb000c0c9be70",
    ),
    (
        "lopcodes.h",
        "b3f511933be3039a4b92ba79601b2e5daf75b34f268bfc757adf3e24ea4f1ec9",
    ),
    (
        "lopnames.h",
        "fbdbebc96b136efc6165cba1ac2b311b0c52413aaef59ae9ec750748f13c9e9a",
    ),
    (
        "lparser.h",
        "68c34ef2d6e5a227f7cff18436a42c6739b1470ce12f059f168d9bb69fc8d49c",
    ),
    (
        "lprefix.h",
        "626785d4eda75e9435f0ac21a780ae3b11f8b27dc36934e637a5c2305f906531",
    ),
    (
        "lstate.h",
        "ac1f8464e0d0dc6c9d22220bc3b5ec682c91de81a661bfb5f971e22b8fce0a9a",
    ),
    (
        "lstring.h",
        "a8cb0d737276f6a41071a0776ee1d8aa13cf02fa7c79ea096d202b14ea792e31",
    ),
    (
        "ltable.h",
        "1c8736b87aa408ef2afebfe693f46ae5424d658d798fb2a8a88b6868cf5569b5",
    ),
    (
        "ltm.h",
        "5b2f6e59951edeef6e71bebe5ac4351f4b3602ee0461729308b80dcee8cea910",
    ),
    (
        "lua.h",
        "3b77329f9deed929a5cbe7a7d5fc81ebbb63fd869a49e0a9302fe9240caf1930",
    ),
    (
        "luaconf.h",
        "af243c94bee6d2601383e8b65ed66a0d1b4ab3b7418ff903fcd0679912160e3a",
    ),
    (
        "lualib.h",
        "4ea0f67d3e467f5d6a327616e22a3c2da8e03d47a3c3660916275f2842f521ea",
    ),
    (
        "lundump.h",
        "1137c0c9d8654ce223374d21c52a599fd1cc1ccb869801f87a27238a7bf55e8e",
    ),
    (
        "lvm.h",
        "aba601b887236d5e135392e0c49370df3b5a9ac5c66ab2d44156b3244f2dcef3",
    ),
    (
        "lzio.h",
        "9673f62eac2d0fe3010f9d23148be7b8113319fdf4c4490015cd9feae2ecaad4",
    ),
];

/// Returns the SHA-256 of `data` as lowercase hexadecimal text.
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

#[test]
fn each_lua_source_file_is_the_file_of_lua_org() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lua-5.4.8");
    for (name, expected) in SOURCE_HASHES {
        let data = std::fs::read(folder.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(sha256_hex(&data), expected, "{name} differs from lua.org");
    }
}

#[test]
fn the_folder_holds_no_other_file() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lua-5.4.8");
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with("._"))
        .collect();
    names.sort();
    let mut expected: Vec<&str> = SOURCE_HASHES.iter().map(|(name, _)| *name).collect();
    expected.sort();
    assert_eq!(names, expected);
}
