extern crate ldscript_parser as lds;

use clap::{arg, value_parser, Command};
use color_eyre::eyre::{ensure, eyre, Context, ContextCompat};
use color_eyre::Result;
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

mod size;
use size::Section;
mod display;

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

trait Absolutify {
    fn as_absolute(&self) -> Result<PathBuf>
    where
        Self: AsRef<Path>,
    {
        let absolute_path = if self.as_ref().is_absolute() {
            self.as_ref().to_path_buf()
        } else {
            std::env::current_dir()
                .with_context(|| "Failed to retreive current directory")?
                .join(self)
        }
        .clean();

        Ok(absolute_path)
    }
}

impl Absolutify for PathBuf {}
impl Absolutify for Path {}

fn main() -> Result<()> {
    color_eyre::install()?;
    const LAST_SIZE_FILE: &str = "fw-size.last";
    let matches = Command::new("pretty_size")
        .version("1.1.0")
        .author("Thibault Geoffroy <tg@nebularnoise.com>")
        .about("Rust re-write of wintertools/fw_size")
        .arg(arg!(<elf> "Path to elf file").value_parser(value_parser!(PathBuf)))
        .arg(arg!(--ld <LD_FILE> "Path to linker script to parse").value_parser(value_parser!(PathBuf)))
        .arg(arg!(-e --"section-edits" <EDITS> "Path to the json file describing operations to apply to group regions and ignore sections in the generated report").value_parser(value_parser!(PathBuf)).required(false))
        .get_matches();

    let elf_path = matches
        .get_one::<PathBuf>("elf")
        .expect("required")
        .as_absolute()?;
    ensure!(elf_path.is_file(), "File not found: {}", elf_path.display());

    let linker_script_path = matches
        .get_one::<PathBuf>("ld")
        .expect("required")
        .as_absolute()?;

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

    let sections = size::size(elf_path.to_str().expect("valid"))
        .with_context(|| format!("Could not read sections of file \"{}\"", elf_path.display()))?;

    let edits_file = matches
        .get_one::<PathBuf>("section-edits")
        .map(|p| p.as_absolute().expect("Permission needed"));

    let memory_layout =
        parse_ld_memory_layout(&linker_script_path, &sections, edits_file.as_deref())
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
        display::print_region(&region.name, region.length as u32, &sections_with_diff);
    }
    println!();
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
    linker_script_path: &Path,
    sections_sizes: &[Section],
    edits_file: Option<&Path>,
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
        format!("File not found: {}", edits_file.display())
    );

    let mut file = File::open(edits_file)
        .with_context(|| format!("Couldn't open edits file \"{}\"", display))?;

    let mut data = String::new();
    file.read_to_string(&mut data)
        .with_context(|| format!("Couldn't read edits file \"{}\"", display))?;

    serde_json::from_str(&data).context("Parsing edits failed.")
}
