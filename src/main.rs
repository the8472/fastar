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

// #![cfg(feature = "alloc_system")]
// #![feature(alloc_system)]
#![cfg_attr(feature = "system_alloc", feature(alloc_system))]
#[cfg(feature = "system_alloc")]
extern crate alloc_system;
#[macro_use] extern crate clap;
#[macro_use] extern crate derive_error;
extern crate reapfrog;
extern crate platter_walk;
extern crate tar;

use std::error::Error;
use std::io::*;
use std::path::Path;
use clap::{Arg, App};
use platter_walk::*;
use tar::{Builder, Header, HeaderMode};

#[derive(Debug, Error)]
enum CliError {
    Io(std::io::Error)
}


fn process_args() -> std::result::Result<(), CliError> {
    let matches = App::new("fast file counting")
        .version(crate_version!())
        .arg(Arg::with_name("ord").long("leaf-order").required(false).takes_value(true).possible_values(&["inode","content", "dentry"]).help("optimize order for listing/stat/reads"))
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

    let out = std::io::stdout();
    let locked = out.lock();
    let mut builder = Builder::new(locked);

    loop {
        match reap.next() {
            None => break,
            Some(Err(e)) => {
                writeln!(std::io::stderr(),"{}", e.description()).unwrap();
            }
            Some(Ok(mut reader)) => {
                let mut p = reader.path().to_owned();
                //writeln!(std::io::stderr(), "before strip {}", p.to_string_lossy())?;
                for path in &starting_points {
                    if p.starts_with(path) {
                        p = p.strip_prefix(path).unwrap().to_owned();
                    }
                }
                //writeln!(std::io::stderr(), "after strip {}", p.to_string_lossy())?;

                let mut header = Header::new_gnu();
                header.set_metadata_in_mode(&reader.metadata(), HeaderMode::Deterministic);
                header.set_path(p)?;
                header.set_cksum();
                builder.append(&header, &mut reader)?
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
            writeln!(std::io::stderr(),"{}", e.description()).unwrap();
            std::io::stderr().flush().unwrap();
            std::process::exit(1);
        }
    };
}