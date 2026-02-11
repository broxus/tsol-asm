/*
 * Copyright (C) 2023 EverX. All Rights Reserved.
 *
 * Licensed under the SOFTWARE EVALUATION License (the "License"); you may not use
 * this file except in compliance with the License.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific EVERX DEV software governing permissions and
 * limitations under the License.
 */

use clap::{Parser, Subcommand};
use tsol_asm::disasm::{disasm_ex, fmt::print_tree_of_cells, loader::Loader};
use tsol_asm::Status;
use tsol_asm::{error, parse_hex_slice};
use std::{collections::HashSet, io::Write, process::ExitCode};
use tycho_types::boc::de::BocHeader;
use tycho_types::boc::de::Options;
use tycho_types::boc::Boc;
use tycho_types::prelude::{Cell, CellFamily};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Dump a boc as a tree of bitstrings
    Dump {
        /// input boc
        boc: String,
    },
    /// Extract one cell from a boc
    Extract {
        /// cell index (from 0 to 3)
        index: usize,
        /// input boc
        boc: String,
        /// output boc
        output_boc: String,
        /// root index (0 by default)
        #[arg(short, long)]
        root: Option<usize>,
    },
    /// Disassemble a code fragment
    Fragment {
        /// bitstring
        bitstring: String,
    },
    /// Disassemble a code boc
    Text {
        /// input boc
        boc: String,
        /// interpret the boc as StateInit and take the code cell
        #[arg(short, long)]
        stateinit: bool,
        /// print full assembler listing w/o collapsing of identical cells
        #[arg(short, long)]
        full: bool,
    },
}

fn main() -> ExitCode {
    if let Err(e) = main_impl() {
        eprintln!("{}", e);
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn main_impl() -> Status {
    let cli = Cli::parse();
    match cli.command {
        Commands::Dump { boc } => subcommand_dump(boc),
        Commands::Extract {
            boc,
            output_boc,
            index,
            root,
        } => subcommand_extract(boc, output_boc, index, root),
        Commands::Fragment { bitstring } => subcommand_fragment(bitstring),
        Commands::Text {
            boc,
            stateinit,
            full,
        } => subcommand_text(boc, stateinit, full),
    }
}

fn subcommand_dump(filename: String) -> Status {
    use tycho_types::boc::de::*;

    let tvc = std::fs::read(filename).map_err(|e| error!("failed to read boc file: {}", e))?;
    let header =
        BocHeader::decode(tvc.as_slice(), &Options::default()).map_err(|e| error!("{}", e))?;
    let roots = header.roots();
    if roots.is_empty() {
        println!("empty");
    } else {
        println!(
            "{} {} in total",
            roots.len(),
            if roots.len() > 1 { "roots" } else { "root" }
        );
        let cells = header.finalize(Cell::empty_context()).unwrap();
        for i in 0..roots.len() {
            let root = cells.get(roots[i]).unwrap();
            println!("root {}: ({} unique):", i, count_unique_cells(&root));
            print_tree_of_cells(&root);
        }
    }
    Ok(())
}

fn count_unique_cells(cell: &Cell) -> usize {
    let mut queue = vec![cell.clone()];
    let mut set = HashSet::new();
    while let Some(cell) = queue.pop() {
        if set.insert(cell.repr_hash().clone()) {
            let count = cell.reference_count();
            for i in 0..count {
                queue.push(cell.reference_cloned(i).unwrap());
            }
        }
    }
    set.len()
}

fn subcommand_extract(
    filename: String,
    output: String,
    index: usize,
    root: Option<usize>,
) -> Status {
    let boc = std::fs::read(filename).map_err(|e| error!("failed to read input file: {}", e))?;

    let header = BocHeader::decode(&boc, &Options::default())?;
    let cells = header.finalize(Cell::empty_context())?;

    let root_index = root.unwrap_or_default();
    let root = cells
        .get(root_index as u32)
        .ok_or_else(|| error!("failed to get root {}", root_index))?;

    let cell = root
        .reference_cloned(index as u8)
        .ok_or_else(|| error!("failed to get reference {}", root_index))?;

    let output_bytes = Boc::encode(&cell);
    let mut output_file = std::fs::File::create(output)?;
    output_file.write_all(&output_bytes)?;

    Ok(())
}

fn subcommand_fragment(fragment: String) -> Status {
    let cell = parse_hex_slice(&fragment)?;
    let mut slice = cell.as_slice()?;

    let mut loader = Loader::new(false);
    let code = loader.load(&mut slice, false)?;
    let text = code.print("", true, 12);

    print!("{}", text);
    Ok(())
}

fn subcommand_text(filename: String, stateinit: bool, full: bool) -> Status {
    let boc = std::fs::read(filename).map_err(|e| error!("failed to read input file: {}", e))?;
    let header = BocHeader::decode(&boc, &Options::default())?;
    let roots = header.roots();
    let cells = header.finalize(Cell::empty_context())?;

    let roots_count = roots.len();
    if roots_count == 0 {
        println!("boc is empty");
        return Ok(());
    } else if roots_count > 1 {
        println!(
            "warning: boc contains {} roots, getting the first one",
            roots_count
        )
    }

    let root0 = cells.get(roots[0]).unwrap();
    let cell = if stateinit {
        root0.reference_cloned(0).unwrap()
    } else {
        root0.clone()
    };

    print!("{}", disasm_ex(&mut cell.as_slice().unwrap(), !full)?);
    Ok(())
}
