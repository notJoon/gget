use clap::{Arg, Command};
use gget::fetch::PackageManager;
use gget::DEFAULT_RPC_ENDPOINT;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("gget")
        .version("0.1.0")
        .arg(
            Arg::new("add")
                .help("Package path to download.\nExample: gget add gno.land/p/demo/avl")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("DIR")
                .help("Output directory for downloaded files.\nDefault: ./gno")
                .default_value("."),
        )
        .arg(
            Arg::new("rpc-endpoint")
                .long("rpc-endpoint")
                .value_name("URL")
                .help("RPC endpoint URL.\nDefault: https://rpc.gno.land:443")
                .default_value(DEFAULT_RPC_ENDPOINT),
        )
        .get_matches();

    let pkg_path = matches.get_one::<String>("package").unwrap();
    let output_dir = matches.get_one::<String>("output").unwrap();
    let rpc_endpoint = matches.get_one::<String>("rpc-endpoint").unwrap();
    let target_path = PathBuf::from(output_dir);

    println!("Downloading package: {}", pkg_path);
    println!("Output directory: {}", output_dir);
    println!("RPC endpoint: {}", rpc_endpoint);

    let pm = PackageManager::new(Some(rpc_endpoint.to_string()));
    match pm.download_package(pkg_path, &target_path).await {
        Ok(()) => {
            println!("Download complete!");
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
