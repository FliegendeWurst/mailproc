use fs2::*;
use log::*;
use simplelog::Config as LogConfig;
use simplelog::{LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use structopt::StructOpt;
use subprocess::ExitStatus::*;

use mailproc::*;

#[derive(StructOpt, Debug)]
struct Opt {
    /// Test configuration and exit
    #[structopt(short = "t", long = "test")]
    test: bool,
}

fn init_log() {
    let mut log = match dirs_next::home_dir() {
        Some(path) => path,
        _ => PathBuf::from(""),
    };
    log.push("mailproc.log");
    let logfile = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("Could not open log file");
    logfile.lock_exclusive().expect("Could not lock log file");

    WriteLogger::init(LevelFilter::Info, LogConfig::default(), logfile)
        .expect("Could not initialize write logger");
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let opt = Opt::from_args();
    let mut conf = match dirs_next::home_dir() {
        Some(path) => path,
        _ => PathBuf::from(""),
    };
    conf.push(".mailproc.conf");
    let config = match Config::load_from_path(conf) {
        Ok(config) => config,
        Err(e) => {
            error!("Colud not read config: {}", e);
            return 1;
        }
    };

    init_log();

    if opt.test {
        let success = config.test();
        if !success {
            println!("Config FAIL");
            return 1;
        } else {
            println!("Config OK");
        }
        return 0;
    }

    let mut input_buf = Vec::<u8>::new();
    match std::io::stdin().read_to_end(&mut input_buf) {
        Ok(_) => (),
        Err(e) => {
            error!("Could not read stdin: {}", e);
            return 2;
        }
    }
    let parsed_mail = match mailparse::parse_mail(&input_buf) {
        Ok(m) => m,
        Err(e) => {
            error!("Could not parse mail: {}", e);
            return 3;
        }
    };

    if let Some((rule, buffer)) = handle(parsed_mail, &input_buf, config) {
        if let Some(ref actions) = rule.action {
            for action in actions {
                info!("Doing action: {}", action.join(" "));
                let job = Job::run(&action, Some(&buffer));
                info!(
                    "Result: {}",
                    match job.subprocess.exit_status() {
                        Some(Exited(code)) => format!("Exited: {}", code),
                        Some(Signaled(code)) => format!("Signaled: {}", code),
                        Some(Other(code)) => format!("Other: {}", code),
                        Some(Undetermined) => "Undetermined".to_string(),
                        None => "None".to_string(),
                    }
                );
            }
        } else {
            info!("No action, message dropped");
        }
    }
    
    0
}
