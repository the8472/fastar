//   ffcnt
//   Copyright (C) 2017 The 8472
//
//   This program is free software; you can redistribute it and/or modify
//   it under the terms of the GNU General Public License as published by
//   the Free Software Foundation; either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU General Public License for more details.
//
//   You should have received a copy of the GNU General Public License
//   along with this program; if not, write to the Free Software Foundation,
//   Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301  USA

#[macro_use] extern crate clap;
#[macro_use] extern crate derive_error;
extern crate reapfrog;
extern crate platter_walk;
extern crate tar;
extern crate nix;

use std::io::*;
use std::path::Path;
use clap::{Arg, App};
use platter_walk::*;
use std::fs::File;
use tar::{Builder, Header, HeaderMode};
use std::os::unix::io::{FromRawFd, AsRawFd};

#[derive(Debug, Error)]
enum CliError {
    Io(std::io::Error),
    Nix(nix::Error),
    OutputIsATty
}


fn process_args() -> std::result::Result<(), CliError> {
    let matches = App::new("fast tar archive creator")
        .version(crate_version!())
        .arg(Arg::with_name("ord").long("leaf-order").required(false).takes_value(true).possible_values(&["inode","content", "dentry"]).help("optimize order for listing/stat/reads"))
        .arg(Arg::with_name("out").short("f").required(false).takes_value(true).help("write output to file instead of stdout"))
        .arg(Arg::with_name("dirs").index(1).multiple(true).required(false).help("directories to traverse [default: cwd]"))
        .get_matches();

    let mut starting_points = matches.values_of_os("dirs").map(|it| it.map(Path::new).map(Path::to_owned).collect()).unwrap_or(vec![]);

    if starting_points.is_empty() {
        starting_points.push(std::env::current_dir()?);
    }

    let mut dir_scanner = ToScan::new();

    dir_scanner.set_order(Order::Content);

    match matches.value_of("ord") {
        Some("inode") => {dir_scanner.set_order(Order::Inode);},
        Some("content") => {dir_scanner.set_order(Order::Content);}
        Some("dentry") => {dir_scanner.set_order(Order::Dentries);}
        _ => {}
    };

    for path in &starting_points {
        dir_scanner.add_root(path.to_owned())?;
    }


    dir_scanner.set_prefilter(Box::new(move |_,ft| ft.is_file()));

    let it = dir_scanner.filter_map(|e| e.ok()).map(|e| e.path().to_owned());
    let mut reap = reapfrog::MultiFileReadahead::new(it);
    reap.dropbehind(true);

    const STDOUT : i32 = 1;

    let out = match matches.value_of("out") {
        Some(s) => std::fs::OpenOptions::new().create(true).write(true).open(s)?,
        None => unsafe { File::from_raw_fd(STDOUT) }
    };

    if nix::unistd::isatty(out.as_raw_fd())? {
        return Err(CliError::OutputIsATty)
    }

    let mut builder = Builder::new(BufWriter::new(out));

    loop {
        match reap.next() {
            None => break,
            Some(Err(e)) => {
                eprintln!("{}", e);
            }
            Some(Ok(mut reader)) => {
                let mut p = reader.path().to_owned();
                let meta = reader.metadata();
                //writeln!(std::io::stderr(), "before strip {}", p.to_string_lossy())?;
                for path in &starting_points {
                    if p.starts_with(path) {
                        p = p.strip_prefix(path).unwrap().to_owned();
                    }
                }
                //writeln!(std::io::stderr(), "after strip {}", p.to_string_lossy())?;

                let mut header = Header::new_gnu();
                header.set_metadata_in_mode(&meta, HeaderMode::Deterministic);
                header.set_cksum();
                builder.append_data(&mut header, &p, &mut reader)?
            }
        }

    }

    Ok(())
}


fn main() {

    match process_args() {
        Ok(_) => {
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("{}", e);
            std::io::stderr().flush().unwrap();
            std::process::exit(1);
        }
    };
}