extern crate colored; // not needed in Rust 2018

use clap::{App, Arg};
use colored::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::process::Command;

pub trait CoolColor {
    fn primary(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 141,
            g: 128,
            b: 255,
        })
    }

    fn secondary(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 255,
            g: 128,
            b: 221,
        })
    }
    fn back(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        let gray_level = 125;
        self.color(Color::TrueColor {
            r: gray_level,
            g: gray_level,
            b: gray_level,
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

#[derive(Debug)]
struct Sizes {
    program: u32,
    stack: u32,
    variables: u32,
}
struct Config {
    flash_size: u32,
    ram_size: u32,
    bootloader: u32,
}

fn main() {
    const LAST_SIZE_FILE: &str = "fw-size.last";
    let matches = App::new("pretty_size")
        .version("1.0")
        .author("Thibault Geoffroy <tg@nebularnoise.com>")
        .about("Rust re-write of wintertools/fw_size")
        .arg(
            Arg::new("elf")
                .value_name("ELF FILE")
                .about("Path to elf file")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("size-prog")
                .long("size-prog")
                .about("Path to size binary")
                .takes_value(true),
        )
        .arg(
            Arg::new("flash-size")
                .long("flash-size")
                .about("Size of FLASH memory")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("ram-size")
                .long("ram-size")
                .about("Size of RAM memory")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("bootloader-size")
                .long("bootloader-size")
                .about("Size of bootloader")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("human-readable")
                .long("human-readable")
                .short('H')
                .about("Print sizes in human-readable format -- unused -- only for backwards compatibility"),
        )
        .get_matches();

    let elf_path_str = matches.value_of("elf").unwrap();
    let elf_path = Path::new(elf_path_str);
    if !elf_path.exists() {
        panic!("{}: No such file", elf_path.display());
    }
    let size_path = matches.value_of("size-pro").unwrap_or("arm-none-eabi-size");

    let (program_size, stack_size, variables_size) = analyze_elf(elf_path_str, size_path);
    let build_dir = elf_path.parent().unwrap();
    let last_file = build_dir.join(LAST_SIZE_FILE);
    let last_file = last_file.as_path();

    let last_sizes: Option<Sizes> = if !last_file.exists() {
        None
    } else {
        let display = last_file.display();
        let mut file = match File::open(&last_file) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };
        let mut data = String::new();
        if let Err(why) = file.read_to_string(&mut data) {
            panic!("couldn't read {}: {}", display, why);
        }
        let last_data: Value = serde_json::from_str(&data).unwrap();
        let last_program_size = last_data.get("program_size").unwrap().as_u64().unwrap();
        let last_variables_size = last_data.get("variables_size").unwrap().as_u64().unwrap();

        Some(Sizes {
            program: last_program_size as u32,
            stack: stack_size,
            variables: last_variables_size as u32,
        })
    };

    print_memory_sections(
        Config {
            bootloader: matches
                .value_of("bootloader-size")
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            flash_size: matches
                .value_of("flash-size")
                .unwrap()
                .parse::<u32>()
                .unwrap(),
            ram_size: matches
                .value_of("ram-size")
                .unwrap()
                .parse::<u32>()
                .unwrap(),
        },
        Sizes {
            program: program_size,
            stack: stack_size,
            variables: variables_size,
        },
        last_sizes,
    );

    // save to last size file
    // The type of `john` is `serde_json::Value`
    let to_save = json!({
        "program_size": program_size,
        "variables_size": variables_size,
    })
    .to_string();

    let display = last_file.display();
    let mut file = match File::create(&last_file) {
        Err(why) => panic!("couldn't open {} with write priliveges: {}", display, why),
        Ok(file) => file,
    };
    if let Err(why) = file.write_all(to_save.as_bytes()) {
        panic!("couldn't write to {}: {}", display, why);
    }
}

enum Align {
    Left,
    Right,
}

fn aligned<T: std::fmt::Display, U: std::fmt::Display>(
    title: &str,
    qt1: T,
    separator: &str,
    qt2: U,
    percent: &str,
    align: Align,
) -> String {
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
    let suffix = 'B';
    let mut num = num as f32;
    for unit in ["", "Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "Zi"] {
        if num.abs() < 1024.0 {
            return format!("{:.2} {}{}", num, unit, suffix);
        }
        num = num / 1024.0
    }
    return format!("{:.2} {}{}", num, "Yi", suffix);
}

fn analyze_elf(elf: &str, size_prog: &str) -> (u32, u32, u32) {
    let fw_size_output = Command::new(size_prog)
        .args(["-A", "-d", elf])
        .output()
        .expect("failed to execute size");

    let fw_size_output = String::from_utf8(fw_size_output.stdout).unwrap();
    let mut sections = HashMap::new();

    for line in fw_size_output.lines().skip(2) {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let section_name = parts.next().unwrap();
        let section_size = parts.next().unwrap().parse::<u32>().unwrap();
        sections.insert(section_name, section_size);
    }

    let program_size = sections.get(".text").unwrap_or(&0)
        + sections.get(".relocate").unwrap_or(&0)
        + sections.get(".data").unwrap_or(&0);

    let stack_size = sections[".stack"];
    let variables_size =
        sections.get(".relocate").unwrap_or(&0) + sections[".data"] + sections[".bss"];

    return (program_size, stack_size, variables_size);
}

fn print_memory_sections(config: Config, sizes: Sizes, previous_sizes: Option<Sizes>) {
    let size_to_flash_ratio = |size: u32| (size as f32) / (config.flash_size as f32);
    let ratio_to_bars = |ratio: f32| (ratio * BAR_LENGTH as f32) as u8;
    let ratio_to_percent_str = |ratio: f32| ((ratio * 100.) as u8).to_string();
    let used_flash = sizes.program + config.bootloader;
    let percent = (100. * size_to_flash_ratio(used_flash)) as u8;

    println!();
    println!();
    println!(
        "{}",
        aligned(
            "Flash used",
            sizeof_fmt(used_flash).primary(),
            "/",
            sizeof_fmt(config.flash_size),
            &percent.to_string(),
            Align::Right
        )
    );
    let bootloader_ratio = size_to_flash_ratio(config.bootloader);
    let bootloader_bars = ratio_to_bars(bootloader_ratio);
    let program_ratio = size_to_flash_ratio(sizes.program);
    let program_bars = ratio_to_bars(program_ratio);
    println!(
        "{}{}{}",
        (0..bootloader_bars)
            .map(|_| "▓")
            .collect::<String>()
            .secondary(),
        (0..program_bars).map(|_| "▓").collect::<String>().primary(),
        (program_bars + bootloader_bars..BAR_LENGTH)
            .map(|_| "░")
            .collect::<String>()
            .back()
    );
    let added = match &previous_sizes {
        None => "".back(),
        Some(prev_s) => {
            let diff = (sizes.program as i64) - (prev_s.program as i64);
            match diff >= 0 {
                true if (diff == 0) => "".back(),
                true => format!("+{}", diff.to_string()).yellow(),
                false => diff.to_string().mint(),
            }
        }
    };
    println!(
        "{}\n{}\n\n",
        aligned(
            "Bootloader",
            sizeof_fmt(config.bootloader),
            "",
            "",
            &ratio_to_percent_str(bootloader_ratio),
            Align::Left
        )
        .secondary(),
        aligned(
            "Program",
            sizeof_fmt(sizes.program),
            "",
            added,
            &ratio_to_percent_str(program_ratio),
            Align::Left
        )
        .primary(),
    );

    let size_to_ram_ratio = |size: u32| (size as f32) / (config.ram_size as f32);
    let used_ram = sizes.stack + sizes.variables;
    let used_ram_ratio = size_to_ram_ratio(used_ram);
    println!(
        "{}",
        aligned(
            "RAM used",
            sizeof_fmt(used_ram).primary(),
            "/",
            sizeof_fmt(config.ram_size),
            &ratio_to_percent_str(used_ram_ratio),
            Align::Right
        )
    );
    let stack_ratio = size_to_ram_ratio(sizes.stack);
    let stack_bars = ratio_to_bars(stack_ratio);
    let variables_ratio = size_to_ram_ratio(sizes.variables);
    let variables_bars = ratio_to_bars(variables_ratio);
    println!(
        "{}{}{}",
        (0..stack_bars).map(|_| "▓").collect::<String>().secondary(),
        (0..variables_bars)
            .map(|_| "▓")
            .collect::<String>()
            .primary(),
        (stack_bars + variables_bars..BAR_LENGTH)
            .map(|_| "░")
            .collect::<String>()
            .back()
    );

    let added = match &previous_sizes {
        None => "".back(),
        Some(prev_s) => {
            let diff = (sizes.variables as i64) - (prev_s.variables as i64);
            match diff >= 0 {
                true if (diff == 0) => "".back(),
                true => format!("+{}", diff.to_string()).yellow(),
                false => diff.to_string().mint(),
            }
        }
    };

    println!(
        "{}\n{}",
        aligned(
            "Stack",
            sizeof_fmt(sizes.stack),
            "",
            "",
            &ratio_to_percent_str(stack_ratio),
            Align::Left
        )
        .secondary(),
        aligned(
            "Variables",
            sizeof_fmt(sizes.variables),
            "",
            added,
            &ratio_to_percent_str(variables_ratio),
            Align::Left
        )
        .primary()
    );
    println!();
    println!();
}
