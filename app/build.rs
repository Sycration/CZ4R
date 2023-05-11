
use ructe::Ructe;
use std::env;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ructe = Ructe::from_env().unwrap();
    ructe.compile_templates("templates").unwrap();

    Ok(())
}

