mod sort;
use std::fs;
use log::{error};
use clap::Parser;
use crate::sort::{is_ancestor_of, Cli, Sorter};


fn print_config(args: &Cli) {
    println!("Quellordner: {:?}", args.source);
    println!("Zielordner:  {:?}", args.dest);
    if args.dry_run {
        println!("=== DRY-RUN – es wird nichts verschoben ===");
    }
}

fn validate_args(args: &Cli) {
    if !args.source.is_dir() {
        error!("Quellordner existiert nicht: {:?}", args.source);
        std::process::exit(1);
    }
    if let Err(e) = fs::create_dir_all(&args.dest) {
        error!("Zielordner kann nicht angelegt werden {:?}: {}", args.dest, e);
        std::process::exit(1);
    }
    if is_ancestor_of(&args.source, &args.dest) {
        error!("Zielordner liegt innerhalb des Quellordners!");
        std::process::exit(1);
    }
}

fn main() {
    env_logger::init();
    let args = Cli::parse();
    print_config(&args);
    validate_args(&args);
    Sorter::new(args.source, args.dest, args.dry_run).run();
}