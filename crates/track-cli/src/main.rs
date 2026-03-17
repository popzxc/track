use clap::Parser;

use track_cli::cli::run_cli;

#[derive(Debug, Parser)]
#[command(
    name = "track",
    about = "Capture personal tasks into Markdown files.",
    version
)]
struct CliArgs {
    #[arg(value_name = "TEXT", num_args = 0.., allow_hyphen_values = true)]
    raw_text: Vec<String>,
}

fn main() {
    let args = CliArgs::parse();

    match run_cli(args.raw_text) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
