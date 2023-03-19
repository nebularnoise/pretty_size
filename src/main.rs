extern crate ldscript_parser as lds;

use clap::{arg, value_parser, Command};
use color_eyre::eyre::{ensure, eyre, Context, ContextCompat};
use color_eyre::Result;
use colored::*;
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Section(String, u32);

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DetailedRegion {
    pub name: String,
    pub length: u64,
    pub sections: Vec<Section>,
}

#[derive(Serialize, Deserialize, Debug)]
enum SectionEdit {
    GroupRegions {
        region_to_insert_as_section: String,
        output_region: String,
        output_section_name: String,
    },
    Ignore {
        region_name: String,
        section_name_to_ignore: String,
    },
}

pub trait CoolColor {
    fn s_purple(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 141,
            g: 128,
            b: 255,
        })
    }

    fn s_pink(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 255,
            g: 128,
            b: 221,
        })
    }

    fn mint(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 127,
            g: 255,
            b: 191,
        })
    }
}
impl<'a> CoolColor for &'a str {}

const BAR_LENGTH: u8 = 47;

fn main() -> Result<()> {
    color_eyre::install()?;
    const LAST_SIZE_FILE: &str = "fw-size.last";
    let matches = Command::new("pretty_size")
        .version("1.1.0")
        .author("Thibault Geoffroy <tg@nebularnoise.com>")
        .about("Rust re-write of wintertools/fw_size")
        .arg(arg!(<elf> "Path to elf file").value_parser(value_parser!(PathBuf)))
        .arg(arg!(--ld <LD_FILE> "Path to linker script to parse").value_parser(value_parser!(PathBuf)))
        .arg(arg!(sizeprog: -s --"size-prog" <SIZE> "Path to size binary")
    .default_value("arm-none-eabi-size")
    )
        .arg(arg!(-e --"section-edits" <EDITS> "Path to the json file describing operations to apply to group regions and ignore sections in the generated report").value_parser(value_parser!(PathBuf)).required(false))
        .get_matches();

    let elf_path = matches.get_one::<PathBuf>("elf").expect("required");
    ensure!(elf_path.is_file(), "File not found: {}", elf_path.display());

    let size_path = matches
        .get_one::<String>("sizeprog")
        .expect("default ensures there is always a value");

    let linker_script_path = matches.get_one::<PathBuf>("ld").expect("required");

    let build_dir = elf_path
        .parent()
        .expect("Elf path is a valid file, it should have a parent dir");
    let last_file = build_dir.join(LAST_SIZE_FILE);

    let last_sizes: Option<Vec<DetailedRegion>> = if !last_file.is_file() {
        None
    } else {
        let display = last_file.display();
        let mut file =
            File::open(&last_file).with_context(|| format!("Couldn't open {}", display))?;

        let mut data = String::new();
        file.read_to_string(&mut data)
            .with_context(|| format!("Couldn't read {}", display))?;

        serde_json::from_str(&data).ok()
    };

    let sections =
        analyze_elf(elf_path.to_str().expect("valid"), size_path).with_context(|| {
            format!(
                "Could not read sections of file \"{}\" with executable \"{}\"",
                elf_path.display(),
                size_path
            )
        })?;

    let edits_file = matches.get_one::<PathBuf>("section-edits");

    let memory_layout = parse_ld_memory_layout(linker_script_path, &sections, edits_file)
        .with_context(|| "Failed to parse memory layout from linker script")?;

    // save to last size file
    let to_save = serde_json::to_string(&memory_layout)
        .with_context(|| "Failed to serialize size information")?;

    print_memory_layout(memory_layout, last_sizes.as_deref());

    let display = last_file.display();
    File::create(&last_file)
        .with_context(|| format!("Could not create {}", display))?
        .write_all(to_save.as_bytes())
        .with_context(|| format!("Could not write to {}", display))?;

    Ok(())
}

enum Align {
    Left,
    Right,
}

fn aligned<T, U>(
    title: &str,
    qt1: T,
    separator: &str,
    qt2: U,
    percent: &str,
    align: Align,
) -> String
where
    T: std::fmt::Display,
    U: std::fmt::Display,
{
    match align {
        Align::Left => format!(
            "{}:{:>width$}  {:1}{:<12}{:>7}",
            title,
            qt1,
            separator,
            qt2,
            format!("({}%)", percent),
            width = (24 - title.len())
        ),
        Align::Right => format!(
            "{}:{:>width$}  {:1}{:>12}{:>7}",
            title,
            qt1,
            separator,
            qt2,
            format!("({}%)", percent),
            width = (24 - title.len())
        ),
    }
}

fn sizeof_fmt(num: u32) -> String {
    if num < 1024 {
        return format!("{}", num);
    }
    let mut num = num as f32 / 1024.0;
    for unit in ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "Zi"] {
        if num.abs() < 1024.0 {
            return format!("{:.2} {}B", num, unit);
        }
        num /= 1024.0
    }
    format!("{:.2} YiB", num)
}

use regex::Regex;

fn analyze_elf(elf: &str, size_prog: &str) -> Result<Vec<Section>> {
    let fw_size_output = std::process::Command::new(size_prog)
        .args(["-A", "-d", elf])
        .output()
        .with_context(|| format!("failed to execute \"{}\"", size_prog))?;

    let fw_size_output = String::from_utf8(fw_size_output.stdout)?;

    let re = Regex::new(
        r"(?imx)^
    (?P<name>\.[\w_\.\-\d]+) # section_name
    \s+
    (?P<size>\d+) # size
    \s+
    (?P<addr>\d+) # size
  ",
    )
    .unwrap();

    let sections: Vec<Section> = re
        .captures_iter(&fw_size_output)
        .map(|cap| {
            let name = cap.name("name").unwrap().as_str();
            let size = cap.name("size").unwrap().as_str().parse::<u32>().unwrap();
            let addr = cap.name("addr").unwrap().as_str().parse::<u32>().unwrap();
            (name, size, addr)
        })
        .filter_map(|(name, size, addr)| (addr != 0).then_some(Section(name.to_owned(), size)))
        .collect();

    Ok(sections)
}

fn compute_sections_size_diff(
    sections: Vec<Section>,
    prev_sections: Option<&Vec<Section>>,
) -> Vec<(Section, i64)> {
    if let Some(previous_sections) = prev_sections {
        sections
            .iter()
            .map(|sec| {
                let Section(name, size) = sec;
                let previous_size = previous_sections
                    .iter()
                    .find(|Section(prev_name, _)| name == prev_name)
                    .map(|Section(_, prev_size)| *prev_size)
                    .unwrap_or(*size);
                (sec.clone(), *size as i64 - previous_size as i64)
            })
            .collect::<Vec<(Section, i64)>>()
    } else {
        sections
            .iter()
            .map(|sec| (sec.clone(), 0))
            .collect::<Vec<(Section, i64)>>()
    }
}

fn print_memory_layout(
    regions: Vec<DetailedRegion>,
    previous_memory_layout: Option<&[DetailedRegion]>,
) {
    println!();
    println!();
    for region in regions {
        let prev_sections = previous_memory_layout.and_then(|layout| {
            layout
                .iter()
                .find(|reg_prev| reg_prev.name == region.name)
                .map(|reg_prev| &reg_prev.sections)
        });
        let sections_with_diff = compute_sections_size_diff(region.sections, prev_sections);
        print_region(&region.name, region.length as u32, &sections_with_diff);
    }
    println!();
    println!();
}

fn print_region(name: &str, region_size: u32, sections: &[(Section, i64)]) {
    let total_usage: u32 = sections.iter().fold(0, |acc, x| acc + x.0 .1);
    let size_to_ratio = |size: u32| (size as f32) / (region_size as f32);
    let percent = (100. * size_to_ratio(total_usage)).round() as u8;
    println!(
        "{}",
        aligned(
            format!("{} used", name).as_str(),
            sizeof_fmt(total_usage).s_purple(),
            "/",
            sizeof_fmt(region_size),
            &percent.to_string(),
            Align::Right
        )
    );

    let bars = sections
        .iter()
        .map(|&(Section(_, size), _)| {
            let ratio = size_to_ratio(size);
            (ratio * BAR_LENGTH as f32).round() as u8
        })
        .collect::<Vec<u8>>();

    bars.iter().enumerate().for_each(|(i, val)| {
        let uncolored_bar = (0..*val).map(|_| "▓").collect::<String>();
        match i % 2 == 0 {
            true => print!("{}", uncolored_bar.s_pink()),
            _ => print!("{}", uncolored_bar.s_purple()),
        }
    });

    let used_bars: u8 = bars.iter().sum();
    println!(
        "{}",
        (used_bars..BAR_LENGTH).map(|_| "░").collect::<String>()
    );

    let ratio_to_percent_str = |ratio: f32| ((ratio * 100.).round() as u8).to_string();

    sections
        .iter()
        .enumerate()
        .for_each(|(i, (Section(name, size), diff))| {
            let ratio = size_to_ratio(*size);

            let diff_string = {
                use std::cmp::Ordering::*;
                match diff.cmp(&0) {
                    Equal => "".black(),
                    Greater => format!("+{}", sizeof_fmt(*diff as u32)).yellow(),
                    Less => format!("-{}", sizeof_fmt(-(*diff) as u32)).mint(),
                }
            };

            let uncolored = aligned(
                name.as_str(),
                sizeof_fmt(*size),
                "",
                diff_string,
                &ratio_to_percent_str(ratio),
                Align::Left,
            );

            match i % 2 == 0 {
                true => println!("{}", uncolored.s_pink()),
                _ => println!("{}", uncolored.s_purple()),
            }
        });
    println!();
}

fn drain_filter<T, F>(vec: &mut Vec<T>, predicate: F) -> Vec<T>
where
    F: Fn(&mut T) -> bool,
{
    let mut ret: Vec<T> = vec![];
    let mut i = 0;
    while i < vec.len() {
        if predicate(&mut vec[i]) {
            ret.push(vec.remove(i));
        } else {
            i += 1;
        }
    }
    ret
}

fn parse_ld_memory_layout(
    linker_script_path: &PathBuf,
    sections_sizes: &[Section],
    edits_file: Option<&PathBuf>,
) -> Result<Vec<DetailedRegion>> {
    let script = std::fs::read_to_string(linker_script_path).with_context(|| {
        format!(
            "Could not read linker script \"{}\" as string",
            linker_script_path.display()
        )
    })?;
    let parsed_ldscript = lds::parse(&script).map_err(|_| {
        eyre!(
            "Failed to parse linker script \"{}\"",
            linker_script_path.display()
        )
    })?;

    let memory_regions_definitions = parsed_ldscript
        .iter()
        .find_map(|item| match item {
            lds::RootItem::Memory { regions } => Some(regions),
            _ => None,
        })
        .with_context(|| "Could not find REGIONS")?;

    let output_section_definitions: Vec<(&String, &String, Option<&String>)> = parsed_ldscript
        .iter()
        .filter_map(|item| match item {
            lds::RootItem::Sections { list } => Some(list),
            _ => None,
        })
        .flat_map(|e| e.iter())
        .filter_map(|sec_command| match sec_command {
            lds::SectionCommand::OutputSection {
                name,
                region: Some(region),
                lma_region,
                ..
            } => Some((name, region, lma_region)),
            _ => None,
        })
        .map(|(name, reg, lma_reg)| (name, reg, lma_reg.as_ref()))
        .collect();

    let mut regions: Vec<DetailedRegion> = memory_regions_definitions
        .iter()
        .map(|reg| {
            let mut sections: Vec<Section> = sections_sizes
                .iter()
                .filter(|Section(name, size)| {
                    *size != 0 && {
                        output_section_definitions.iter().any(
                            |&(output_section_name, reg_name, lma_reg_name)| {
                                let goes_in_this_region = (reg_name == &reg.name)
                                    || (lma_reg_name
                                        .map_or(false, |s| s.as_str() == reg.name.as_str()));
                                output_section_name == name && goes_in_this_region
                            },
                        )
                    }
                })
                .cloned()
                .collect();

            let misc_sections = drain_filter(&mut sections, |Section(_name, size)| {
                let percentage = 100.0 * (*size as f64) / (reg.length as f64);
                percentage < 2.0
            });

            if !misc_sections.is_empty() {
                let misc_size = misc_sections.iter().map(|Section(_, size)| size).sum();

                sections.push(Section("miscellaneous".to_owned(), misc_size));
            }

            DetailedRegion {
                name: reg.name.clone(),
                length: reg.length,
                sections,
            }
        })
        .collect();

    if let Some(edits_file_path) = edits_file {
        for edit in deserialize_section_edits(edits_file_path)? {
            edit.apply_to(&mut regions);
        }
    }

    Ok(regions)
}

impl SectionEdit {
    fn apply_to(self, regions: &mut Vec<DetailedRegion>) -> Option<()> {
        match self {
            SectionEdit::GroupRegions {
                region_to_insert_as_section,
                output_region: output_region_name,
                output_section_name,
            } => {
                let reg2sec = regions.remove(
                    regions
                        .iter()
                        .position(|reg| reg.name == region_to_insert_as_section)?,
                );

                let output_region = regions
                    .iter_mut()
                    .find(|reg| reg.name == output_region_name)?;

                output_region
                    .sections
                    .insert(0, Section(output_section_name, reg2sec.length as u32));
                output_region.length += reg2sec.length;

                Some(())
            }
            SectionEdit::Ignore {
                region_name,
                section_name_to_ignore,
            } => {
                let reg = regions.iter_mut().find(|reg| reg.name == region_name)?;
                reg.sections.remove(
                    reg.sections
                        .iter()
                        .position(|Section(sec_name, _)| sec_name == &section_name_to_ignore)?,
                );
                Some(())
            }
        }
    }
}

fn deserialize_section_edits(edits_file: &Path) -> Result<Vec<SectionEdit>> {
    let display = edits_file.display();
    ensure!(
        edits_file.is_file(),
        format!(
            "File not found: {}",
            absolute_path(edits_file)
                .with_context(|| format!("Couldn't resolve edits file path \"{}\"", display))?
                .display()
        )
    );

    let mut file = File::open(edits_file)
        .with_context(|| format!("Couldn't open edits file \"{}\"", display))?;

    let mut data = String::new();
    file.read_to_string(&mut data)
        .with_context(|| format!("Couldn't read edits file \"{}\"", display))?;

    serde_json::from_str(&data).context("Parsing edits failed.")
}

pub fn absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    }
    .clean();

    Ok(absolute_path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizeof_fmt() {
        assert_eq!(crate::sizeof_fmt(32), "32 B");
        assert_eq!(crate::sizeof_fmt(128), "128 B");
        assert_eq!(crate::sizeof_fmt(1024), "1.00 KiB");
        assert_eq!(crate::sizeof_fmt(1024 * 2), "2.00 KiB");
        assert_eq!(crate::sizeof_fmt(1024 * 1024), "1.00 MiB");
    }
}
