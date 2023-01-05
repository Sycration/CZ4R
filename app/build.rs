use cornucopia::{generate_live, CodegenSettings};
use postgres::{Client, NoTls};
use ructe::Ructe;
use std::env;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cornucopia()?;

    let mut ructe = Ructe::from_env().unwrap();
    ructe.compile_templates("templates").unwrap();

    Ok(())
}

fn cornucopia() -> Result<(), Box<dyn std::error::Error>> {
    let queries_path = "queries";
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let file_path = Path::new(&out_dir).join("cornucopia.rs");
    let file_path = file_path.to_str();
    let db_url = env::var_os("DATABASE_URL").unwrap();
    let db_url = db_url.to_str().unwrap();
    let settings = CodegenSettings {
        is_async: true,
        derive_ser: true,
    };
    let mut client = Client::connect(db_url, NoTls).unwrap();
    // Rerun this build script if the queries or migrations change.
    println!("cargo:rerun-if-changed={queries_path}");

    let output = generate_live(&mut client, queries_path, file_path, settings)?;

    // If Cornucopia couldn't run properly, try to display the error.
    eprintln!("{output}");

    Ok(())
}
