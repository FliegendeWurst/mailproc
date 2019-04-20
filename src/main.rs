extern crate toml;
extern crate regex;
extern crate dirs;
extern crate mailparse;
#[macro_use]
extern crate serde_derive;

use std::io::Read;
use std::fs::File;
use std::path::PathBuf;
use std::collections::HashMap;
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use mailparse::*;
use subprocess::{Popen, Redirection, PopenConfig};

#[derive(Deserialize,Clone,Debug)]
struct Rule {
    headers: Option<Vec<HashMap<String, String>>>,
    body: Option<Vec<Vec<String>>>,
    raw: Option<Vec<Vec<String>>>,
    action: Option<Vec<Vec<String>>>,
    filter: Option<Vec<String>>,
}

struct Match {
    headers: bool,
    body: bool,
    raw: bool,
}
impl Match {
    fn matched(&self) -> bool {
        (self.headers && self.body && self.raw)
    }
}

struct Job {
    subprocess: Popen,
    stdout: Option<Vec<u8>>,
    stderr: Option<Vec<u8>>,
}

impl Job {
    fn run(action: &Vec<String>, input: &Vec<u8>) -> Job {

        let mut p = Popen::create(action, PopenConfig {
            stdin:  Redirection::Pipe,
            stdout: Redirection::Pipe,
            stderr: Redirection::Pipe,
            ..Default::default()
        }).expect("Could not spawn child process");

        let mut stdout = None;
        let mut stderr = None;
        match p.communicate_bytes(Some(input)) {
            Ok((out, err)) => {
                stdout = out;
                stderr = err;
            }
            _ => ()
        }
        let _ = p.wait();

        let job = Job {
            subprocess: p,
            stdout: stdout,
            stderr: stderr,
        };
        job

    }
}

#[derive(Deserialize,Clone)]
struct Config {
    version: usize,
    rules: Vec<Rule>,       
}

fn get_config() -> Config {
    let mut conf = match dirs::home_dir() {
        Some(path) => path,
        _ => PathBuf::from(""),
    };
    conf.push(".mailproc.conf");
    let mut f = File::open(&conf)
        .expect(&format!("Could not open config file {:?}.", &conf));
    let mut buf = String::new();
    f.read_to_string(&mut buf).unwrap();
    let config: Config = toml::from_str(&buf)
        .expect(&format!("Could not parse config file {:?}.", &conf));
    config
}


fn main() {
    let config = get_config();
    let mut input_buf = Vec::<u8>::new();
    std::io::stdin().read_to_end(&mut input_buf).unwrap();
    let parsed_mail = mailparse::parse_mail(&input_buf).unwrap();

    for rule in config.rules {
        println!("Testing rule: {:?}", rule);
        
        // If there is a filter, then run it and collect the output
        let mut filter_res = match rule.filter {
            None => None,
            Some(ref filter) => Some(Job::run(&filter, &input_buf)),
        };

        // If there was a filter, then grab its output if it was successful
        let filter_buffer = match filter_res {
            Some(ref mut job) if job.subprocess.exit_status().is_some() && 
                job.subprocess.exit_status().unwrap().success()=> {
                    Some(job.stdout.take().unwrap())
                },
            Some(ref job) => {
                println!("Rule filter failed: {:?} => {:?}: {:?}",
                         rule.filter,
                         job.subprocess.exit_status(),
                         job.stderr);
                None
            }
            _ => None,
        };

        // Parse the output from the filter if there was one
        let filter_parsed = match filter_buffer {
            Some(ref filtered) => Some(mailparse::parse_mail(filtered).unwrap()),
            _ => None,
        };

        // Assign the input buffer to be the original or filtered message content
        let buffer = match filter_buffer {
            Some(ref b) => &b,
            _ => &input_buf,
        };

        // Assign the mailparse ref to be the original or filtered output
        let parsed = match filter_parsed {
            Some(ref p) => &p,
            _ => &parsed_mail,
        };

        // And start the business of matching. 
        // Create a Match struct for each of the message parts we can match,
        // then for each of those parts, test all the rules.
        let mut mail_match = Match {
            headers: rule.headers.is_none(),
            body: rule.body.is_none(),
            raw: rule.raw.is_none(),
        };

        if let Some(ref headers_vec) = rule.headers {
            for headers_set in headers_vec {
                let mut doaction = true;
                for (k, v) in headers_set {
                    let re = Regex::new(&v)
                        .expect(&format!("Could not compile regex: {}", v));
                    doaction &= match parsed.headers.get_first_value(&k) {
                        Ok(Some(ref h)) => re.is_match(h),
                        _ => false,
                    }
                }
                mail_match.headers |= doaction;
            }
        }

        if let Some(ref body_vec) = rule.body {
            for body_set in body_vec {
                let mut doaction = true;
                for body_re in body_set {
                    let re = Regex::new(&body_re)
                        .expect(&format!("Could not compile regex: {}", body_re));
                    doaction &= match parsed.get_body() {
                        Ok(ref b) => re.is_match(b),
                        _ => false,
                    }
                }
                mail_match.body |= doaction;
            }
        }

        if let Some(ref raw_vec) = rule.raw {
            for raw_set in raw_vec {
                let mut doaction = true;
                for raw_re in raw_set {
                    let re = BytesRegex::new(&raw_re)
                        .expect(&format!("Could not compile regex: {}", raw_re));
                    doaction &= re.is_match(&buffer);
                }
                mail_match.raw |= doaction;
            }
        }

        if mail_match.matched() {
            println!("Matched rule: {:?}", rule);
            if let Some(actions) = rule.action {
                for action in actions {
                    println!("Doing action {:?}", action);
                    let job = Job::run(&action, &buffer);
                    println!("Result: {:?}", job.subprocess.exit_status());
                }
            } else {
                println!("No action, message dropped");
            }
            // break rule processing loop
            break;
        }
    }
}
