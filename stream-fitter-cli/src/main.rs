use std::{
    fs::File,
    panic::{set_hook, take_hook},
    path::PathBuf,
    process::exit,
};

use serde_yaml::from_reader;
use structopt::StructOpt;
use tracing::{error, instrument};

use stream_fitter::{
    errors::FitterResult,
    pipe_fitter::{PipeFitter, PipeFitterConfig},
};

#[derive(StructOpt)]
struct StreamFitterCli {
    #[structopt(parse(from_os_str))]
    config_file: PathBuf,
}

fn entrypoint() -> FitterResult<()> {
    pretty_env_logger::try_init()?;

    let cli = StreamFitterCli::from_args();

    let fitter_config: PipeFitterConfig = from_reader(File::open(cli.config_file)?)?;

    let mut fitter = PipeFitter::from_config(fitter_config)?;

    fitter.run()
}

#[instrument]
fn main() {
    let panic_default = take_hook();
    set_hook(Box::new(move |info| {
        panic_default(info);
        exit(1);
    }));

    exit(match entrypoint() {
        Ok(_) => 0,
        Err(err) => {
            for e in err.iter_chain() {
                error!("{}", e);
            }
            1
        }
    })
}
