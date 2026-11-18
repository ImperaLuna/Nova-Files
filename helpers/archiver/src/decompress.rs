use std::fs::File;
use std::io::{self, BufReader, Read};

use crate::format::Compression;

pub fn tar_stream(file: File, compression: Compression) -> io::Result<Box<dyn Read>> {
    let file = BufReader::new(file);
    Ok(match compression {
        Compression::Plain => Box::new(file),
        Compression::Gzip => Box::new(flate2::bufread::MultiGzDecoder::new(file)),
        Compression::Xz => Box::new(lzma_rust2::XzReader::new(file, true)),
        Compression::Zstd => Box::new(zstd::stream::read::Decoder::with_buffer(file)?),
        Compression::Bzip2 => Box::new(bzip2::bufread::MultiBzDecoder::new(file)),
    })
}
