// Copyright 2018 Johannes M. Griebler
//
// This file is part of mycon.
//
// mycon is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// mycon is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with mycon.  If not, see <https://www.gnu.org/licenses/>.

extern crate ansi_term;
#[macro_use]
extern crate clap;
extern crate mycon;

use std::fs::File;
use std::io::Read;
use std::process;
use std::time::Duration;

use ansi_term::Colour;
use clap::{App, Arg};

use mycon::*;

macro_rules! print_error {
    ($fmt:expr $(, $arg:expr)*) => {
        eprintln!(concat!("{} ", $fmt), Colour::Red.bold().paint("error:"), $($arg),*);
    };
}

fn run() -> i32 {
    let matches = App::new("mycon")
        .version(crate_version!())
        .author("Johannes M. Griebler <johannes.griebler@gmail.com>")
        .about("Befunge-98 interpreter")
        .arg(Arg::with_name("SOURCE_FILE")
             .help("the source file to be interpreted")
             .required(true))
        .arg(Arg::with_name("TRACE")
             .help("whether to trace command execution")
             .short("t")
             .long("trace"))
        .arg(Arg::with_name("SLEEP")
             .help("duration to sleep after each command, in milliseconds")
             .short("s")
             .long("sleep")
             .takes_value(true)
             .value_name("time"))
        .get_matches();

    let path = matches.value_of("SOURCE_FILE").unwrap();

    let mut code;

    {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                print_error!("The file \"{}\" could not be opened: {}", path, e);
                return 1;
            }
        };

        code = String::new();
        if let Err(e) = file.read_to_string(&mut code) {
            print_error!("The file \"{}\" could not be read: {}", path, e);
            return 1;
        }
    }

    let mut config = Config::new()
        .trace(matches.is_present("TRACE"));

    if let Some(n) = matches.value_of("SLEEP").and_then(|s| s.parse::<u64>().ok()) {
        config = config.sleep(Duration::from_millis(n));
    }

    let mut prog = Program::read(&code).config(config);

    prog.run()
}

fn main() {
    let exit = run();

    process::exit(exit);
}
