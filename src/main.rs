mod sort;
use std::fs;
use log::{error};
use clap::Parser;
use crate::sort::{is_ancestor_of, Cli, Sorter};

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    if !cli.source.is_dir() {
        error!("Quellordner existiert nicht: {:?}", cli.source);
        std::process::exit(1);
    }

    // Fix canonicalize()-Edge-Case: Zielordner muss existieren bevor
    // is_ancestor_of() aufgerufen wird, sonst schlägt canonicalize() fehl
    if let Err(e) = fs::create_dir_all(&cli.dest) {
        error!("Zielordner kann nicht angelegt werden {:?}: {}", cli.dest, e);
        std::process::exit(1);
    }

    if is_ancestor_of(&cli.source, &cli.dest) {
        error!("Zielordner liegt innerhalb des Quellordners!");
        std::process::exit(1);
    }

    if cli.dry_run {
        println!("=== DRY-RUN – es wird nichts verschoben ===");
    }

    let mut sorter = Sorter::new(cli.source, cli.dest, cli.dry_run);
    sorter.run();
}