use std::{env, fs, io::Write, path::PathBuf};
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipWriter};

fn main() {
    let source = "src/aws_worker_gateway/cleanup.py";
    println!("cargo:rerun-if-changed={source}");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let file = fs::File::create(output.join("gateway-cleanup.zip")).expect("create setup archive");
    let mut archive = ZipWriter::new(file);
    archive
        .start_file(
            "index.py",
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .last_modified_time(DateTime::default())
                .unix_permissions(0o644),
        )
        .expect("start handler entry");
    archive
        .write_all(&fs::read(source).expect("read setup handler"))
        .expect("write setup handler");
    archive.finish().expect("finish setup archive");
}
