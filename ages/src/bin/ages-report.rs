//! `ages-report` — read an NDJSON event log produced by `ages`,
//! render a post-run report (markdown or HTML). Pure consumer; the
//! sim binary `ages` is unaffected.
//!
//! Usage:
//!   ages-report --in events.ndjson --out report.md
//!   ages-report --in events.ndjson --format html --out report.html
//!   ages-report < events.ndjson > report.md  # pipe-friendly

use anyhow::{Context, Result};
use sim_report::{render_from_reader, render_html_from_reader};
use std::fs::File;
use std::io::{stdin, stdout, BufReader, Read, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Markdown,
    Html,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let render = |reader: Box<dyn Read>| -> Result<String> {
        Ok(match args.format {
            Format::Markdown => render_from_reader(reader)?,
            Format::Html => render_html_from_reader(reader)?,
        })
    };
    let report = match &args.input {
        Some(path) => {
            let f = File::open(path).with_context(|| format!("could not open {path}"))?;
            render(Box::new(BufReader::new(f)))?
        }
        None => render(Box::new(BufReader::new(stdin().lock())))?,
    };
    if let Some(path) = args.output {
        let mut out = File::create(&path).with_context(|| format!("could not create {path}"))?;
        out.write_all(report.as_bytes())?;
    } else {
        let mut out = stdout().lock();
        out.write_all(report.as_bytes())?;
    }
    Ok(())
}

#[derive(Debug)]
struct Args {
    input: Option<String>,
    output: Option<String>,
    format: Format,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            format: Format::Markdown,
        }
    }
}

fn parse_args() -> Result<Args> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--in" => {
                args.input = Some(iter.next().context("--in needs a path")?);
            }
            "--out" => {
                args.output = Some(iter.next().context("--out needs a path")?);
            }
            "--format" => {
                let raw = iter.next().context("--format needs a value")?;
                args.format = match raw.as_str() {
                    "markdown" | "md" => Format::Markdown,
                    "html" => Format::Html,
                    other => anyhow::bail!("--format expects markdown or html, got {other}"),
                };
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    Ok(args)
}

fn print_help() {
    println!(
        "ages-report — render a post-run report from an NDJSON event log.\n\
         \n\
         USAGE:\n  \
             ages-report [--in <path>] [--out <path>] [--format markdown|html]\n\
         \n\
         OPTIONS:\n  \
             --in     <path>   path to NDJSON event log (default: stdin)\n  \
             --out    <path>   output path (default: stdout)\n  \
             --format <fmt>    markdown (default) or html\n"
    );
}
