//! SHA-256 streaming and one-shot implementations (NIST-compatible).

use std::io::{self, Read};
use std::path::Path;

/// Streams a file through the incremental SHA-256 state.
///
/// # Errors
///
/// Returns the underlying I/O error when the file cannot be opened or read.
#[allow(clippy::large_stack_arrays)] // 64 KiB streaming buffer, moved from core
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut state = Sha256State::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        state.update(&buffer[..read]);
    }
    Ok(state.finalize())
}

/// Incremental SHA-256 state with 64 KiB-friendly chunked updates.
pub struct Sha256State {
    h: [u32; 8],
    length: u64,
    buffer: [u8; 64],
    buffered: usize,
}

impl Sha256State {
    #[must_use]
    #[allow(clippy::new_without_default, clippy::unreadable_literal)]
    pub fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            length: 0,
            buffer: [0; 64],
            buffered: 0,
        }
    }
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.buffered != 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                compress(&mut self.h, &self.buffer);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            compress(&mut self.h, &data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }
    #[must_use]
    pub fn finalize(mut self) -> String {
        let bit_len = self.length.wrapping_mul(8);
        self.buffer[self.buffered] = 128;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            compress(&mut self.h, &self.buffer);
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut self.h, &self.buffer);
        self.h.iter().map(|v| format!("{v:08x}")).collect()
    }
}

#[must_use]
#[allow(clippy::many_single_char_names, clippy::unreadable_literal)]
pub fn sha256(data: &[u8]) -> String {
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
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut m = data.to_vec();
    m.push(128);
    while m.len() % 64 != 56 {
        m.push(0)
    }
    m.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for c in m.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([c[i * 4], c[i * 4 + 1], c[i * 4 + 2], c[i * 4 + 3]])
        }
        for i in 16..64 {
            let a = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let b = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(a)
                .wrapping_add(w[i - 7])
                .wrapping_add(b)
        }
        let (mut a, mut b, mut c0, mut d, mut e, mut f, mut g, mut z) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = z
                .wrapping_add(s)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let t2 = s.wrapping_add((a & b) ^ (a & c0) ^ (b & c0));
            z = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c0;
            c0 = b;
            b = a;
            a = t1.wrapping_add(t2)
        }
        for (i, v) in [a, b, c0, d, e, f, g, z].into_iter().enumerate() {
            h[i] = h[i].wrapping_add(v)
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[allow(clippy::many_single_char_names, clippy::unreadable_literal)]
fn compress(h: &mut [u32; 8], chunk: &[u8]) {
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
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            chunk[i * 4],
            chunk[i * 4 + 1],
            chunk[i * 4 + 2],
            chunk[i * 4 + 3],
        ])
    }
    for i in 16..64 {
        let a = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let b = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(a)
            .wrapping_add(w[i - 7])
            .wrapping_add(b)
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut z) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for i in 0..64 {
        let s = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let t1 = z
            .wrapping_add(s)
            .wrapping_add((e & f) ^ (!e & g))
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let t2 = s.wrapping_add((a & b) ^ (a & c) ^ (b & c));
        z = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2)
    }
    for (i, v) in [a, b, c, d, e, f, g, z].into_iter().enumerate() {
        h[i] = h[i].wrapping_add(v)
    }
}

#[cfg(test)]
mod tests {
    use super::{Sha256State, sha256, sha256_file};

    #[test]
    fn sha256_matches_nist_vectors_and_chunking() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut state = Sha256State::new();
        for chunk in b"abc".chunks(1) {
            state.update(chunk);
        }
        assert_eq!(state.finalize(), sha256(b"abc"));
    }

    #[test]
    fn sha256_file_matches_in_memory_hash_and_rejects_missing_files() {
        let root = std::env::temp_dir().join(format!("ocrx-primitives-hash-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("block.bin");
        let payload: Vec<u8> = (0..=255_u8).cycle().take(200_003).collect();
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            sha256(&payload),
            "streamed file hash must equal the in-memory hash across block boundaries"
        );
        assert!(sha256_file(&root.join("missing.bin")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sha256_is_length_sensitive_across_the_56_byte_padding_boundary() {
        // Lengths around 55/56/57/64 exercise every finalize() padding branch.
        // The locked property: one-shot == incremental for every length, and
        // the NIST anchors stay stable.
        for len in [55_usize, 56, 57, 63, 64, 65, 127, 128] {
            let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let mut state = Sha256State::new();
            state.update(&data[..len / 2]);
            state.update(&data[len / 2..]);
            assert_eq!(state.finalize(), sha256(&data), "len={len}");
        }
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
