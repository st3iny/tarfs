use std::io::Read;

#[derive(Debug, Clone, Copy)]
pub enum Compression {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Unknown,
}

impl Compression {
    pub fn detect(mut reader: impl Read) -> std::io::Result<Compression> {
        let mut buf = [0u8; 6];
        reader.read_exact(&mut buf)?;
        Ok(match buf {
            [0x1f, 0x8b, _, _, _, _] => Compression::Gzip,
            [0x42, 0x5a, 0x68, _, _, _] => Compression::Bzip2,
            [0x28, 0xb5, 0x2f, 0xfd, _, _] => Compression::Zstd,
            [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] => Compression::Xz,
            _ => Compression::Unknown,
        })
    }
}
