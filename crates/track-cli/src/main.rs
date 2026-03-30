fn main() {
    match track_cli::cli::run_from_os_args(std::env::args_os().collect()) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
