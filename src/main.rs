//   fastar
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
use std::path::{Path, PathBuf};
use clap::{Arg, App};
use platter_walk::*;
use std::fs::File;
use tar::{Builder, Header, HeaderMode, EntryType};
use std::os::unix::io::{FromRawFd, AsRawFd};
use std::collections::HashMap;
use std::os::linux::fs::MetadataExt;

#[derive(Debug, Error)]
enum CliError {
    Io(std::io::Error),
    Nix(nix::Error),
    OutputIsATty
}

struct Config {
    starting_points: Vec<PathBuf>,
    order: Order,
    out: File,
}


fn process_args() -> std::result::Result<Config, CliError> {
    let matches = App::new("fast tar archive creator (for HDDs)")
        .version(crate_version!())
        .arg(Arg::with_name("ord").long("leaf-order").required(false).takes_value(true).possible_values(&["inode","content", "dentry"]).help("optimize order for listing/stat/reads"))
        .arg(Arg::with_name("out").short("f").required(false).takes_value(true).help("write output to file instead of stdout"))
        .arg(Arg::with_name("dirs").index(1).multiple(true).required(false).help("directories to traverse [default: cwd]"))
        .get_matches();

    let mut starting_points = matches.values_of_os("dirs").map(|it| it.map(Path::new).map(Path::to_owned).collect()).unwrap_or(vec![]);

    if starting_points.is_empty() {
        starting_points.push(std::env::current_dir()?);
    }

    let order = match matches.value_of("ord") {
        Some("inode") => Order::Inode,
        Some("content") =>Order::Content,
        Some("dentry") => Order::Dentries,
        _ => Order::Content
    };


    const STDOUT : i32 = 1;

    let out = match matches.value_of("out") {
        Some(s) => std::fs::OpenOptions::new().create(true).write(true).open(s)?,
        None => unsafe { File::from_raw_fd(STDOUT) }
    };

    if nix::unistd::isatty(out.as_raw_fd())? {
        return Err(CliError::OutputIsATty)
    }

    Ok(Config {
        out,
        starting_points,
        order
    })
}

fn archive(config: Config) -> std::result::Result<(), CliError> {

    let mut dir_scanner = ToScan::new();

    dir_scanner.set_order(config.order);

    for path in &config.starting_points {
        dir_scanner.add_root(path.to_owned())?;
    }

    dir_scanner.set_prefilter(Box::new(move |_,ft| ft.is_file()));

    let it = dir_scanner.filter_map(|e| e.ok()).map(|e| e.path().to_owned());
    let mut reap = reapfrog::MultiFileReadahead::new(it);
    reap.dropbehind(true);


    let mut builder = Builder::new(BufWriter::new(config.out));
    let mut hardlinks: HashMap<(u64, u64), PathBuf> = HashMap::new();

    loop {
        match reap.next() {
            None => break,
            Some(Err(e)) => {
                eprintln!("{}", e);
            }
            Some(Ok(mut reader)) => {
                let mut p = reader.path().to_owned();
                let meta = reader.metadata();

                for path in &config.starting_points {
                    if p.starts_with(path) {
                        p = p.strip_prefix(path).unwrap().to_owned();
                    }
                }

                if meta.file_type().is_file() && meta.st_nlink() > 1 {
                    let existing = hardlinks.entry((meta.st_dev(), meta.st_ino())).or_insert(p.clone());
                    if existing != &p {
                        // hardlinked file we already visited
                        let mut header = Header::new_gnu();
                        header.set_metadata_in_mode(&meta, HeaderMode::Deterministic);
                        header.set_entry_type(EntryType::hard_link());
                        header.set_cksum();
                        builder.append_link(&mut header, &p, &existing)?;

                        continue;
                    }
                }

                let mut header = Header::new_gnu();
                header.set_metadata_in_mode(&meta, HeaderMode::Deterministic);
                header.set_cksum();
                builder.append_data(&mut header, &p, &mut reader)?
            }
        }
    }

    builder.into_inner()?;

    Ok(())
}


fn main() -> std::result::Result<(), CliError> {
    let config = process_args()?;
    archive(config)
}


#[cfg(test)]
mod test {
    use std::fs;
    use super::*;

    #[test]
    fn test_hardlinks() {
        let tempdir = tempfile::tempdir().unwrap();
        let tmp_path = tempdir.path();

        let out = File::create(tmp_path.join("out.tar")).unwrap();
        fs::create_dir(tmp_path.join("in")).unwrap();
        fs::create_dir(tmp_path.join("unpack")).unwrap();
        File::create(tmp_path.join("in/a")).unwrap();
        fs::hard_link(tmp_path.join("in/a"), tmp_path.join("in/b")).unwrap();

        let config = Config {
            out,
            starting_points: vec![tmp_path.join("in").to_path_buf()],
            order: Order::Content
        };

        archive(config).unwrap();

        let mut archive = tar::Archive::new(File::open(tmp_path.join("out.tar")).unwrap());

        archive.unpack(tmp_path.join("unpack")).unwrap();

        assert!(tmp_path.join("unpack/a").exists());
        assert!(tmp_path.join("unpack/b").exists());

        assert_eq!(2, fs::read_dir(tmp_path.join("unpack")).unwrap().filter(|e| e.as_ref().unwrap().metadata().unwrap().st_nlink() == 2).count(), "two files, one hardlink");
    }
}