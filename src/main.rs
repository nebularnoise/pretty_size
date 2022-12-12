extern crate ldscript_parser as lds;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::{Context, Result};
use clap::{Arg, Command};
use colored::*;
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RegionWithSections {
    pub name: String,
    pub length: u64,
    pub sections: Vec<(String, u32)>,
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
    const LAST_SIZE_FILE: &str = "fw-size.last";
    let matches = Command::new("pretty_size")
        .version("1.0.1")
        .author("Thibault Geoffroy <tg@nebularnoise.com>")
        .about("Rust re-write of wintertools/fw_size")
        .arg(
            Arg::new("elf")
                .value_name("ELF FILE")
                .help("Path to elf file")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("ld")
                .long("ld")
                .value_name("LD FILE")
                .help("Path to linker script to parse")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("size-prog")
                .long("size-prog")
                .help("Path to size binary")
                .takes_value(true),
        )
        .arg(
            Arg::new("section-edits")
                .long("section edits")
                .short('e')
                .help("Path to the json file describing operations to apply to group regions and ignore sections in the generated report")
                .takes_value(true),
        )
        .get_matches();

    let elf_path_str = matches.value_of("elf").unwrap();
    let elf_path = Path::new(elf_path_str);
    ensure!(elf_path.exists(), "{}: No such file", elf_path.display());
    let size_path = matches
        .value_of("size-prog")
        .unwrap_or("arm-none-eabi-size");

    let linker_script_path = matches.value_of("ld").unwrap();

    let build_dir = elf_path.parent().unwrap();
    let last_file = build_dir.join(LAST_SIZE_FILE);
    let last_file = last_file.as_path();

    let last_sizes: Option<Vec<RegionWithSections>> = if !last_file.exists() {
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

    let sections = get_sections_sizes(elf_path_str, size_path).with_context(|| {
        format!(
            "Could not read sections of file \"{}\" with executable \"{}\"",
            elf_path_str, size_path
        )
    })?;

    let edits_file = matches.value_of("section-edits").unwrap_or("");
    let edits_file = Path::new(edits_file);

    let regions_with_sections =
        get_regions_and_sections_from_linker_script(linker_script_path, &sections, edits_file)
            .with_context(|| "Failed to fetch regions and sections from linker script")?;

    print_memory_sections(&regions_with_sections, last_sizes.as_ref());

    // save to last size file
    let to_save = serde_json::to_string(&regions_with_sections)
        .with_context(|| "Failed to serialize size information")?;

    let display = last_file.display();
    let mut file =
        File::create(&last_file).with_context(|| format!("Could not write to {}", display))?;
    file.write_all(to_save.as_bytes())
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
        num = num / 1024.0
    }
    return format!("{:.2} YiB", num);
}

fn get_sections_sizes(elf: &str, size_prog: &str) -> Result<HashMap<String, u32>> {
    let fw_size_output = std::process::Command::new(size_prog)
        .args(["-A", "-d", elf])
        .output()
        .expect("failed to execute size");

    let fw_size_output = String::from_utf8(fw_size_output.stdout)?;
    let mut sections = HashMap::new();

    for line in fw_size_output.lines().skip(2) {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let section_name = parts.next().unwrap();
        let section_size = parts.next().unwrap().parse::<u32>().unwrap();
        if section_size == 0 {
            continue;
        }
        if let Some(addr_str) = parts.next() {
            if let Ok(addr) = addr_str.parse::<u32>() {
                if addr == 0 {
                    continue;
                }
                sections.insert(section_name.to_owned(), section_size);
            }
        }
    }

    return Ok(sections);
}

fn print_memory_sections(
    regions: &Vec<RegionWithSections>,
    regions_prev: Option<&Vec<RegionWithSections>>,
) {
    println!();
    println!();

    regions.iter().for_each(|reg| {
        let prev_sections = regions_prev.and_then(|prev_reg| {
            prev_reg
                .iter()
                .find(|reg_prev| reg_prev.name == reg.name)
                .and_then(|reg_prev| Some(&reg_prev.sections))
        });

        let sections_with_diff = match &prev_sections {
            None => reg
                .sections
                .iter()
                .map(|(name, size)| (name.clone(), *size, 0))
                .collect::<Vec<(String, u32, i64)>>(),
            Some(prev_s) => reg
                .sections
                .iter()
                .map(|(name, size)| {
                    let previous_size = prev_s
                        .iter()
                        .find(|&(prev_name, _)| name == prev_name)
                        .and_then(|(_, prev_size)| Some(*prev_size))
                        .unwrap_or(*size);
                    (name.clone(), *size, *size as i64 - previous_size as i64)
                })
                .collect::<Vec<(String, u32, i64)>>(),
        };

        print_region(reg.name.as_str(), reg.length as u32, &sections_with_diff);
    });
    println!();
    println!();
}

fn print_region(name: &str, region_size: u32, sections: &Vec<(String, u32, i64)>) {
    let total_usage: u32 = sections.iter().fold(0, |acc, x| acc + x.1);
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
        .map(|&(_, ssize, _)| {
            let ratio = size_to_ratio(ssize);
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
        .for_each(|(i, (name, size, diff))| {
            let ratio = size_to_ratio(*size);

            let diff_string = match *diff >= 0 {
                true if (*diff == 0) => "".black(),
                true => format!("+{}", sizeof_fmt(*diff as u32)).yellow(),
                false => ("-".to_owned() + &sizeof_fmt((-diff) as u32)).mint(),
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

fn get_regions_and_sections_from_linker_script(
    linker_script_path: &str,
    sections_sizes: &HashMap<String, u32>,
    edits_file: &Path,
) -> Result<Vec<RegionWithSections>> {
    let script = &mut String::new();
    File::open(linker_script_path)
        .with_context(|| format!("Invalid linker script path \"{}\"", linker_script_path))?
        .read_to_string(script)
        .with_context(|| {
            format!(
                "Could not read linker script \"{}\" as string",
                linker_script_path
            )
        })?;

    let parsed_vec = lds::parse(script)
        .map_err(|_| anyhow!("Failed to parse linker script \"{}\"", linker_script_path))?;

    // mem regions
    let regions = parsed_vec
        .iter()
        .filter_map(|item| match item {
            lds::RootItem::Memory { regions } => Some(regions),
            _ => None,
        })
        .next()
        .with_context(|| "Could not find REGIONS")?;

    let sections: Vec<(&String, &String, Option<&String>)> = parsed_vec
        .iter()
        .filter_map(|item| match item {
            lds::RootItem::Sections { list } => Some(list),
            _ => None,
        })
        .flat_map(|e| e.iter())
        .filter_map(|sec_command| match sec_command {
            lds::SectionCommand::OutputSection {
                name,
                region,
                lma_region,
                ..
            } => Some((name, region, lma_region)),
            _ => None,
        })
        .filter(|(_, reg, _)| reg.is_some())
        .map(|(name, reg, lma_reg)| (name, reg.as_ref().unwrap(), lma_reg.as_ref()))
        .collect();
    // println!("{:#?}", sections);

    let mut regions_better: Vec<RegionWithSections> = regions
        .iter()
        .map(|reg| {
            let mut sections: Vec<(String, u32)> = sections
                .iter()
                .filter(|&&(_name, reg_name, lma_reg_name)| {
                    (reg_name == &reg.name)
                        || (lma_reg_name.map_or("", |s| s.as_str()) == reg.name.as_str())
                })
                .filter_map(|&(name, ..)| match sections_sizes.get(name.as_str()) {
                    Some(size) => match size {
                        0 => None,
                        _ => Some((name.clone(), *size)),
                    },
                    _ => None,
                })
                .collect();

            let misc_sections = drain_filter(&mut sections, |(_name, size)| {
                let percentage = 100.0 * (*size as f64) / (reg.length as f64);
                percentage < 2.0
            });

            if !misc_sections.is_empty() {
                let misc_size = misc_sections
                    .iter()
                    .fold(0u32, |acc, &(_, size)| acc + size);

                sections.push(("miscellaneous".to_owned(), misc_size));
            }

            RegionWithSections {
                name: reg.name.clone(),
                length: reg.length,
                sections: sections,
            }
        })
        .collect();

    let section_edits = get_section_edits(edits_file)?;

    if let Some(section_edits) = section_edits {
        section_edits.iter().for_each(|s_e| match s_e {
            SectionEdit::GroupRegions {
                region_to_insert_as_section,
                output_region,
                output_section_name,
            } => {
                if let Some(index) = regions_better
                    .iter()
                    .position(|reg| &reg.name == region_to_insert_as_section)
                {
                    let bootloader_reg = regions_better.remove(index);
                    if let Some(reg) = regions_better
                        .iter_mut()
                        .find(|reg| &reg.name == output_region)
                    {
                        reg.sections.insert(
                            0,
                            (output_section_name.clone(), bootloader_reg.length as u32),
                        );
                        reg.length += bootloader_reg.length;
                    }
                }
            }
            SectionEdit::Ignore {
                region_name,
                section_name_to_ignore,
            } => {
                if let Some(reg) = regions_better
                    .iter_mut()
                    .find(|reg| &reg.name == region_name)
                {
                    if let Some(index) = reg
                        .sections
                        .iter()
                        .position(|(sec_name, _)| sec_name == section_name_to_ignore)
                    {
                        reg.sections.remove(index);
                    }
                }
            }
        });
    }

    return Ok(regions_better);
}

fn get_section_edits(edits_file: &Path) -> Result<Option<Vec<SectionEdit>>> {
    let display = edits_file.display();
    if !edits_file.exists() {
        let edits_file_absolute_path = absolute_path(edits_file)
            .with_context(|| format!("Couldn't resolve edits file path \"{}\"", display))?;
        bail!(
            "Section edits file does not exist: {:?}",
            edits_file_absolute_path
        );
    }
    let mut file = File::open(&edits_file)
        .with_context(|| format!("Couldn't open edits file \"{}\"", display))?;

    let mut data = String::new();
    file.read_to_string(&mut data)
        .with_context(|| format!("Couldn't read edits file \"{}\"", display))?;

    Ok(serde_json::from_str(&data).ok())
}

pub fn absolute_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();

    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
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
